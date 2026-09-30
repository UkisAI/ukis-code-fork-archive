# OpenResearch (`orx`) - Experiment execution, remote compute, metrics/telemetry

Repo: `/home/pavle/projekti/vendor/OpenResearch` (Rust, `openresearch-cli` v0.2.14, MIT, (c) 2026 alphaXiv; `Cargo.toml:2-7`, `LICENSE:1-3`).
HEAD: `ea3b968 feat: add durable model and experiment telemetry (v0.2.14)`.
All paths below are relative to the repo root.

TL;DR
- Execution = "immutable git-archive snapshot of the experiment branch's commit -> detached run dir (`run.sh`, `log`, `pid`, `exit_code`) -> detached `orx supervise <runId>` that polls state every 5s, tails logs every 2s into a local per-run log file, and honours cancel intent from SQLite". Nine backends share one trait.
- There is **no metrics system**. The metric contract is "print everything to stdout; the agent greps the log". The v0.2.14 "telemetry" is **product analytics** (which harness/model launched an experiment, token usage per chat turn) shipped to `api.openresearch.sh`, not ML metrics.
- Dashboard = axum on 127.0.0.1, ~130 JSON routes + one SSE stream (`/api/events`) driven by a 500ms SQLite/log-file diff loop; websockets only for terminals/login flows. `orx up --remote` = SSH-forwarded remote `orx up` behind a local auth gateway.
- Reproducibility is commit + sha256 source digest + fixed run command. No env/dependency snapshot, no seed management, no config snapshot beyond the commit.
- No budget/queue/concurrency engine. Parallelism and "budget loops" are delegated to the agent via prompts (`orx exp wait --project` as the tick primitive).

---

## 1. How runs are executed

### 1.1 Object model
- Project -> tree of **experiments** (each = a git branch `orx/<slug>` + inherited `run_command`) -> **runs** (one execution of an experiment at a commit). Schema: `src/store.rs:444-455` (`runs`), `:501-530` (`local_projects`, `local_experiments`), migrations adding `commit_sha`, `result_markdown`, `cancel_requested`, `chat_session_id` at `:671-674`.
- SQLite at `<data dir>/orx.db`, WAL + `busy_timeout=5000` (`src/store.rs:440-442`). Log per run at `<data dir>/run-logs/<runId>.log` (`src/store.rs:215-222`).
- Run status machine: `starting -> running -> done|failed|cancelled` (`src/store.rs:254-284`). `update_status` is a guarded UPDATE that only transitions from non-terminal states, so terminal is sticky and concurrent writers cannot regress it (`src/store.rs:1082-1115`). On the terminal transition it also stages the telemetry report (`:1108-1110`).

### 1.2 Launch path (`orx exp run <expId> [--backend ...]`)
1. `LocalPlane::launch` (`src/plane/local_plane.rs:245-324`): fill backend/flavor from the saved compute default, validate flags, then **either** forward to the running `orx up` over HTTP (when invoked from inside an agent session, `trusted_up_port()` `src/local/chat/mod.rs:8739-8754`; `submit_run_via_up` `src/commands/up.rs:2137+`, bearer token from `ORX_UP_AUTH_TOKEN` `src/commands/up.rs:2126-2135`) **or** call the backend launcher directly.
2. Both paths converge on `compute::submit` (`src/compute.rs:741-868`):
   - `validate_run_args` (`src/compute.rs:669-739`) - per-backend flag matrix (e.g. `--timeout` not for local/ssh/ray, `--container` only ssh, `--manifest` only k8s).
   - `backend.preflight()` then `backend.stage_source()` -> `SourceSnapshot::create` (`src/compute.rs:34-69`): `git archive --format=tar <branch head sha>` (`:126-148`), sha256 it (`:168-182`), install content-addressed at `<data dir>/source-snapshots/<sha256>.tar`, mode 0600, dir 0700 + uid check (`:150-223`). Ray additionally gets a zip.
   - Run command resolution: experiment `run_command`, else project `run_command` (`src/compute.rs:771-780`).
   - `reserve_run` (`src/compute.rs:870-907`): per-experiment `fd_lock` file in `<data dir>/submission-locks/<expId>`, rejects if any non-terminal run exists for that experiment unless `--force`, then in one SQLite tx inserts the `starting` run row + the telemetry reservation.
   - `backend.submit(...)`; on failure before a provider handle was persisted, the run is marked failed with `result_markdown = "Compute submission failed: ..."` (`src/compute.rs:846-866`).
   - Optional GitHub branch publication is fire-and-forget after launch; compute never needs a push (`src/compute.rs:830-844`).
