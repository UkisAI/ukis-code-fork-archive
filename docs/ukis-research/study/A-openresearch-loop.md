# OpenResearch (`orx`) - core agent loop and orchestration

Repo: `/home/pavle/projekti/vendor/OpenResearch` (alphaXiv, Rust crate `openresearch-cli` v0.2.14, binary `orx`; HEAD `ea3b968`).
Scope: read-only study. All refs are `path:line` in that repo.

---

## 0. TL;DR

- **OpenResearch has no LLM loop of its own.** It is a *harness orchestrator* plus an *experiment ledger*. The "agent" is whichever coding CLI you pick (Claude Code, Codex, OpenCode, Cursor, Antigravity). `orx` spawns it, injects a system prompt ("playbook") and a set of native skills into a private git worktree, then streams and normalizes its events into a web dashboard.
- **The research loop runs inside the LLM, driven by prose doctrine** (`SKILL.md`, `agent-skills/orx-experiment-tree/SKILL.md`). The Rust side gives it primitives: `create-experiment` (git branch plus DB row), `exp run` (immutable snapshot plus detached supervisor), `exp wait --project` (edge-triggered "first completion" sleep), `exp wake` (durable callback that re-enters the chat when a run ends), `logs`, `runs`.
- **Nothing in the code parses metrics, compares results, picks winners or enforces node freezing.** All of that is LLM judgment over raw stdout logs, steered by doctrine. The code does enforce: one in-flight run per node (unless `--force`), committed-code-only snapshots, the run command copied at node creation, a cap of 5 helper agents and depth 1, and read-only gating in plan mode.
- The valuable IP is (a) the **doctrine**: fixed run contract, frozen nodes, "stacked bushes" tree shape, the repair/refill/promote/stop decision after each completion, repair caps, evidence contract; and (b) the **plumbing**: resident harness children, durable wakeups, content-addressed source snapshots, and one Harness trait over 5 CLIs.

---

## 1. Architecture

### 1.1 Binary / entry points
- `src/main.rs` (1424 lines): clap `enum Command` at `src/main.rs:62-181`. User verbs: `login/logout`, `projects`, `project view|edit`, `agent spawn`, `runs`, `logs`, `create-experiment`, `compute`, `instance`, `ssh-key`, `exp {status,desc,run,cancel,wake,wait,archive,unarchive}`, `skill`, `skills`, `templates`, `install-skills`, `discover`, `paper`, `version`, `update`, `install-cli`, `delete`, `telemetry`, `feedback`.
- Daemon / internal verbs: `serve` (loopback HTTP/SSE over the run store, `main.rs:136-138`), `supervise <runId>` (the detached per-run watcher, `main.rs:140-142`), `up` (dashboard + API + harness host, `main.rs:144-146`), and the hidden gates `plan-gate`, `invocation-gate`, `mcp-gate`, `antigravity-gate`, `publish-branch`, `remote-host` (`main.rs:155-180`).
- Stack: tokio, axum 0.8 (HTTP/WS/SSE), rusqlite (bundled), rust-embed (the React UI in `ui/dist` is embedded), sha2, fd-lock. `Cargo.toml`.

