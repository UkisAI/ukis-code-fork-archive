# G - ext/research gates: what the ukis-code extension API can and cannot enforce

Repo: `/home/pavle/projekti/ai-tooling/ukis-code` (HEAD 77a4296720), Rust workspace `codex-rs/`.
All paths below are relative to `codex-rs/` unless absolute. Read-only study, nothing built or modified.

Note: the prior reports (`D-ukis-code-arch.md`, OpenResearch A/B/C) did NOT exist in the scratchpad
when this study ran (the `study/` dir was absent). This report is self-contained; experiment-tree
rules are taken from the task brief (model proposes, code decides; numbers only from recorded runs,
5 seeded repeats) and the user's `experiment` skill conventions (preregistered contract, controls, MDE).

---

## 0. TL;DR

- The extension API is rich for **adding tools, pinning context per sampling step, observing
  everything, and starting/steering turns**. It is **observe-only for tool calls** and has **no
  in-process Stop veto**.
- A same-turn "you may not finish with that claim" gate IS possible today without core changes:
  `TurnLifecycleContributor::on_item_completed` runs inline on the final AgentMessage, and
  injecting a correction via `CodexThread::inject_if_running` makes `has_pending_input` true,
  so the turn loop continues instead of ending. Cleaner with a 50-line upstream hook (sec. 6).
- The only built-in pre-execution veto is the **config/plugin hooks engine** (PreToolUse / Stop,
  Claude-Code-compatible JSON) and **execpolicy `.rules` (`forbidden`)**. Extensions cannot
  register hooks programmatically.
- The strongest anti-sycophancy mechanism is structural: **`research_report` is the only way a
  number becomes "a result"**; its validator recomputes every number from the run ledger. The
  final-message gate then only has to check "every number / success word in prose is backed by a
  validated report".
- `update_plan` is stateless and non-durable (not persisted to rollout, lost on compaction) - do
  not build on it; mirror it and replace it with extension-owned `task_*` tools + a world-state
  pinned block.

---

## 1. Contributor traits and hooks (file:line)

All traits: `ext/extension-api/src/contributors.rs` unless noted. Registration:
`ext/extension-api/src/registry.rs:49-151` (`ExtensionRegistryBuilder::*_contributor`).
Production install site: `app-server/src/extensions.rs:50-120` (`thread_extensions()`); TUI and
`exec` both run through the in-process app-server (`exec/src/lib.rs:21-23,986`), so installing
there covers TUI + exec. `cli/src/main.rs:2037-2062` only builds a debug prompt registry.