3. Provider handle is written **twice** for crash safety: `runs.backend_json` in SQLite and `<data dir>/submission-handles/<runId>.json` (`record_submission_handle`, `src/compute.rs:909-940`; `recover_submission_handle` `:942-951`). A supervisor that finds no `job_id` in SQLite recovers it from the file, else fails the run with "inspect provider for resources labelled or_run=<id>" (`src/commands/supervise.rs:62-80`).

### 1.3 The backend abstraction
```rust
// src/compute.rs:282-341
pub trait ComputeBackend: Send + Sync {
    fn capabilities(&self) -> Capabilities;          // id,label,remote,flavors,requires_flavor,source_transport
    async fn preflight(&self, args) -> Result<Preflight>;
    async fn stage_source(&self, project, experiment) -> Result<StagedSource>;
    async fn submit(&self, args, source, run_id) -> Result<StoredRun>;
    async fn status(&self, handle) -> Result<StoredRun>   // default: read SQLite
    async fn logs(&self, handle, cursor) -> Result<LogBatch> // default: 256 KiB base64 slice of local log file
    async fn cancel(&self, handle) -> Result<()>          // default: set cancel_requested + respawn supervisor
    async fn cleanup(&self, handle) -> Result<()>;
}
```
- Adapters are stamped out by `backend_adapter!` (`src/compute.rs:373-421`): `local`, `tinker`, `hf`, `modal`, `k8s`, `slurm`, `ray`, `openresearch` (`:423-644`); `SshCompute` is hand-written because it caches a resolved launch target (`:527-600`). Registry `backend(id)` (`:646-659`).
- **Key design point:** status/logs/cancel are never provider calls from the API. The provider is only talked to by the detached supervisor; everyone else reads SQLite + the local log file. That is what makes the dashboard, CLI and agents consistent and restart-safe.
- `BackendDescriptor` (`src/jobs/mod.rs:52-100`) is the serialized reattach handle: `kind` (`<backend>_job`), `job_id` (run dir / job id / sandbox id), `namespace`, `flavor`, `image`, `context`, `manifest`, k8s `resources`, openresearch `ssh_host/port/user`, `timeout_secs`, `source_digest/path/size`, `monitoring_error`, `cancellation_accepted`, `ssh_container`.
- Shared stage vocabulary: `SCHEDULING|RUNNING|UPDATING|COMPLETED|ERROR|CANCELED|DELETED` -> run status (`src/jobs/mod.rs:234-248`).

### 1.4 Local backend (this machine)
- `submit_controller_run` (`src/local/localrun.rs:50-167`): script = `snapshot_script(archive, cmd)` = `set -eo pipefail; mkdir -p repo; tar -xf <archive> -C repo; cd repo; <cmd>` (`src/compute.rs:225-231`). Env = synced `~/.openresearch/env` (`src/config.rs:344-368`) + `HF_TOKEN` + login-shell `PATH` + exported shell env; Tinker key passed as a secret env, not written to run.sh (`localrun.rs:83-111`).
- `localbox::run_job` (`src/jobs/localbox.rs:45-135`) writes `<data dir>/local-runs/<runId>/run.sh`:
  ```bash
  cd <dir> || exit 97
  ( trap 'exit 143' TERM
    trap 'code=$?; wait; echo "$code" > exit_code; exit "$code"' EXIT
    <script> ) > log 2>&1
  ```
  spawned with `process_group(0)` (pid == pgid), stdio null, `PYTHONUNBUFFERED=1` + `PYTHONIOENCODING=utf-8` defaulted (`src/jobs/mod.rs:29-44`). A reaper thread `wait()`s the child and writes `exit_code` if the trap did not (OR-337 fix, `localbox.rs:115-128`). `pid` and `pid_script` recorded.
- Liveness (`localbox.rs:138-160`): `ps -o stat=,command= -p <pid>`, not zombie, **and** command line ends with the unique `run.sh` path (defends against PID reuse). `inspect_job` (`:215-255`): `exit_code` file wins; live pid -> RUNNING; dead pid w/o exit_code -> ERROR "killed?" (with a 1s grace and re-reads to avoid races).
- Logs: supervisor re-reads the whole `log` and forwards lines past `skip` (`:258-268`). Cancel: `kill -TERM -- -<pgid>`, fallback to pid (`:272-330`). Windows path uses `taskkill /T /F`.