### 1.2 Module map
| Area | Files | Role |
|---|---|---|
| Dashboard / API server | `src/commands/up.rs` (8583 lines, ~87 axum routes), `src/commands/up_remote.rs` | `orx up` on `127.0.0.1:4791`: UI, JSON/SSE API, starts the background `watch_runs` task (`up.rs:167`) |
| Chat host / turn engine | `src/local/chat/mod.rs` (11,226 lines) | `ChatHost`: sessions, turns, queueing, steering, forks (message tree), permission cards, wakeups, spawns |
| Harness adapters | `src/local/harness/{mod,claude,codex,opencode,opencode_v2,cursor,antigravity,detect,options,plan_gate,title}.rs` | One `Harness` trait, 5 impls |
| Harness processes | `src/local/claude.rs` (resident Claude child), `src/local/codex.rs` (Codex app-server JSON-RPC), `src/local/opencode.rs` (opencode serve + **playbook rendering**) | |
| Experiment model | `src/local/experiments.rs`, `src/local/model.rs`, `src/plane/local_plane.rs`, `src/commands/{exp,create_experiment,project,runs,logs}.rs` | Tree of nodes = git branches |
| Git | `src/local/git.rs` (2788) | Shells out to `git`: hub clone, session worktrees, branches, diffs |
| Compute | `src/compute.rs` (ComputeBackend trait + SourceSnapshot), `src/jobs/*` (hf, k8s, modal, ray, slurm, ssh, localbox, openresearch, tinker), `src/local/{hf,k8s,modal,ray,slurm,ssh,localrun,openresearch}.rs` | 9 backends |
| Supervisor | `src/commands/supervise.rs` (1460) | Detached per-run process: tail logs, poll status, honor cancel |
| Store | `src/store.rs` (5493), `src/store/telemetry.rs` | SQLite `orx.db` |
| Skills / prompts | `SYSTEM_PROMPT.md`, `SKILL.md`, `agent-skills/*/SKILL.md`, `src/local/agent_skills.rs`, `src/local/harness/mod.rs:712-778` (install shims) | Doctrine |
| Literature | `src/commands/{discover,paper}.rs` | alphaXiv / OpenAlex / bioRxiv / PubMed primitives |
| Extras | `src/local/{latex,overleaf,overleaf_live,latex_templates}.rs`, `demo.rs` | Paper writing, Overleaf sync, onboarding demo (nanochat) |

`AGENTS.md:1-9` states the split: `orx` owns local CLI, dashboard, SQLite, agent integrations, experiment orchestration and backends. The companion service `openresearch.sh` owns accounts, orgs, sandboxes and the managed-compute catalog. Projects, runs, logs and artifacts stay local.

---

## 2. Agent loop: orx drives external agents, it has no loop of its own

### 2.1 The Harness trait
`src/local/harness/mod.rs:1-18` (module doc) and trait at `:255-431`:
- `detect()` / `detect_snapshot()`: is the CLI installed and authenticated, which models.
- `run_turn(&mut TurnCtx) -> TurnResult`: "spawn the CLI, parse its event stream, push wire parts onto ctx" (`:292-301`).
- `supports_steering()`, `compact()`, `options()` (permission modes, reasoning levels), `one_shot()` (a throwaway headless child, used for titles), `resume_from_prompt()` (how an answered question or permission card flows back: `ResumeAction::SendMessage` for Claude, which ends the turn and resumes with `--resume`, or `Handled` for OpenCode, which replies inline, `:215-252`).
- Skill install: `skill_target()`, `skill_shim()`, `session_skills_dir()` (`.claude/skills`, `.agents/skills`, `.opencode/skills`, `.cursor/skills`).
- Registry: `registry()` at `:512-520` lists ClaudeCode, Codex, OpenCode, Cursor, Antigravity.
- Watchdog: a turn with no events for 30 minutes is treated as wedged and interrupted (`TURN_WATCHDOG`, `:58`). Retry budget is 3 retries within 15 s (`:60-62`).