| Trait / capability | Methods | What it can do |
|---|---|---|
| `ToolContributor` (357) | `tools()` 359, `tools_for_step()` 366 | Add native tools. Executors are `codex_tools::ToolExecutor` (`tools/src/tool_executor.rs:106-128`: `tool_name`, `spec`, `exposure`, `handle`). Per-step variant can hide/show tools dynamically. Tool gets `ToolCall` (`tools/src/tool_call.rs:132-144`: turn_id, call_id, payload, read-only `conversation_history`, `turn_item_emitter`, `environments` with **file_system only**, no process API `tool_call.rs:90-101`). Tool errors: `FunctionCallError::RespondToModel` goes back to the model. |
| `ContextContributor` (109) | `contribute_thread_context` 111, `contribute_turn_context` 124, `contribute_world_state` 135, `retain_world_state_after_compaction` 151 | Inject developer text. Thread context = stable DeveloperPolicy / DeveloperCapabilities / ContextWindow slots (`contributors/prompt.rs:8-13`), re-injected with initial context after compaction. **World state** = per sampling step section with own snapshot + diff renderer (`contributors/world_state.rs:86-158`), core calls it every step (`core/src/session/world_state.rs:295-318`). After compaction only retained metadata survives (`core/src/session/mod.rs:4064-4088`), so a section that retains nothing is re-rendered in full. This is the "pinned block" mechanism. |
| `TurnInputContributor` (282) | `contribute` 285 | Add contextual user fragments once per submitted user turn (`core/src/session/turn.rs:1184`). |
| `ThreadLifecycleContributor` (164) | `on_thread_start` 166 (gets Config, session source, `persistent_thread_state_available`), `on_thread_ready` 174, `on_thread_resume` 182, `on_thread_idle` 194, `on_thread_stop` 203 | Seed/rehydrate/flush state. `on_thread_idle` fires after a turn completes/interrupts/fails (`core/src/tasks/lifecycle.rs:65-83`) and is where goal starts continuation turns. `on_thread_stop` must cancel+join background work (doc 200-202). |
| `TurnLifecycleContributor` (216) | `turn_start_phase` 219, `on_turn_start` 232, `on_item_completed` 240, `on_turn_stop` 251, `on_turn_abort` 260, `on_turn_error` 268 | Observe turn + every completed TurnItem (incl. final AgentMessage with `phase`). `on_item_completed` is awaited inline (`core/src/session/mod.rs:2529-2543`) before the turn loop decides whether to end. `on_turn_stop` is observe-only, runs before turn-complete event. |
| `TurnItemContributor` (437) | `contribute(&mut TurnItem)` 438 | Mutate a parsed item before emission to clients (`core/src/stream_events_utils.rs:237-250,487-491`). Does NOT change model history: history records the original ResponseItem (`stream_events_utils.rs:389-394`). Good for a visible "UNVERIFIED" banner. |
| `ToolLifecycleContributor` (381) | `on_tool_dispatch` 384, `on_tool_start` 390, `on_command_start` 398, `on_mcp_tool_result` 403 (can mutate MCP result), `on_tool_finish` 411, `on_tool_timing` 417 | **Observe-only** for native/shell tools (doc 377-380: "without rewriting the invocation... use hooks for policy"; `on_tool_dispatch` "cannot affect tool execution" `contributors/tool_lifecycle.rs:246-247`). `ToolStartInput.payload` = finalized args (`tool_lifecycle.rs:118-148`); `CommandStartInput.command/cwd` = resolved argv, only for unified_exec (`core/src/tools/handlers/unified_exec/exec_command.rs:407`), includes code-mode commands. `ToolFinishInput.outcome` (198-215). Only exception: MCP results are `&mut`. |
| `ApprovalReviewContributor` (422) | `decide` 424 -> `Allow / Reviewed(ReviewDecision) / AskUser` (`contributors/approval_review.rs:14-19`) | Can DENY, but only for actions that already enter the approval flow (`core/src/tools/approvals.rs:613-676`, `core/src/guardian/decision.rs:45-120`). Under full-access / never-ask it is not consulted for ordinary exec. Not a general veto. |
| `ModelRequestContributor` (`model_request.rs:33`) + `ModelResponseInterceptor` (40) | `request`, `intercept(stream)` | Wrap the raw provider response stream (buffer/inspect/transform `ResponseEvent`s). Input has only thread_id/model/metadata (19-29), no extension stores. Could rewrite a final message before core sees it; heavy and brittle, not recommended. |
| `TokenUsageContributor` (316) | `on_token_usage` 318 | Per-response token accounting (goal budgets). |
| `ConfigContributor` (299) | `on_config_changed` 301 | Before/after config snapshot. |
| `SkillInvocationContributor` (336) | `on_skill_invocation` 345 | Observe explicit/implicit skill use. |
| `McpServerContributor` (83) | `contribute`, `selected_plugins` | Add/replace/remove MCP servers at runtime (`contributors/mcp.rs:166-197`). |
| Capabilities | `ExtensionEventSink::emit / emit_warning` (`capabilities/events.rs:19-28`), `ResponseItemInjector` (`capabilities/response_items.rs:15-33`), `ExtensionMetrics`, `ConversationHistorySnapshot` | Emit protocol events (goal emits `ThreadGoalUpdated`) and user-visible warnings. |
| State | `ExtensionData` (`state.rs:345-435`: typed `get/get_or_init/insert/insert_if/remove`), scopes session/thread/turn/step; `ExtensionDataInit` for host-seeded values (`state.rs:314-343`) | **In-memory only**; no persistence helper. Durable state = own storage (goal uses `codex_state` sqlite). |
| Host policy knobs (not extension-owned) | `ToolPolicy` allow-list (`tool_policy.rs:10-44`), `SessionIsolation` (`session_isolation.rs`), `TurnStartAdmission` (`turn_admission.rs`) | Supplied by host via `ExtensionDataInit`; an extension cannot tighten them after start. |
| Core handles (extensions may depend on `codex-core`, goal does: `ext/goal/Cargo.toml`) | `CodexThread::inject_if_running` (`core/src/codex_thread.rs:557`), `start_turn_if_idle` (390), `continue_turn_if_idle` (413), `thread_extension_data` (284), `active_turn_root` (597) | Same-turn steering and autonomous follow-up turns. Needs `Weak<ThreadManager>` passed at install (goal `ext/goal/src/extension.rs:615-641`). |