### 1.5 SSH backend (your own GPU box) - `--backend ssh --host <alias>`
- Transport is the system `ssh` binary; orx never reads keys. `ControlMaster=auto`, `ControlPersist=600`, socket in `/tmp/orx-ssh-<uid>-<hash>/`, `BatchMode=yes` for background calls, `ConnectTimeout=10` (`src/jobs/ssh.rs:1-15`, `:208-256`).
- `stage_source` (`src/jobs/ssh.rs:426-468`): upload the content-addressed tar **once per digest** to `~/.orx/source/<sha256>.tar` (atomic tmp+mv), then extract into `~/.orx/runs/<runId>/repo` (or stream into the container's `$HOME/.orx/runs/<runId>/repo`).
- `run_job` (`src/jobs/ssh.rs:520-555`): env exported **inside** `run.sh` (umask 077, file 0600). Launch via `setsid bash run.sh &` (fallback `nohup`/`set -m`), `echo $! > pid`. Host wrapper `host_script` + `TERMINAL_TRAPS` (`:486-505`): EXIT trap writes `exit_code`; TERM/INT trap forwards the signal to the child group, then after 2s `kill -KILL -<pgid>` and waits for the group to be empty before writing 143/130.
- Container mode (`src/jobs/ssh/container.rs`): targets an **already-running** Docker container (no lifecycle mgmt). Host wrapper verifies container `StartedAt`+`running` matches what was recorded at submit (generation check), then `docker exec <id> setsid --wait bash <run.sh>`; logs and `exit_code` stay on the host (`container.rs:213-219`). Inner script records `identity` = pid + process start time; cancel refuses to signal if start time changed, TERM group, 5s, KILL (`container.rs:176-194`, `:317-349`). `LaunchUncertain` error type lets submission reconcile when the launch ack was lost (`ssh.rs:508-517`, `src/local/ssh.rs:77-80`).
- Inspect (`ssh.rs:573-647`): one ssh round trip, bash helper that checks `exit_code`, else `/proc/<pid>/stat` for a non-zombie group member. Logs: `tail -n +<seen+1> ~/.orx/runs/<id>/log` every 2s (`:649-671`). Cancel: `kill -TERM -<pid>` (`:674-689`). Preflight only checks `bash` + `tar` (`:699-738`).
- Supervisor `watch_ssh_job` (`src/commands/supervise.rs:604-692`) is shared by ssh and openresearch; it also appends `[orx] <container status>` lines into the remote log when container state messages change.

### 1.6 Other backends (one line each)
| Backend | Submit | State source | Cancel | Timeout |
|---|---|---|---|---|
| `hf` | HF Jobs REST, labels `or_run` (`src/local/hf.rs:87`) | `inspect_job` poll + streamed logs w/ reconnect+dedup (`supervise.rs:39-181`) | HF cancel API | `timeoutSeconds` (`jobs/huggingface.rs:336`) |
| `modal` | bundled Python launcher driving `modal.Sandbox` in an orx-managed venv (`jobs/modal.rs:1-30`) | `Sandbox.from_id` | terminate | sandbox `timeout` (`modal.rs:52`) |
| `k8s` | `kubectl` + committed manifest (`.orx/k8s.yaml`), labels `or_run`, records created resources (`jobs/kubernetes.rs`) | kubectl | delete exactly recorded resources | injects `activeDeadlineSeconds` (`kubernetes.rs:242`) |
| `slurm` | ssh to login node, `job.sbatch` with `#SBATCH --output=log --open-mode=append --time --partition --account --gres` (`jobs/slurm.rs:154-189`), `sbatch --parsable` | `exit_code` file, then `squeue`/`sacct` | `scancel` | `--time` (default 4h) |
| `ray` | Ray Jobs REST with `working_dir` zip, `entrypoint_num_gpus/cpus` from `--flavor gpu:1,cpu:4` (`jobs/ray.rs:115-156,291`) | poll + full-log snapshot | POST stop | unsupported |
| `openresearch` | hosted `POST /sandboxes` (org-billed box), then ssh path (`jobs/openresearch.rs:1-9`) | `GET /sandboxes/{id}` until online, then ssh loop | ssh + `DELETE /sandboxes/{id}` | `timeout --signal=TERM --kill-after=30s` wrapper (`openresearch.rs:70-78`), default 4h |
| `tinker` | local controller process (same as local) + `ORX_RUN_ID` env, Tinker SDK does remote model ops (`localrun.rs:101-111`) | local | stops local controller only | none |

### 1.7 Supervision, resume, kill
- `spawn_detached_supervise` (`src/commands/exp.rs:179-226`): spawns `orx supervise <runId>` from the current binary path (handles replaced binaries), own process group, stdin/stdout null, stderr to `<runId>.supervisor.log` (rotated at 1 MiB), inherits the shell env so it resolves the same data dir.
- `supervise::run` (`src/commands/supervise.rs:39-181`): exclusive `fd_lock` on `<runId>.supervisor.lock` (`try_write`, a second supervisor exits silently, `:44-50`), bails if already terminal, dispatches on `descriptor.kind` (`:81-101`).
- Uniform "two-half" loop: a tokio task tails logs into the local log file (truncate on open and replay from line 0, so restarts never duplicate), the main loop polls state every `POLL_INTERVAL=5s`, flips status, and on terminal drains logs (20s cap) before exiting (`supervise.rs:24-28`, `:604-692`, `:972-1036`).
- **Resume after crash/reboot:** `orx up` startup respawns a supervisor for every `list_active_runs()` row (`src/commands/up.rs:90-100`). The supervisor reattaches purely from the descriptor (run dir / job id / sandbox id). OpenResearch supervisor checks `launched()` before re-launching payload so a restart mid-run reattaches instead of double-launching (`supervise.rs:846-850`).
- **Cancel** is intent-based: `request_local_run_cancel` sets `runs.cancel_requested=1` under a per-run `cancel.lock` and (re)spawns a supervisor, rolling back the flag if the spawn fails (`src/commands/exp.rs:228-265`). The supervisor sees the flag on its next tick and calls the backend's cancel; a terminal non-success stage after cancel is reported as `cancelled` (`supervise.rs:184-207`).
- **Monitoring loss** (Slurm): if inspect fails for > `MONITORING_GRACE=60s`, `monitoring_error` is written onto the descriptor; the run is **not** declared failed (`supervise.rs:1092-1199`). Chat host alerts the waiting agent once per outage (`src/local/chat/mod.rs:7955-7976`, `:8056-8100`).
- **Agent wakeups:** `orx exp wake <expId>` registers a `chat_run_wakeups` row; the chat host injects `[orx] Run X finished with status done|failed ...` as a hidden turn when the run terminates (`src/commands/exp.rs:92-125`, `src/local/chat/mod.rs:7940-7950`, `:7978+`).
- **Wait primitives:** `orx exp wait <expId>` (level-triggered on latest run) and `orx exp wait --project <id>` (edge-triggered: snapshot statuses, return on the first run that became terminal; prints `drained: no runs in flight` when idle) (`src/plane/local_plane.rs:348-435`). Pure SQLite polling (default 5s interval, 1800s timeout, nonzero exit on timeout).

### 1.8 `orx up --remote <host>`
- Not a job backend; it runs the whole orx (dashboard + agents + store) on a remote box and tunnels it. `src/commands/up_remote.rs:1-11`.
- Flow: probe remote `orx` (markers `ORX_REMOTE_PATH=`, `ORX_REMOTE_VERSION=` ... `:1689-1695`), install/upgrade with the GitHub release installer if missing/mismatched (`remote_installer` `:1990-2016`), start a **persistent** remote host (`orx remote-host ensure`, detached, with a Unix-socket control channel, `src/commands/remote_host.rs:31-100`, `:503-556`), then `connect_once` (`up_remote.rs:881-1053`): reserve a local loopback port, `ssh -L local:remote` + run `remote attach <instanceId>`, write a fresh 256-bit token on the SSH stdin, wait for `ORX_REMOTE_ATTACHED=1`, verify an authenticated health check, then heartbeat `ping\n` every 5s; disconnects are retryable with backoff.
- The laptop runs a local **gateway** axum router that proxies HTTP + WebSocket to the forwarded port, rewriting headers and injecting the bearer token (`up_remote.rs:1113-1632`). Remote server enforces bearer auth (sha256 digest set, constant-time callback token) and forbids some routes in SSH workspaces (`src/commands/up.rs:752-800`, `remote_host.rs:103-151`).
- `user@1.2.3.4:PORT` targets get `StrictHostKeyChecking=accept-new` (TOFU) for fresh cloud boxes; hostnames keep the user's own policy (`up_remote.rs:2018-2060`).
- Stop preview counts active turns, queued messages, pending permissions, **active runs** before letting you stop the remote host (`remote_host.rs:50-66`).

---

## 2. Metrics and telemetry

### 2.1 ML metrics: there is no metrics pipeline
- Grep for `wandb|tensorboard|mlflow|metric|loss` in `src/` finds nothing that parses or stores metrics. Former commands `wandb`, `chart`, `query`, `search-logs`, `explore`, `report` were **removed**; a test asserts they no longer parse (`src/main.rs:1372-1390`).
- **The metric contract is stdout.** From `agent-skills/orx-evidence/SKILL.md:6-47`:
  > Run logs are the evidence channel. Make the run command print everything needed to judge the result ... Print final metrics and a compact summary block at the end of the run ... Echo the configuration the run actually used ... For a long run, print periodic one-line metrics.
- `orx logs <runId>` (`src/commands/logs.rs:46-110`) deliberately prints only: absolute log path, byte size, last 500 chars, and instructions to `rg` it ("This preview is not proof of absence"; "Avoid reading the entire log file at once into the context window"). The agent is expected to grep the file and cite lines.
- Comparison = agent reads multiple logs; diffs of code between nodes via `/api/runs/{id}/diff` and `/api/experiments/{id}/diff` (git diff of run commit vs parent branch, `src/commands/up.rs:2380-2412`).
- Plotting = agent-authored matplotlib scripts using a vendored style module (`orx skill figures/assets/orx_figstyle.py`); rule "Every number comes from a run. Read metrics from the file located by `orx logs`" (`agent-skills/orx-figures/SKILL.md:40-44`, `:60-75`). No server-side charts; the UI has none either (no chart components in `ui/src/components`).
- Chat playbook enforces citing: measured results must be tagged `<run id="..." label="+3.65pp"/>` after reading the log (`SYSTEM_PROMPT.md:72-84`).
- Optional W&B only mentioned as a soft suggestion in the reproduce-paper template (`src/local/skills.rs:26`).
- `runs.result_markdown` is only used for failure reasons / teardown warnings, not results (`src/compute.rs:863`, `supervise.rs:945-970`). `runs.exit_code` is never set by supervisors in production (all calls pass `None`); the code only lands in `result_markdown` as "Job failed: exited with code N".

### 2.2 What v0.2.14 "durable model and experiment telemetry" actually is
Product analytics, not experiment metrics. Diff: `git show ea3b968` (+2203 lines, main file `src/store/telemetry.rs` 1183 lines).
- **Experiment attribution:** on `compute::submit`, a pending `cli_experiment_finished` event is prebuilt with `{harness, model, provider, status:"failed"}` and stored in `run_telemetry` in the same tx as the run row (`src/compute.rs:792-814`, `src/store/telemetry.rs:127-138`). When the run reaches a terminal status, `stage_run_terminal` patches `status` + `occurredAt` and moves it to `telemetry_pending_events` (`src/store/telemetry.rs:140-162`, called from `store.rs:1108-1110`). So the event survives crashes and is exactly-once per run.
- **Which model launched the run:** a Claude Code `PreToolUse` hook (`orx invocation-gate`, `src/commands/invocation_gate.rs`) rewrites every Bash tool call to prefix `export ORX_INVOCATION_CONTEXT='{"harness":"claude-code","model":"...","provider":...}';`, looking the identity up by `tool_use_id` from `native_invocation_identities` (waits up to 10s). `ExpRunArgs::invocation_identity` reads it (`src/main.rs:573-587`). Identity labels are validated to not contain paths/ARNs/URLs (`src/store/telemetry.rs:12-47`). Similar capture for codex/opencode/cursor/antigravity via their native events.
- **Token usage:** per chat turn, per harness native counters (`input/output/cache_read/cache_write/reasoning` tokens, `TokenUsage` `src/store/telemetry.rs:50-94`) with cumulative-baseline delta logic (resume/replay safe, `:164-226`), finalized into a `cli_chat_model_usage` event with `coverage` (complete/partial) (`:355-470`). Codex source: `thread/tokenUsage/updated` app-server notifications (`src/local/harness/codex.rs`, commit diff).
- **Delivery:** outbox files (`persist_payload`, atomic 0600 + dir fsync) -> `POST https://api.openresearch.sh/analytics/v1/cli-events` (`src/telemetry.rs:34-46`, `:730-790`, `:834-891`). Payload envelope: `{schemaVersion:1, installId, context:{cliVersion, buildChannel, os, arch, ci, installKind}, events:[{eventId, name:"cli_<event>", occurredAt, properties}]}` (`src/telemetry.rs:656-686`).
- **Opt-out:** only production builds send; `ORX_TELEMETRY_ENV` != production disables; `--no-telemetry` flag; `orx telemetry off` persists and purges queued + SQLite-staged events; unreadable settings fail closed (`src/telemetry.rs:548-632`).
- Relevance to a harness fork: the **pattern** (transactional outbox + terminal-state staging + hook-injected invocation identity) is reusable; the endpoint and event schema are alphaXiv-specific.

---

## 3. Dashboard (`orx up`) data/API

- axum server bound to `127.0.0.1` (default port 4790-ish; `src/commands/up.rs:72-89`), SPA embedded with `rust_embed` from `ui/dist` (`up.rs:7746-7750`), React + TanStack Router + xterm-style `LogTerminal` (`ui/src/components/LogTerminal.tsx`). Middleware: `loopback_guard` (Host/Origin must be loopback), optional remote bearer auth (`up.rs:752-800`).
- 132 `.route()` calls (`up.rs:471-752`). Run/experiment-relevant subset:

| Route | Handler | Shape |
|---|---|---|
| `GET /api/projects/{id}/experiments` | list_experiments | experiment rows |
| `GET /api/projects/{id}/runs` | `list_project_runs` (`up.rs:2037-2048`) | `{runs: ApiRun[]}` |
| `POST /api/runs` | `create_run` (`up.rs:2213-2249`) | `CreateRunReq {experimentId, backend, flavor, host, container, noContainer, manifest, image, timeout, org, disk, provider, force, chatSessionId, invocationContext, telemetrySuppressed}` -> `{run}` |
| `GET /api/runs/{id}` | `get_run` (`:2264-2274`) | `{run}` (+ backend cleanup on terminal) |
| `POST /api/runs/{id}/cancel` | `cancel_run` (`:2305-2316`) | `{ok, alreadyTerminal?}` |
| `GET /api/runs/{id}/logs?cursor=` | `run_logs` (`:2323-2332`) | `LogBatch {dataBase64, nextCursor, eof}` 256 KiB |
| `GET /api/runs/{id}/log?offset=` | `run_log` (`:2339-2351`) | `{dataBase64, nextOffset, eof}` 4 MB |
| `GET /api/runs/{id}/diff`, `/api/experiments/{id}/diff`, `/commits` | git diff vs parent branch (`:2380+`) | `{diff, truncated, bytesRead, byteLimit}` |
| `GET /api/instances` | `list_instances` (`:2285-2303`) | all runs across projects (<=500) + `projectName` |
| `GET /api/compute/backends` | capabilities list (`:2050-2052`) | |
| `GET /api/events` | SSE (`:7502-7515`) | see below |
| `/api/settings/{ssh,slurm,ray,compute,...}` | preflights, defaults | |

- `ApiRun` (`up.rs:879-919`): `{id, experimentId, projectId, status, backend (descriptor JSON), command, commitSha, resultMarkdown, createdAt, updatedAt, endedAt, exitCode, cancelRequested}`.
- **SSE `/api/events`** is the real-time spine: per subscriber, a 500ms loop (`event_loop` `up.rs:7580-7606`) diffs SQLite `updated_at` values and log-file sizes against an `EventCursor` (`:7556-7570`) and emits named events: `project.updated`, `experiment.updated`, `files.updated`, `run.updated {run}`, `run.log {runId, dataBase64, offset}`, `update.status`, plus chat events forwarded from a broadcast channel (`chat.session`, `chat.message`, `chat.busy`, ..., `resync.required` on lag) (`:7516-7548`). First pass = full snapshot; live runs replay their whole log, terminal runs start at EOF. Log bytes capped at 2 MB/tick, mpsc buffer 16 for backpressure (`:7506-7508`, `:7625-7705`). Client: one `EventSource` fans out per-run log listeners (`ui/src/events.ts:1-45`).
- WebSockets are only used for interactive terminals and login/connect flows (`up.rs:5427-5994`, e.g. `/api/settings/ssh/connect`, `/api/projects/{id}/terminal`, openresearch login).
- A separate minimal hand-rolled daemon `orx serve` exposes the same run store (`/runs`, `/runs/{id}/logs?offset`, SSE `/event`) for a hosted API to SSH-tunnel into (`src/commands/serve.rs:1-16`).

---

## 4. Reproducibility guarantees

| Aspect | What exists | Where |
|---|---|---|
| Code version | Every run records `commit_sha` = local branch head at launch; payload is `git archive` of that sha, never the worktree; uncommitted files are excluded by construction | `compute.rs:34-69`, `:126-148`; `orx-compute/SKILL.md` "Commit before launching" |
| Integrity | sha256 digest + size stored on descriptor; `SourceSnapshot::from_run` re-verifies before a delayed/restarted launch | `compute.rs:77-123` |
| Isolation | each run extracts into its own `run dir/repo`; remote cache keyed by digest | `jobs/ssh.rs:426-468`, `localbox.rs:22-29` |
| Command | run command stored on the run row; "fixed run contract": same command for the whole tree, vary code/config via child branches | `orx-experiment-tree/SKILL.md:6-12`; `compute.rs:771-780` |
| Frozen nodes | "never edit a node a run has answered" is **prompt-only**; nothing in code blocks commits to a branch that already has a done run | `orx-experiment-tree/SKILL.md:24-40`; no `frozen` in src |
| Concurrency | one in-flight run per experiment unless `--force` (code-enforced) | `compute.rs:870-907` |
| Env | synced `~/.openresearch/env` + HF_TOKEN (+ PATH/shell env for local) injected at launch; **not recorded** on the run; PYTHONUNBUFFERED/IOENCODING defaulted | `localrun.rs:81-111`, `local/ssh.rs:76-80`, `jobs/mod.rs:29-44` |
| Image | recorded for hf/modal/k8s (`descriptor.image`/manifest); ssh/slurm/local use whatever the host has | `jobs/mod.rs:52-100` |
| Dependencies | only via playbook policy: `uv run --locked`, commit locks, "run recipes must recreate dependencies from committed snapshots" | `SYSTEM_PROMPT.md:46-69` |
| Seeds | none. No seed injection, no seed field. Only prompt guidance ("one seed is an anecdote", figures skill) | `orx-figures/SKILL.md:47-49` |
| Config snapshot | only what the committed code + stdout echo provides ("Echo the configuration the run actually used") | `orx-evidence/SKILL.md:27-31` |
| Caveats | `git archive` drops submodules, LFS content stays pointers, honours `export-ignore`; exit code not stored in `runs.exit_code` | inferred from `compute.rs:126-148`; `update_status(..., None)` everywhere in `supervise.rs` |

---

## 5. Budget / cost / time / parallelism / queueing

- **Time limits:** backend-native only. hf `timeoutSeconds`, modal sandbox timeout, k8s `activeDeadlineSeconds` injected if absent, slurm `--time` (default 4h, configurable/clearable), openresearch `timeout --kill-after=30s` wrapper (default 4h). `--timeout` is rejected for local/ssh/ray (`compute.rs:670-683`). No wall-clock guard for local/ssh runs at all.
- **Cost:** `orx compute` lists GPU/CPU offers with `$/hr` from the hosted catalog (`GET /compute/catalog`, requires `orx login`; `src/commands/compute.rs:30-94`, `src/client.rs:50-80,359-363`). No spend tracking, no GPU-hour ledger; the reproduce template even says "Do not invent or maintain a GPU-hour ledger unless the user explicitly asks" (`src/local/skills.rs:25`).
- **Leak protection:** OpenResearch boxes are torn down after the watch loop even on error; failed teardown appends a "still billing" warning to `result_markdown` (`supervise.rs:936-970`). Resources are labelled `or_run=<id>` on hf/k8s/modal for manual cleanup.
- **Parallelism:** no scheduler/queue. Each `orx exp run` submits immediately; many experiments can run concurrently (one per experiment, more with `--force`). Local runs share the machine with no GPU pinning (`references/local.md`). Multi-node only insofar as the backend (slurm gres, ray resources, k8s manifest) provides it.
- **"Budget loop" is an agent pattern**, not code: launch a round of sibling nodes, then loop `orx exp wait --project` -> reconcile with `orx runs` -> refill/promote/stop; stop after ~3 failed/regressed runs; repair cap of 2 non-answering runs per node (`agent-skills/orx-experiment-tree/SKILL.md:24-40`, `:130-190`). Capacity is "total GPUs across in-flight runs" (`src/local/skills.rs:25`).
- Delegation: `orx agent spawn` helper sessions carry explicit "allowed compute" in their brief (`agent-skills/orx-agent-delegation/SKILL.md:35-50`).

---

## 6. What to lift into a Codex CLI fork vs what is product-coupled

### High-value, portable (MIT, mostly self-contained)
1. **Run-dir protocol + detached supervisor** (`jobs/localbox.rs`, `jobs/ssh.rs` host_script/TERMINAL_TRAPS/HOST_PROCESS_HELPERS, `commands/supervise.rs` two-half loop). File contract `run.sh | log | pid | exit_code` is trivially portable and makes runs survive agent/CLI/SSH death. Worth copying almost verbatim: pgid kill semantics, PID-reuse guard via `pid_script`, container generation/identity checks, EXIT-trap exit codes, reaper thread.
2. **Immutable content-addressed source snapshots** (`compute.rs:24-223`): `git archive <sha>` -> sha256 -> upload-once cache on remote. Cheap, strong "the run ran exactly this commit" guarantee without pushing branches. Add submodule/LFS handling if needed.
3. **`ComputeBackend` trait + `BackendDescriptor` reattach handle + dual persistence (SQLite + recovery JSON)** (`compute.rs:282-341`, `:909-951`, `jobs/mod.rs:52-100`). Clean seam for local/ssh/slurm/k8s adapters.
4. **Intent-based cancel via DB flag + supervisor respawn**, supervisor lockfile singleton, respawn-on-startup of all active runs (`exp.rs:228-265`, `supervise.rs:39-50`, `up.rs:90-100`).
5. **Agent-facing wait primitives**: level-triggered `wait <exp>` and edge-triggered `wait --project` returning on first completion + "drained" sentinel (`local_plane.rs:348-435`), and the **wakeup injection** of `[orx] Run X finished` into an idle agent session (`chat/mod.rs:7940-8100`). Directly applicable to a Codex fork's long-running-job UX.
6. **Log-as-evidence contract for agents**: `orx logs` returning path + size + 500-char tail + "grep, do not slurp" guidance (`commands/logs.rs`), plus the evidence skill and `<run id=.../>` citation tags. Cheap context-budget win.
7. **SSE diff loop** for a live UI (`up.rs:7502-7705`): cursor of `updated_at` + log offsets, base64 byte-exact chunks, per-tick byte budget, backpressure. Simple and robust.
8. **Monitoring-loss semantics** (do not declare failure on lost visibility; surface `monitoring_error` after 60s grace; alert waiting agent once) (`supervise.rs:1092-1199`).
9. **SSH plumbing**: ControlMaster multiplexing with short socket paths, BatchMode split, TOFU only for raw IP:port (`jobs/ssh.rs:205-256`, `up_remote.rs:2018-2060`). And the `--remote` pattern (remote persistent host + stdin-delivered token + authenticated loopback forward + local proxy gateway) if you want remote dashboards.
10. **Experiment-tree methodology** (skills text): fixed run contract, provisional vs frozen nodes, stacked-bush tree shape, repair caps. Pure prompt assets, portable to any harness. Consider enforcing "frozen" in code (reject commits/launches on an experiment whose latest run is `done`), which orx does not do.
11. **Hook-injected invocation identity** (`invocation_gate.rs`): PreToolUse rewrite that prefixes Bash commands with an env var carrying harness/model identity, so downstream CLIs can attribute work. Useful for provenance in a multi-model fork.

### Tightly coupled to OpenResearch / alphaXiv
- `openresearch` backend (`jobs/openresearch.rs`, `client.rs` `/sandboxes`, `/compute/catalog`), `orx login`, org billing, SSH key registration, `orx instance`.
- Product telemetry to `api.openresearch.sh/analytics/v1/cli-events` (`telemetry.rs`, `store/telemetry.rs`), install ids, onboarding/demo funnels.
- `orx paper` / `discover` / lit-review (alphaXiv paper API), Overleaf sync, LaTeX templates.
- Auto-update/installer from their GitHub releases (`updates.rs`, `up_remote.rs:1990`).
- Multi-harness chat host (`local/chat/mod.rs` 11k lines, `local/harness/*` driving Codex app-server, Claude Code, OpenCode, Cursor, Antigravity). A Codex fork already is the harness; only the Codex app-server event handling (e.g. `thread/tokenUsage/updated`) is interesting as reference.
- The React dashboard itself (i18n, onboarding, tours).

### Gaps to fill if you lift it
- A structured metrics side-channel (e.g. `metrics.jsonl` in the run dir tailed alongside `log`) - orx intentionally has none.
- Seeds and env snapshot (pip freeze / `uv.lock` hash / nvidia-smi / image digest) recorded on the run row.
- Persist real `exit_code`; wall-clock timeout for local/ssh; GPU pinning / slot accounting for local and ssh boxes; a real queue if you want fire-and-forget sweeps without an agent driving the loop.