### 2.2 How each harness is driven
- **Claude Code** (`src/local/harness/claude.rs:1-20`, spawn at `src/local/claude.rs:500-590`): one *resident* child per chat session:
  ```
  claude --print --input-format stream-json --output-format stream-json
         --include-partial-messages --replay-user-messages --verbose
         --permission-mode <auto|bypassPermissions|plan|...>
         --append-system-prompt-file <worktree>/.openresearch/agent/autoresearch-local.md
         [--model M] [--effort E] [--resume <native_session_id>]
         --settings <file> [--mcp-config <file> --permission-prompt-tool mcp__orx__approve]
  ```
  cwd is the session worktree. The child stays alive across turns with stdin held open. Each turn writes one stream-json user message. The `--replay-user-messages` echo marks where the current turn starts, and a `result` event marks where it ends. A config change, interrupt or crash respawns the child with `--resume`. Permission prompts are bridged through an MCP stdio server (`orx mcp-gate`) that relays each request to `orx up`, which shows an approval card (`MCP_TOOL_TIMEOUT=3600000`).
  The 9ed74d7 commit ("Claude Code provider auth detection") rewrote detection so that `claude auth status --json` is the readiness source of truth (`harness/claude.rs:18-20`). The `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` variables are only fallbacks. If auth fails mid-turn, it recovers once and retries (`harness/claude.rs:2335-2400`).
- **Codex** (`harness/codex.rs:1-26`): one long-lived `codex app-server` (JSON-RPC) per session. `thread/start` or `thread/resume` creates the session thread and each turn is a `turn/start`. The playbook goes in through `developerInstructions`, and the sandbox policy is sent with every turn. Older Codex versions fall back to `codex exec --json`.
- **OpenCode** (`harness/opencode.rs:1-22`): an `opencode serve` child. Each turn is a POST plus a subscription to the SSE `/event` stream, and approvals are answered inline. The playbook is added through the config `instructions` list (`src/local/opencode.rs:339-381`).
- **Cursor / Antigravity**: also implement `run_turn`. For Cursor, the first turn carries a pointer to the playbook file (`opencode.rs:305-306`).

### 2.3 Environment the agent inherits
`prepare_env` (`src/local/chat/mod.rs:8485-8501`) puts this `orx` first on PATH and exports:
- `ORX_CHAT_SESSION_ID`: stamped onto every run the agent launches (`chat/mod.rs:8507`, used at `compute.rs:809` and `experiments.rs:232`).
- `ORX_LOCAL_SESSION`, `ORX_UP_PORT`, `ORX_UP_AUTH_TOKEN`: when the agent runs `orx exp run`, the launch is routed to the owning `orx up` process through `submit_run_via_up` (`local_plane.rs:257-272`).
A shell `PATH_GUARD` keeps the right `orx` in front even after the user's profile modifies PATH (`chat/mod.rs:8543`).

### 2.4 Per-turn context assembly
`with_turn_context` (`chat/mod.rs:1268-1304`) wraps the user text:
- `bootstrap_context`: included only while there is no native session yet.
- `<orx-goal>Keep working toward this goal until it is met, across every turn: ...</orx-goal>`: **included on every turn**. It survives compaction and a lost native session (`:1282-1289`). This is the "autoresearch" mode (`chat_sessions.goal`, set via the API at `up.rs:6963-7000`).
- Demo evidence and shell context, then `<current-user-message>...</current-user-message>`.
Selected chat excerpts are passed as quoted JSON marked untrusted (`:1306-1330`).