Config access: extensions get the host `Config` in `on_thread_start` / `on_config_changed` (pattern:
`ext/web-search/src/extension.rs:42-118`, `ext/memories/src/extension.rs:45-55`). A custom `[research]`
table can be read without touching core via `config.config_layer_stack.effective_config()`
(`config/src/state.rs:530`); `ConfigToml` only has schemars `deny_unknown_fields`
(`config/src/config_toml.rs:164-166`), serde tolerance of an unknown top-level table is
**unverified** (may warn). A typed `ResearchConfig` field on `Config` is the clean option.

---

## 2. Can an extension veto "done"?

How a turn ends today (`core/src/session/turn.rs`): after each sampling response,
`needs_follow_up = model_needs_follow_up || has_pending_input` (566). If false (653), config Stop
hooks run (`run_turn_stop_hooks`, `core/src/hook_runtime.rs:448-470`); a `block` with a prompt
records the continuation and `continue`s the loop, guarded by `stop_hook_active` (672-697).

Options, best first:

1. **`research_report` is the only channel for results (structural).** Numbers the user should
   trust exist only as tool output rendered by code from the ledger. Works today, zero core change.
   Necessary but not sufficient: the model can still type prose numbers.
2. **In-process gate via `on_item_completed` + `inject_if_running` (works today, verify with a
   test).** On a completed `TurnItem::AgentMessage` whose `phase != Commentary`
   (`protocol/src/models.rs:944-952`; phase is optional, None must be treated as final), run the
   claim checker. On violation call `thread.inject_if_running(vec![correction])` (goal does the
   same from `on_tool_finish`, `ext/goal/src/runtime.rs:525-537`). Injection lands in the pending
   input queue (`core/src/session/inject.rs:17-38`), so `has_pending_input` becomes true and the
   turn continues with the correction visible to the model. Loop guard in `turn_store`
   (max 2 corrections per turn), then fall back to a visible banner (TurnItemContributor) +
   `emit_warning`. Caveats: the offending message was already streamed/emitted to the user; if the
   same response also contained tool calls the correction lands one step later (harmless);
   lock-ordering of `inject_if_running` from inside item emission is **unverified** (goal only
   calls it from tool-finish), needs a test before relying on it.
3. **Goal-style idle continuation (works today).** If the turn already ended, `on_thread_idle`
   (cause `Completed`) -> `thread.start_turn_if_idle(TurnInputRequest::new(TurnInput::ResponseItem(item)))`
   exactly like `ext/goal/src/runtime.rs:425-523`. Costs an extra turn; good as fallback and for
   "run finished, wake up" events.
4. **Config Stop hook (works today, not "built-in").** A command hook
   (`config/src/hook_config.rs:163-201`, types command/mcp_tool/prompt/agent) such as
   `ukis-code research gate-stop` reads `last_assistant_message`, returns
   `{"decision":"block","reason":...}`. Exact semantics, but requires Feature::CodexHooks, hook
   trust, and config/plugin/project install (`core/src/session/mod.rs:5100-5131`; sources =
   config layers + plugin bundles + executor hooks). Extensions cannot add hooks in code.