### 2.5 One research "turn" end to end
1. The user (or a wakeup) sends a message. `ChatHost::send_message_showing` (`chat/mod.rs:5028+`) validates the permission mode and service tier, claims the session's turn slot atomically, writes a `chat_turns` row (idempotent on `client_turn_id` plus `request_hash`), and calls `harness.run_turn(&mut ctx)` (`:5682`).
2. The harness `run_turn` (e.g. `harness/claude.rs:2284`) calls `ensure_playbook` (`src/local/opencode.rs:317-346`), which:
   - creates or restores the **session worktree** `<data>/worktrees/<projectId>/<sessionId>` (`git.rs:1258-1276`, `:71-77`);
   - renders `SYSTEM_PROMPT.md` into `.openresearch/agent/autoresearch-local.md` (`PLAYBOOK_REL`, `opencode.rs:36`), filling in project facts, the compute default, the artifacts dir, a live **project state snapshot** (#experiments, #runs, #active; `opencode.rs:136-197`) and the skill list;
   - rewrites every `agent-skills/*` module into the worktree's native skills dir on every turn, so the skills never drift from the binary (`agent_skills.rs:299-330`);
   - adds the injected files to `.git/info/exclude` (`opencode.rs:263-306`).
3. The harness streams events and folds them into wire parts (text, tool, prompt) that are persisted in `chat_messages.parts_json` and pushed to the UI.
4. Inside the turn the agent follows the doctrine using shell commands: `orx project view`, `orx create-experiment ...` (creates a branch), `git checkout orx/<slug>`, edit, `git commit`, `orx exp run <id>`, and then either:
   - **wait inline**: `orx exp wait --project <pid>` blocks until the first completion (`local_plane.rs:379-435`), then `orx runs` to reconcile, `orx logs <runId>`, read the log file, and decide; or
   - **go idle**: `orx exp wake <expId>` registers a `chat_run_wakeups` row (`exp.rs:92-123`), and the turn ends.
5. `exp run` → `compute::submit` (`compute.rs:741-868`): preflight, then `SourceSnapshot::create` runs `git archive` on the branch HEAD commit, stores it SHA-256 content-addressed in `<data>/source-snapshots/<digest>.tar` (`compute.rs:34-69`), and reserves the run under a per-experiment file lock that refuses a second in-flight run unless `--force` (`:870-907`). Then `backend.submit`, and a detached `orx supervise <runId>` (`exp.rs:179-225`) tails logs into `<data>/run-logs/<runId>.log` and polls status every 5 s (`supervise.rs:23`).
6. `orx up` runs `watch_runs` every 3 s (`chat/mod.rs:8445-8474`). When a run the session subscribed to reaches `done` or `failed`, it injects a **hidden user message** into that session (`process_run_wakeups`, `:7978-8043`):
   > `[orx] Run `<id>` finished with status **<status>**. You can compare this result with other project runs using `orx runs <pid>` and inspect the file located by `orx logs <id>`.` (`:7940-7948`)
   Busy sessions keep the wakeup pending. Delivery is claim-token based, and the watcher only attempts the earliest wakeup per session on each tick. If Slurm monitoring is lost, the session gets a one-time "cannot monitor" alert (`:7952-7968`, `:8045+`).
   **This is the whole autonomy mechanism: run completion triggers a hidden message, which starts a new agent turn, which runs the doctrine's decision step.** Together with `<orx-goal>` on every turn, the agent keeps going across many turns without a human.

---

## 3. The experiment tree

### 3.1 Representation
- **Node = a `local_experiments` row plus a git branch `orx/<slug>`** (`experiments.rs:173-237`). A child's branch forks from the parent's branch tip with `git branch --no-track orx/<slug> <parent-branch>` (`git.rs:1842-1852`). A root forks from the project base branch (`main`), which is **never** a node itself. The base branch stays mutable for READMEs and notebooks (`experiments.rs:173-177`). Legacy roots that sit on `main` trigger a warning (`:51-65`).
- Slugs are unique across DB rows and existing `orx/*` branches (`base`, `base-2`, ...) (`experiments.rs:15-37`).
- **Run command inheritance**: explicit value, else the parent's command, else the project default, else empty (`experiments.rs:208-217`). The command is **copied** onto the node when it is created. At launch, `compute.rs:763-771` uses the experiment's command first and falls back to the project command. Consequence: `orx project edit --run-command` does not change nodes created earlier. Also note the gap between code and doctrine: `create-experiment --run-command` exists (`invocation.rs:36-42`) even though the doctrine forbids per-node commands.
- **Parent default**: a create without a parent attaches to the oldest non-archived root. `--baseline` creates another root (`local_plane.rs:448-464`, `experiments.rs:43-49`).
- **Worktrees**: one hub clone of the repo, plus one git worktree per **chat session** at `<data>/worktrees/<projectId>/<sessionId>`. It starts detached on the baseline so it does not claim any branch (`git.rs:1248-1276`). Git itself prevents two worktrees from checking out the same branch, and the doctrine turns that into "one branch has one worktree owner" (`agent-skills/orx-git/SKILL.md:16-19`).
- "**Restore vanished chat worktrees at their last checkout**" (commit f336b12): if a registered worktree directory has disappeared, `ensure_worktree_from` reads `git worktree list --porcelain` for that path and re-adds it at its last branch or commit instead of the baseline (`git.rs:1307-1388`).
- Runs never execute in the worktree. They extract the snapshot into `<data>/local-runs/<runId>/repo` (`jobs/localbox.rs:20-29`; `compute.rs:225-243`: `tar -xf archive -C repo; cd repo; <command>`). **Uncommitted files are never part of a run.**
- Archiving (hide, not delete) can target ancestors, descendants, the node only, a region, or a task region (`experiments.rs:67-171`). This is the only "pruning" the code has.

### 3.2 Deciding what to try next, comparing, picking a winner
**All of this is LLM work. There is no Rust code for it.** A grep for metric parsing, winner or best-run logic finds nothing. `runs.result_markdown` holds only failure text such as "Job failed: ..." or "Compute submission failed: ..." (`supervise.rs:70,149,...`, `compute.rs:863`).
- **Comparison happens by reading logs.** `orx-evidence` requires the run command to print final metrics, a compact summary and the effective configuration to stdout. The agent then reads `<data>/run-logs/<runId>.log` with its own file tools (`agent-skills/orx-evidence/SKILL.md:6-47`). "Every node runs the *same* command over *different code*, so their logged result summaries stay comparable" (`SKILL.md:36-39`).
- **Diffs**: `git diff <parent-branch>...orx/<child>` (`orx-git/SKILL.md:37-43`).
- **Decision after each completion**, one of four moves: **repair / refill / promote / stop** (`orx-experiment-tree/SKILL.md:174-192`).
- **Promotion is implicit.** No field marks a winner. "Promote" just means the next round's `create-experiment --parent` points at the winner. The winning node's row and branch are unchanged.
- **Stop rule**: stop when the goal is met or after about 3 consecutive failed or regressed runs, then write a report artifact (`:194-196`).
- `orx exp desc` holds free-form notes per node, which are how sibling sessions coordinate (`:203-222`, `orx-git/SKILL.md:11-15`).

### 3.3 Parallelism
- **Within a session**: several nodes can run at once on remote backends. `exp wait --project` is an **edge trigger** that returns on the first run to become terminal, compared against a snapshot taken at call time (`local_plane.rs:379-435`). It prints `drained: no runs in flight` when nothing is in flight.
- **Across sessions**: `orx agent spawn` creates a new top-level session with its own worktree. The caps are `MAX_LIVE_SPAWNS = 5`, and spawned agents cannot spawn (`src/commands/agent.rs:25-90`). By default the parent is woken with the helper's closing reply (`chat_spawns` table, `deliver_wake_up` `chat/mod.rs:8400-8431`).

---

## 4. Persisted state

Data dir: `$ORX_DATA_DIR`, else the settings.json `dataDir`, else `$XDG_DATA_HOME/openresearch`, else `~/.local/share/openresearch` (`store.rs:8-32`). Layout:
```
orx.db                              SQLite (WAL, busy_timeout 5000)          store.rs:438-442
run-logs/<runId>.log                append-only plain text (supervisor writes) store.rs:4-6, :215
run-logs/<runId>.supervisor.log|.lock|.cancel.lock
source-snapshots/<sha256>.tar|.zip  content-addressed git archive of the run commit   compute.rs:34-69
submission-locks/<experimentId>     fd-lock guarding the one-in-flight rule  compute.rs:877
local-runs/<runId>/repo             extracted snapshot, cwd of local runs    jobs/localbox.rs:22
worktrees/<projectId>/<sessionId>   per-chat git worktree                     git.rs:71-77
files/<projectSlug>/                project artifacts dir (reports, figures)  files.rs:46
repos/...                           hub clones                                git.rs:1-4
```
Schema (`store.rs:443-663`, with ALTER migrations at `:666-710`):
- `local_projects(id, name, slug, github_owner, github_repo, github_sync_enabled, baseline_branch, repo_path, run_command, paper_id, workspace_state_json, ...)`
- `local_experiments(id, project_id, parent_experiment_id, slug, branch_name, title, description, run_command, agent_status, chat_session_id, archived, created_at, updated_at)`. This is the whole tree. It is an adjacency list, and `agent_status` is effectively always `"idle"`.
- `runs(id, experiment_id, project_id, status, backend_json, command, commit_sha, result_markdown, cancel_requested, chat_session_id, created_at, updated_at, ended_at, exit_code)`. `status` is one of `starting|running|done|failed|cancelled`. `backend_json` is a serialized `BackendDescriptor` (kind, job id, host, flavor, source digest/path/size, monitoring error, ...).
- Chat: `chat_sessions` (harness, native_session_id, model, permission_mode, plan_mode, reasoning_level, **goal**, bootstrap_context, parent_session_id, active_leaf_id, ...), `chat_messages` (parts_json; a **message tree** via parent_id for forked turns, `:720-757`), `chat_turns` (state, delivery_state, attempts, error/recovery fields), `chat_queued_messages`, `chat_turn_leases`.
- Orchestration: `chat_run_wakeups(run_id, chat_session_id, state, claim_token, delivered_at, monitoring_alerted)` and `chat_spawns(session_id, parent_session_id, prompt, wake_parent, state, claim_token, attempts)`.
- Telemetry (v0.2.14): `run_telemetry`, `native_invocation_identities`, `chat_usage_*`, `native_usage_*`, `telemetry_pending_events`.
- Misc: `ssh_host_tests`, `overleaf_links`, `ui_state`, `data_dir_move_lease`.

**There is no metric table and no "result" entity.** Results live only in log files and in the prose the LLM writes into `exp desc` and artifacts.

---

## 5. The doctrine (verbatim-ish), the real IP

### 5.1 Cardinal rules (`SKILL.md:17-47`)
1. **Never edit a node once a run has answered it.** "A node freezes the moment a run establishes its baseline or tests its hypothesis - that includes the root - and freezing is permanent: a disappointing result is still a result." Until then it is **provisional**. To test a new hypothesis, branch a **child**.
2. **The run command *and* the environment are a fixed contract, identical on every node.** "Do **not** give nodes different start commands, and do **not** vary behavior through environment variables or env-prefixed commands (`LR=3e-4 python ...`). The *only* thing that may differ between nodes is the **committed code/config** on the node's git branch."
3. **Vary code, not knobs-in-the-command.** "Encode hyperparameters in the code/config files and branch a child per variant ... Every node runs the *same* command over *different code*, so their logged result summaries stay comparable."
4. **Grow the tree downward, not sideways.** "Fan a little *within* a round (the options of one decision), then **descend onto that round's winner** for the next round. A root with a long row of direct children and no grandchildren is the failure mode."
> "If you're ever tempted to change the command, pass an env var, or pile another node onto the root instead of branching a child, editing its branch, and descending - stop."

### 5.2 Node lifecycle (`orx-experiment-tree/SKILL.md:14-47`)
- Create a node only when a planned run will **establish a baseline or test a hypothesis**. No nodes for cleanup, refactors, bug fixes or dependency bumps.
- **"Provisional until it answers - repair, don't branch."** A run that dies on an error answered nothing, so fix the branch in place and re-run the same node.
- "Unintended behaviour is not an answer. An OOM, a timeout, a divergence from a bug, a missing dep ... the node is still provisional (unless the node's hypothesis *is* about memory or runtime)."
- **Repair cap**: "two runs in a row that answer nothing on one node, then ask the user. Different errors still count; a bare relaunch or a flavor/backend switch is a repair. If the same failure hits a second node, that is one setup problem - ask then." This is separate from the scientific stop rule ("~3 consecutive failed or regressed runs").

### 5.3 Tree shape: stacked bushes (`:49-91`)
The three shapes are FLAT FAN (wrong), NOODLE (wrong) and STACKED BUSHES (right).
- Flat fan: "every result is measured against the *start*, so wins never accumulate."
- Noodle: "depth manufactured for its own sake."
- **The one rule**: "Before you make X a child of Y, name what Y established that X builds on." If you can name it, X is a child. If X and Y are co-equal options, they are siblings.
- "**width = the open options of one decision**; **depth = decisions already resolved, stacked** ... A new *round* never hangs off the root - it hangs off the previous round's winner."
- "Re-read the tree each round" with `orx project view` and check its shape.

### 5.4 The auto-research loop (`:93-201`)
1. Read the baseline code on its branch, and find the run command and where the knobs live.
2. Form **one round's** hypotheses: "the co-equal options of a *single* decision (which LR? which schedule? which init?)". Don't mix decisions from different rounds.
3. Create the round as a bush under the baseline (round 1 only) or under the **previous round's confirmed winner**. The title is the idea. The description is the concrete change ("Set the LR in config.yaml to 2e-5; change nothing else.").
4. Implement on the child's branch and commit. Leave the run command alone. Before launching, make sure the code emits enough evidence.
5. Launch with `orx exp run <childId>`. Omit `--backend` when a default target is set.
6. "**Drive a per-completion loop, not a wait-for-all barrier.**" `exp wait --project` is "one **tick** of a loop, where *you* are the loop body". Three robustness rules:
   - "wait is a sleep-until-change signal, not the source of truth." After every wake, re-read `orx runs` and reconcile *every* newly terminal run.
   - Re-issue the wait on every tick.
   - Terminate on `drained`.
7. Analyze each finished run as it lands: read the log (not the status) and diff against the parent. Then choose one of **Repair / Refill / Promote / Stop**. "Frozen nodes stay untouched throughout - promotion moves the *focal parent* down the tree."
- Close any turn that changed experiments with "one line per relevant node with what it tested, its status, and the headline result."

### 5.5 Launch contract (`orx-compute/SKILL.md`)
- "**Launch all experiment compute with `orx exp run`.** Never invoke provider CLIs, schedulers, raw SSH, or the training command directly ... direct jobs are untracked and may run code other than the recorded commit."
- "Commit before launching ... Uncommitted files are excluded." Check `git status --short` and `git show --stat HEAD` first (`orx-git/SKILL.md:31-35`).
- "Use another backend only when the user names one; a connected credential is not a signal to switch."
- Sizing: "Decide GPU versus CPU first ... smallest shape that fits ... Escalate after a real OOM."
- Wait or wake, never both. "Timeout ... means nothing changed yet, not that the run failed." "A failed run is not a new node."

### 5.6 Git discipline (`orx-git/SKILL.md`)
- "Once a run answers an experiment, its branch and history are immutable. Never merge or rebase it. To incorporate other work, create a child and put the merge commit on the child's branch."
- Before starting, check `git branch -a`, `orx runs` and sibling `exp desc` notes "so you do not duplicate their work". Keep notes current.

### 5.7 Evidence contract (`SYSTEM_PROMPT.md:72-113`, `orx-evidence`)
- Every substantive claim is followed by a clickable citation: `<file path=... lines=... exp=.../>` for code or `<run id=... label="+3.65pp"/>` for results. "Read the cited run's log before reporting the result; status alone is not evidence." Label inferences as inferences.
- "Truncated output is not evidence of absence."
- "Never infer a result from run status or memory."

### 5.8 Other doctrine
- Python env policy (`SYSTEM_PROMPT.md:45-70`): check for uv, never share `.venv` between worktrees, "Establish baseline dependencies before branching experiments. Run recipes must recreate dependencies from committed snapshots."
- Delegation (`orx-agent-delegation`): delegate only work with a clean boundary. "Never give a helper a branch checked out by this session". State exactly which `exp run` calls the helper may make, and explicitly forbid launches if none are allowed. Never delegate literature retrieval.
- Lit review (`orx-lit-review`): the main agent owns the retrieval loop. It sets a difficulty score from 1 to 10 that fixes the follow-up budget (0/1/2 rounds), and the date window and priority must not change mid-loop. Build answers around the papers' original figures.
- "`orx` is internal and should stay under the hood; do not mention it in user-facing responses" (`SYSTEM_PROMPT.md:43`).

---

## 6. Novel and valuable vs. generic packaging

**Genuinely valuable / novel**
1. **Research methodology encoded as agent doctrine.** Fixed run contract, freeze-on-answer, "repair, don't branch", stacked bushes with "name what Y established", repair/refill/promote/stop, and two separate stop rules (a setup failure cap and a scientific regression cap). It targets the known failure modes of LLM research agents: flat sweeps, config drift through env vars, rewriting history after a bad result, and claiming results from status. It is short and transferable to any harness.
2. **Git as the experiment database, with provenance enforced by construction.** A node is a branch, a run is a SHA-256 content-addressed `git archive` of the exact commit, and the run executes in its own extracted dir, never in the worktree. Reproducibility doesn't depend on the agent behaving.
3. **Durable wakeup plus goal injected on every turn gives autonomy without a custom loop.** Run completion becomes a hidden `[orx]` user message. The goal rides every turn and survives compaction. Claim tokens, leases and a queue-behind-user-messages rule make it crash-safe. This is a clean way to make a stock coding CLI into a long-horizon agent.
4. **`exp wait --project` as an edge-triggered tick**, with the explicit teaching "wait is a signal, `orx runs` is the truth; reconcile every terminal run". This is the right concurrency primitive for an LLM that is itself the loop body.
5. **Per-session git worktrees on one hub clone**, restored at their last checkout if they vanish. Git's own "branch already checked out" lock doubles as the ownership rule between parallel agents.
6. **One Harness trait over five CLIs**, each driven through its native embedding channel: Claude as a resident stream-json child with `--resume`, Codex app-server JSON-RPC, OpenCode serve plus SSE. The playbook goes through each CLI's real system-instruction channel. The plan-mode gate uses an allowlist of read-only `orx` and git verbs (`harness/plan_gate.rs:1-28`), and an MCP permission bridge handles headless approvals.
7. **Skills re-materialized into the worktree on every turn** from the binary, so the prompt never drifts from the installed CLI. The global shims only say "run `orx skill`".

**Generic / packaging**
- The dashboard, React UI, i18n, Overleaf/LaTeX, the updater, telemetry, onboarding demo, harness detection, and the nine compute backend adapters are big and useful but ordinary engineering. The Rust is heavily defensive: most of chat/mod.rs handles recovery, leases and idempotency.
- **Weak spots**: no structured metrics or winner record. The "frozen" rule and "same command" rule are doctrine only; the code does not refuse commits to an answered node, and `create-experiment --run-command` exists. Run commands are copied at node creation, so a later project command edit doesn't reach existing nodes. Everything about comparison depends on the LLM correctly reading unstructured logs.

**Takeaway for reuse**: the portable core is roughly 300 lines of doctrine (`SKILL.md` cardinal rules plus `orx-experiment-tree` plus `orx-evidence` plus `orx-git`) and four primitives: branch-per-node, commit-snapshot runs, edge-triggered wait, and durable completion wakeup into the agent session. A structured `metrics.json` convention and code-enforced freezing would be the obvious upgrades.