5. Response-stream interception (`ModelResponseInterceptor`) - possible, not recommended.

**Recommendation:** 1 + 2 now (with 3 as fallback), and upstream the tiny `on_turn_stop_request`
hook (sec. 6) so the gate runs exactly where Stop hooks run, after the final message, with the
existing `stop_hook_active` guard.

---

## 3. Can an extension block specific tool calls?

Not in-process today. `ToolLifecycleContributor` cannot veto (sec. 1). The dispatch path runs
`run_pre_tool_use_hooks` then `notify_tool_start` (`core/src/tools/registry.rs:602-654`), and the
only veto there is the hooks engine (`core/src/hook_runtime.rs:188-243`, blocked -> 
`FunctionCallError::RespondToModel("Command blocked by PreToolUse hook: ...")`).

Deterministic levers available now:

| Rule | Mechanism today | Notes |
|---|---|---|
| deny `git commit --amend`, `git rebase`, `git reset --hard`, `git push -f` in experiment worktrees | execpolicy `prefix_rule(..., decision="forbidden")` in `<layer>/rules/*.rules` (`core/src/exec_policy.rs:662-700`, `Decision::Forbidden` 395) | Parses `bash -lc` inner plain commands (`exec_policy.rs:39,872-880`). Project layer = `.codex/rules/` (`config/src/state.rs:219-231`). Prefix match, not cwd-aware. |
| run the benchmark only through `exp_run` | execpolicy forbid `ukis-bench run` prefix for the model; `exp_run` spawns it from Rust (not through the model's exec tool, so the rule doesn't apply) | Plus detection: `on_command_start` sees argv (+cwd) and taints the experiment if seen. |
| no edits inside a frozen experiment worktree | (a) freeze = extension commits, records tree hash, `chmod -R a-w`; (b) keep frozen worktrees outside sandbox writable roots; (c) PreToolUse hook on `apply_patch` paths | Detection is the real guarantee: `exp_run` refuses if `git status --porcelain` is dirty or `HEAD != frozen_sha`; `research_report` refuses runs whose recorded tree hash doesn't match. `on_tool_start` sees apply_patch payload paths -> immediate steering warning. |
| amend after the fact | ledger stores commit SHA + tree hash per run; report validator checks the SHA still exists and tree hash matches | Amend cannot silently change what a run measured. |

Upstream (sec. 6): a `pre_tool_use` veto on `ToolLifecycleContributor`.

---

## 4. ext/goal as the template

- **Storage:** separate sqlite goals DB in `codex_state` (`state/src/runtime.rs:96,133-163`;
  schema `state/goals_migrations/0001_thread_goals.sql`, `0002_thread_goal_continuation_deferrals.sql`;
  API `state/src/runtime/goals.rs`). Runtime handle cached in `thread_store`
  (`ext/goal/src/extension.rs:171-189`), re-registered in a process-wide `GoalService`.
  Resume: `restore_after_resume` re-reads DB (`runtime.rs:401-423`).
- **Compaction:** goal is NOT in context as a pinned block (goal has no `ContextContributor`). It
  survives because the source of truth is the DB and it is re-injected as a fresh internal
  fragment on every idle continuation (`runtime.rs:469-486`, template
  `ext/goal/templates/goals/continuation.md`), plus steering on budget limit
  (`extension.rs:541-553`, `templates/goals/budget_limit.md`) and objective edits
  (`runtime.rs:237-240`). Fragments are `InternalModelContextFragment` with source "goal"
  (`ext/goal/src/steering.rs:60-65`; `core/src/context/internal_model_context.rs:17-64`).
- **Budget:** `TokenUsageContributor` records totals per turn (`extension.rs:452-478`); turn
  start snapshots baseline (256-310); tool finish and turn stop account deltas into the DB
  (`runtime.rs:539-600`); DB flips status to `BudgetLimited` and a steering item is injected once.
  Empty-response / repeated failures -> `Blocked` (`runtime.rs:277-399`).
- **Continuation:** `on_thread_idle` -> `continue_if_idle` -> `thread.start_turn_if_idle`
  (`runtime.rs:425-523`) with a semaphore so user mutations can't race it.
- **Model contract:** `update_goal` tool description forces a completion audit
  (`ext/goal/src/spec.rs:60-94`); goal is honor-system for "complete" (model decides). This is
  exactly the weakness ext/research must remove: research completion must be computed.
- **TUI:** bespoke path. `EventMsg::ThreadGoalUpdated` via event sink (`ext/goal/src/events.rs`)
  -> app-server `ThreadGoalUpdatedNotification` (`app-server/src/extensions.rs:7-8`) -> TUI
  `chatwidget/goal_status.rs` -> footer line "Pursuing goal (usage)" (`tui/src/bottom_pane/footer.rs:579-593`),
  menu `chatwidget/goal_menu.rs`, `/goal` actions `app/thread_goal_actions.rs`. `StatusLineItem`
  is a closed enum (`tui/src/bottom_pane/status_line_setup.rs:56+`). Visible tool items are also a
  closed enum `ExtensionItem` (`ext/items/src/lib.rs:31-44`, "adding a variant also requires
  app-server to add its typed public wrapper").

**For experiment-tree state, use world state instead of goal's pattern:** a
`contribute_world_state` section `research_state` whose snapshot is the compact tree digest and
whose `render_diff` returns the full block whenever `previous != current` or `Absent/Unknown`
(pattern: `ext/git-attribution/src/world_state.rs:30-65`). Return nothing from
`retain_world_state_after_compaction` so compaction forces a full re-render. Durable truth lives
in files (sec. 7), rehydrated in `on_thread_start/resume`.

---

## 5. Shipping methodology as built-in

Three layers, all exist today:
1. **Always-on rules** (short, <60 lines): `include_str!` template rendered into a
   `PromptFragment::developer_policy` from `contribute_thread_context` (memories does this,
   `ext/memories/src/extension.rs:57-100`; goal embeds templates with `include_str!` +
   `codex_utils_template`, `ext/goal/src/steering.rs:10-43`).
2. **Long-form skill**: system skills are embedded with `include_dir!`
   (`skills/src/lib.rs:55`, dir `skills/src/assets/samples/{imagegen,openai-docs,review-agent,skill-creator,skill-installer}`)
   and installed to `CODEX_HOME/skills/.system` with a fingerprint marker (`skills/src/lib.rs:77-100`).
   Add `skills/src/assets/samples/research/SKILL.md` (+ references) and it ships with the binary.
3. **Living methodology**: at thread start, load `eval/methodology.md` + the active technique's
   `methodology.md` from the ukis-methods repo (sec. 7b), inject its version id + key rules into
   the world-state block. Improvements flow in without rebuilding.

---

## 6. Gaps and minimal upstream changes

| Gap | Evidence | Minimal change |
|---|---|---|
| No in-process pre-tool veto | `contributors.rs:377-380`; `tool_lifecycle.rs:246-247` | Add `fn pre_tool_use(&self, ToolStartInput-like) -> ExtensionFuture<ToolGate>` (`Allow` / `Block{reason}`) to `ToolLifecycleContributor`; call in `core/src/tools/registry.rs` right after `run_pre_tool_use_hooks` (~603) and in `unified_exec/exec_command.rs:407` (covers code mode). ~60 LOC. |
| No in-process Stop veto | `on_turn_stop` observe-only (`contributors.rs:251`); Stop hooks config-only | Add `fn on_turn_stop_request(&self, {last_agent_message, stop_attempt, stores}) -> ExtensionFuture<StopDecision>` (`Allow` / `Continue{fragment}`); call at `core/src/session/turn.rs:653` next to `run_turn_stop_hooks`, reuse `stop_hook_active` + `build_hook_prompt_message`. ~50 LOC. |
| No generic status line / visible item for extensions | `StatusLineItem` and `ExtensionItem` closed enums | Either add `ExtensionItem::ResearchRun/ResearchReport` (`ext/items`) + app-server wrapper + TUI renderer, or a generic `EventMsg::ExtensionStatus{namespace,text}` + `StatusLineItem::ExtensionStatus`. |
| No process API for extension tools | `ToolEnvironment` has file_system only (`tools/src/tool_call.rs:90-101`) | Not needed for local: `exp_run` spawns a detached host process (runs outside the model sandbox, by design). Remote executors unsupported until an exec capability is exposed. |
| No persistence helper | `ExtensionData` in-memory (`state.rs:345`) | Use repo files (preferred for research) or a `codex_state`-style sqlite. |
| Programmatic hooks / execpolicy rules | `HooksConfig` from config + plugins only (`hooks/src/registry.rs:42-51`) | Optional: let extensions contribute `PluginHookSource`s. Not required if the two veto hooks above land. |
| `update_plan` not durable | sec. 8a | Don't extend; replace with extension tools. |

---

## 7. Proposed skeleton: `ext/research` (crate `codex-research-extension`)

### 7a. Code repo ledger (repo-tracked, committed with the experiment)
```
<repo>/.ukis/research/
  tree.json                  # experiment tree: nodes {id, parent, hypothesis, contract_sha, status,
                             #   worktree, base_sha, frozen_sha, frozen_tree_hash, runs[]}
  tasks.json                 # task slices from paper/plan: {id, title, source_ref, status, how, exp_ids}
  contracts/<exp>.json       # preregistered: metric (real name), direction, seeds[5], cmd template,
                             #   controls, MDE, tolerance; sha256 frozen at first run
  runs/<run_id>/meta.json    # argv, cwd, commit sha, tree hash, dirty=false, seed, host, t0/t1, exit
  runs/<run_id>/results.json # written by the team CLI, parsed/validated by Rust (never by the model)
  runs/<run_id>/stdout.log
  reports/<rep_id>.json      # validated research_report + rendered canonical table
  journal.jsonl              # append-only, one line per tool/turn/task event (machine)
  journal/<task_id>.md       # rendered from journal.jsonl (human)
```
Why repo-tracked (not `~/.codex`): runs, contracts and journal must travel with the commit they
measured, be reviewable in PRs, and survive machine changes. `~/.codex` only holds the per-thread
pointer (active tree node / task) so resume is instant.

### 7b. Methods repo (separate git repo, working name `ukis-methods`)
Config (`~/.codex/config.toml` or project `.codex/config.toml`):
```toml
[research]
enabled = true
methods_repo = "~/projekti/ukis-methods"      # absolute path to the second repo
bench_cmd = ["ukis-bench", "run"]              # team CLI; argv built by Rust from the contract
default_seeds = [0, 1, 2, 3, 4]
max_gate_corrections_per_turn = 2
```
Layout owned by the extension:
```
techniques/<slug>/README.md
techniques/<slug>/paper/{full_text.md, source.json}      # fetched by paper_read, quoted claims point here
techniques/<slug>/claims.json                             # validated quotes -> metric/value/conditions
techniques/<slug>/reproductions/<date>-<slug>/{contract.json, results.json, comparison.json,
                                               journal.md, experiment.json (code repo url+sha)}
techniques/<slug>/methodology.md                          # versioned (front-matter version + changelog)
eval/methodology.md                                       # evaluation rules themselves
```
Deterministic commits: the extension, not the model, runs fixed argv
`git -C <methods_repo> add -- <explicit paths it wrote>` then
`git -C <methods_repo> commit -m "<templated: research(<slug>): <action> <ids>>"`, records the SHA
in the ledger/journal. Model-issued git inside `methods_repo` is forbidden (execpolicy prefix
`git -C <methods_repo>` + `on_command_start` cwd detection -> taint + steering). Changes to
`eval/methodology.md` are staged as a proposal and only committed after explicit user approval
(e.g. `/research approve <id>` slash command or an approval via `ApprovalReviewContributor` /
`request_user_input`).

### 7c. Tools (all `ToolContributor`, JSON-schema args; model supplies intent, code supplies facts)

| Tool | Args | Deterministic behavior |
|---|---|---|
| `lit_search` | `{query, limit?, since?}` | Calls arXiv/S2 (or the team vault MCP). Returns ids + titles only. |
| `paper_read` | `{paper_id, section?}` | Fetches full text into `methods_repo/techniques/<slug>/paper/` (cached), returns text chunk + char offsets. |
| `technique_open` | `{slug, paper_id, title}` | Creates `techniques/<slug>/` skeleton, commits. |
| `claims_extract` | `{slug, claims:[{id, quote, metric, value, unit, conditions, section}]}` | Rejects any claim whose `quote` is not a verbatim substring of `paper/full_text.md` or whose `value` does not appear in the quote. Writes `claims.json`, commits. |
| `task_plan` | `{source_ref, tasks:[{id, title, acceptance}]}` | Slices paper/plan into `tasks.json`; replaces update_plan as the source of truth. |
| `task_start` | `{task_id, how}` | Exactly one `in_progress`; `how` is required (the "how I'm doing it"). Journals baseline HEAD. |
| `task_done` | `{task_id, evidence:{report_id? , commit_sha?}}` | Refuses unless acceptance evidence exists (validated report or reachable commit). |
| `exp_create` | `{parent?, hypothesis, metric, direction, controls[], mde, cmd_args}` | Writes contract, creates git worktree from HEAD, returns exp_id. Contract hash frozen at first run. |
| `exp_freeze` | `{exp_id}` | Extension commits worktree (templated msg), records sha + tree hash, `chmod -R a-w`. |
| `exp_run` | `{exp_id, seeds?}` (seeds default from contract; fewer than 5 marks the run `pilot`) | Refuses if not frozen, dirty, or HEAD != frozen_sha. Spawns `bench_cmd` detached per seed with argv built from contract; writes `meta.json`; returns run_ids. |
| `exp_wait` | `{run_ids, timeout_s}` | Polls PIDs/result files; parses `results.json` against schema; returns only status + ledger values. |
| `exp_status` | `{exp_id?}` | Tree + runs + aggregates computed in Rust (mean, std, 95% CI, n). |
| `research_report` | `{exp_ids, baseline_exp_id, claims:[{metric, stat:"mean"|"delta"|"ratio", value, ci?}], verdict:"supported"|"refuted"|"inconclusive", narrative}` | **Gate.** Recomputes every value from the ledger; rejects on mismatch beyond contract tolerance, n < seeds, pilot-only runs, contract changed after runs, missing controls, tree-hash mismatch, verdict inconsistent with CI vs MDE. On success stores report and returns the canonical code-rendered table the model must quote. |
| `repro_record` | `{slug, report_id}` | Writes `reproductions/<date>-<slug>/` (contract, results, `comparison.json` computed claim-vs-measured, journal.md rendered from ledger, experiment commit link), commits. |
| `retro_submit` | `{slug?, what_worked[], what_failed[], methodology_patch}` | Appends to technique `methodology.md` with version bump + changelog, commits; patches to `eval/methodology.md` become pending proposals. |

### 7d. Hooks registered
- `ThreadLifecycleContributor`: start (read `[research]`, open ledger, rehydrate), resume (rehydrate),
  idle (wake on finished runs via `start_turn_if_idle`; gate fallback), stop (flush; do not kill
  detached runs - they are external processes by design).
- `ConfigContributor`: refresh config.
- `ContextContributor`: thread context = built-in policy (`include_str!`); **world state
  `research_state`** = current task id + title + "how" + status, active exp node, frozen sha,
  running jobs, last validated numbers, open gate violations, methodology version. Retain nothing
  after compaction -> full re-render. This is the "always know which task and how" answer.
- `TurnLifecycleContributor`: `on_turn_start` (record HEAD, dirty set), `on_item_completed`
  (claim gate), `on_turn_stop` (journal entry: task, `git diff --stat` vs baseline, commits made,
  tools used, runs started/finished, gate outcomes).
- `ToolLifecycleContributor`: `on_tool_start` (mirror `update_plan` args into tasks if used;
  apply_patch into frozen paths -> taint + steering), `on_command_start` (bench outside `exp_run`,
  git amend/rebase in exp worktrees, git in methods_repo -> taint), `on_tool_finish` (journal line).
- `TurnItemContributor`: visible "UNVERIFIED: n numbers not backed by a report" banner when the
  gate gave up after max corrections.
- Optional `TokenUsageContributor`: research budget like goal.
- Install: `app-server/src/extensions.rs` next to goal, passing `thread_manager.clone()`,
  gated by a feature flag.

### 7e. Anti-sycophancy gate, end to end
1. Model proposes: contract via `exp_create` (preregistered metric, seeds, controls, MDE).
2. Code freezes: `exp_freeze` commits + hashes + read-only.
3. Code measures: `exp_run` runs the team CLI with 5 seeds; results parsed by Rust into the ledger.
4. Code decides: `research_report` recomputes every claimed number and the verdict; only then does
   a report exist. Its canonical table is code-rendered.
5. Code checks prose: on the final AgentMessage, extract numbers (with units/percent/x) and success
   phrases ("fixed", "improved", "beats", "all tests pass", "SOTA", "you're absolutely right"
   followed by a changed conclusion). Each number must match a value in a report validated in this
   thread (within tolerance) or a paper claim quoted with its id; success phrases require a
   `supported` report this turn (or a passing `task_done` evidence). Violation -> inject
   correction listing the unbacked tokens and the exact tool to call; turn continues.
6. After `max_gate_corrections_per_turn`, show the UNVERIFIED banner + warning; never silently pass.
7. Honest limit: "you're absolutely right" capitulation without numbers is only detectable by
   pattern; code can require an evidence reference but cannot judge argument quality.

---

## 8. Extra requirements (task slicing, journal, methodology loop)

**(a) Reuse `update_plan`?** It is a core builtin (`core/src/tools/handlers/plan.rs:48-99`, spec
`plan_spec.rs:7-58`), stateless: it emits `EventMsg::PlanUpdate` and returns "Plan updated". There
is no server-side plan state; `PlanUpdate` is in the transient, non-persisted list of the rollout
policy (`rollout/src/policy.rs:189`, arm ends `=> false` at 204). So the plan lives only as
function-call args in model history and does not survive compaction or resume. It is also
disabled unless `[tools.update_plan] enabled = true` (`core/src/config/mod.rs:2717-2723`).
An extension cannot wrap or replace it, but can observe its finalized args in `on_tool_start`
(`ToolStartInput.payload`, `contributors/tool_lifecycle.rs:143`). Recommendation: own
`task_plan/task_start/task_done` tools with durable `tasks.json`; mirror update_plan calls if a
model uses it anyway.

**(b) Current task + how pinned every step:** world-state section (sec. 4, 7d). Diff-rendered each
sampling step, forced full re-render after compaction, rehydrated from `tasks.json` on resume.

**(c) Journal location and writer:** machine journal `.ukis/research/journal.jsonl` in the code
repo, committed alongside runs; per-task `journal/<task_id>.md` rendered from it; per-reproduction
`journal.md` copied into ukis-methods by `repro_record`. Written by code only: `on_tool_finish`
(every tool, outcome), `on_command_start` (argv/cwd), `on_turn_stop` (diff stat, commits, runs,
gate results), and the research tools themselves (contract hash, run meta, report ids). The model
only contributes the required `how` / acceptance strings via task tools. Retros (`retro_submit`)
bump the versioned `methodology.md`; eval-rule changes need user approval.

---

## 9. Unverified / risks
- `inject_if_running` from inside `on_item_completed` (lock ordering) - needs a test.
- Whether the TUI shows the TurnItemContributor-mutated final text or the already-streamed deltas.
- Serde behavior of an unknown `[research]` table in `config.toml` (tolerated vs warning).
- `MessagePhase` is optional; providers that don't set it make "final vs commentary" ambiguous
  (upstream Stop-request hook removes this ambiguity).
- execpolicy is prefix-based; creative command forms may evade it - ledger hash checks are the
  real guarantee.
