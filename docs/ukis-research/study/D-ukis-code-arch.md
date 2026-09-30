# ukis-code architecture study: where a research mode plugs in

Repo: `/home/pavle/projekti/ai-tooling/ukis-code` (branch `ukis-research`, HEAD `77a4296720`, clean; `main` == `ukis-research` == `origin/main`).
Read-only study. Nothing was built or modified. All paths below are relative to `codex-rs/` unless they start with the repo root.

---

## 0. TL;DR

- The fork is almost pure upstream Codex. The Ukis delta is 15 commits on top of upstream `d42056091a` (#49517, 2026-09-30, same day). In Rust it only touches TUI/CLI branding: 63 files, +155/-153, mostly insta snapshots. Everything else sits outside Rust: a Node launcher plus a Claude Agent SDK "provider bridge" in `scripts/providers/`.
- Upstream Codex now ships a real **in-process extension framework** (`ext/extension-api`) with typed contributors (tools, prompt/context, turn/thread lifecycle, MCP servers, approvals, model-request interceptors). It already hosts `/goal`, web search, memories, skills, MCP and queue as separate `ext/*` crates.
- **One install point covers TUI, `exec` and the app-server**, because the TUI and `exec` both embed the app-server in-process: `app-server/src/extensions.rs:50-122` (`thread_extensions`).
- The `/goal` extension (`ext/goal`) is already an autonomous "keep working until the objective is verified" loop: it re-launches turns when the thread goes idle, tracks a token budget, and exposes create/get/update tools. A research mode is structurally a sibling of this.
- Remote GPU runs need no Rust: `codex exec-server` runs on the GPU box and is registered in `~/.codex/environments.toml`. `exec_command` then takes an `environment_id` argument.

---

## 1. Workspace map (codex-rs, ~115 crates)

Line counts are non-test `.rs` lines, which gives a rough idea of weight.

### The ones that matter for this question

| Role | Crate (dir) | LoC | Purpose |
|---|---|---|---|
| **core / agent loop** | `core` (codex-core) | 244k | Session, submission loop, turn loop, tool router/registry, built-in tool handlers, compaction, multi-agent, guardian. AGENTS.md: "resist adding code to codex-core". |
| core API facade | `core-api` | 0.1k | Re-exports for embedders (incl. `ExtensionRegistryBuilder`, `core-api/src/lib.rs:92`) |
| **extension API** | `ext/extension-api` | 2.3k | Contributor traits and registry that core calls into (see section 3) |
| extensions | `ext/goal` (3.3k), `ext/skills` (18.7k), `ext/mcp` (3k), `ext/web-search` (0.9k), `ext/memories` (2.6k), `ext/queue`, `ext/history-notes`, `ext/image-generation`, `ext/guardian-v2` (12.8k), `ext/guardian-reviewer`, `ext/git-attribution`, `ext/agent-message-board`, `ext/agent`, `ext/connectors`, `ext/items` | | Feature crates built on extension-api |
| **tools** | `tools` (codex-tools) | 7.7k | `ToolSpec`, `ToolExecutor` trait (`tools/src/tool_executor.rs:106`), Responses API tool JSON types |
| **tui** | `tui` | 359k | ratatui UI; talks to an in-process app-server |
| **exec** | `exec` | 6.1k | `codex exec` headless runner (also through in-process app-server, `exec/src/lib.rs:21-23,986`) |
| **app-server** | `app-server` | 62k | JSON-RPC server (stdio/ws/UDS). Owns thread lifecycle and installs extensions |
| app-server protocol | `app-server-protocol` | 35.6k | v1/v2 request/notification types, TS/JSON schema export |
| app-server client | `app-server-client` | 3.3k | In-process and remote client facade used by TUI and exec (`app-server-client/src/lib.rs:1-17`) |
| app-server-transport / -daemon / -test-client | | | Transport, background daemon, test client |
| **mcp** | `codex-mcp` (25k), `rmcp-client` (25k), `ext/mcp` | | Connection manager, tool catalog/naming, rmcp client, runtime MCP contributors |
| **skills** | `skills` (2.6k), `ext/skills` (18.7k) | | SKILL.md parsing, bundled system skills, discovery, catalog prompt, `skills.*` tools |
| **config** | `config` (30k), `config-schema`, `features` (3.5k) | | `ConfigToml`, layer stack, MCP types, requirements; feature flags |
| **protocol** | `protocol` (31.7k) | | Core Op/EventMsg, models (ResponseItem), items, dynamic tools |
| hooks | `hooks` (16k) | | Claude-Code-style lifecycle hooks (command + MCP runners) |
| prompts | `prompts` (2.7k) | | Base/model instructions, compaction prompt, review prompts |
| plugins | `core-plugins` (48k), `plugin`, `utils/plugins` | | Plugin marketplace/install, command-to-skill migration |
| agents | `agent-roles`, `agent-graph-store`, `agent-identity`, `agent-message-board-client` | | Role files for spawned subagents; agent graph; identity |
| execution | `exec-server` (44.8k), `exec-server-protocol`, `shell-command`, `sandboxing`, `linux-sandbox`, `bwrap`, `windows-sandbox-rs`, `execpolicy`, `network-proxy` | | Local and remote process/filesystem execution, sandboxes, policies |
| state | `state` (26k, SQLite), `rollout` (16.7k), `rollout-trace`, `thread-store` (34.6k), `history`, `message-history` | | Persistence of threads, goals, rollouts |
| model | `codex-api` (15k), `codex-client`, `model-provider`, `model-provider-info`, `models-manager`, `ollama`, `lmstudio`, `responses-api-proxy` | | Responses API client, provider abstraction, model catalog |
| code mode | `code-mode`, `code-mode-host`, `code-mode-protocol`, `code-mode-runtime`, `v8-poc` | | JS "code mode" tool host (V8) |
| misc | `cli` (29.8k, the `codex`/`ukis` multitool), `login`, `otel`, `analytics`, `feedback`, `git-utils`, `worktree`, `file-search`, `file-watcher`, `mermaid`, `realtime-webrtc`, `voice-host`, `cloud-tasks*`, `connectors`, `secrets`, `utils/*` | | |

---

## 2. The agent loop

### Call chain

```
app-server (in-process for TUI/exec)
  -> core Session::submission_loop          core/src/session/handlers.rs:420   (Op dispatch)
    -> Session::spawn_task<T: SessionTask>   core/src/tasks/mod.rs:272 (trait at :180)
      -> RegularTask::run                    core/src/tasks/regular.rs:103-120 (loops run_turn while pending input)
        -> run_turn                          core/src/session/turn.rs:163
           loop {                            core/src/session/turn.rs:426
             drain pending (steered) input   :430-449
             capture StepContext (tools, MCP, world state)  :468-507
             record world state / reminders  :509-521
             run_sampling_request            core/src/session/turn.rs:1618
               ToolCallRuntime::new          :1638  (core/src/tools/parallel.rs:54)
               build_prompt + retry loop     :1653-1700
               try_run_sampling_request      core/src/session/turn.rs:2525
                 client_session.stream(...)  :2575-2587  (Responses API stream)
                 for ResponseEvent::OutputItemDone -> handle_output_item_done
                                             core/src/stream_events_utils.rs:315
                   ToolRouter::build_tool_call(item)   core/src/tools/router.rs:248
                   tool call  -> ToolCallRuntime::handle_tool_call (parallel.rs:77), needs_follow_up = true (stream_events_utils.rs:356)
                   message    -> finalize turn item, last_agent_message
                   RespondToModel error -> synthetic FunctionCallOutput, needs_follow_up = true (:424)
                 in-flight tool futures drained (FuturesOrdered)
             needs_follow_up = model_needs_follow_up || has_pending_input   turn.rs:566
             context-limit rollover / auto-compact -> continue             turn.rs:601-651
             if !needs_follow_up:
                run_turn_stop_hooks          turn.rs:655 (core/src/hook_runtime.rs:392)
                Stop hook "block" + prompt -> inject continuation, continue   turn.rs:668-690
                else post-turn compaction, break                            turn.rs:700-755
           }
```

### Stop and continue rules
- The loop continues while the model emitted at least one tool call, or while the user steered new input mid-turn (`turn.rs:543-566`).
- The turn ends when the model returns only an assistant message and no Stop hook blocks.
- A `Stop` hook can force continuation by returning a block decision plus a prompt (`turn.rs:668-690`). This is the cheapest "don't stop until done" lever.
- **Cross-turn autonomy** is not part of `run_turn`. It lives in extensions: `ThreadLifecycleContributor::on_thread_idle` (`ext/extension-api/src/contributors.rs:194`) runs after the thread goes idle. The goal extension uses it to call `CodexThread::start_turn_if_idle` (`core/src/codex_thread.rs:390`) with a steering item (`ext/goal/src/runtime.rs:425-505`). `ThreadIdleCause` (Completed/Interrupted/Failed) is at `ext/extension-api/src/contributors/thread_lifecycle.rs:59`.

### Key types
- `Session`: `core/src/session/session.rs`
- `TurnContext` / `StepContext`: `core/src/session/turn_context.rs`, `core/src/session/step_context.rs`
- `ModelClientSession`: `core/src/client.rs`
- `ToolRouter` (`core/src/tools/router.rs`), `ToolRegistry` and `CoreToolRuntime` (`core/src/tools/registry.rs:56,301-430`), `ToolCallRuntime` (`core/src/tools/parallel.rs`)
- Tool set per step is assembled in `core/src/tools/spec_plan.rs` (1534 lines). Built-ins are registered at roughly `:1200-1420`. Extension tools come from `extension_tool_executors` (`:331-346`) and `append_extension_tool_executors` (`:1477`), wrapped by `ExtensionToolAdapter` (`core/src/tools/handlers/extension_tools.rs:32-73`).

---

## 3. Extension points, ranked from least to most invasive

### (a) Skills: zero Rust, zero merge cost
- **Format:** a directory with `SKILL.md` and YAML frontmatter `name`, `description`, optional `metadata.short-description` (`skills/src/parser.rs:6-28`; lenient YAML repair at `:50-60`). The name is capped at 64 chars (`:4`).
- **Optional sidecar** `agents/openai.yaml` in the skill dir (`ext/skills/src/loader/mod.rs:21`; schema at `ext/skills/src/loader/metadata.rs:27-75`). It supports:
  - `interface`
  - `dependencies.tools[]` of type/value/transport/command/url, i.e. MCP server dependencies the skill needs
  - `policy.allow_implicit_invocation`, `policy.products`
  - Example: `skills/src/assets/samples/review-agent/agents/openai.yaml`
- **Discovery roots** (`ext/skills/src/host_roots.rs:28-131`):
  - `$CODEX_HOME/skills` (deprecated user location, `:96-101`)
  - `~/.agents/skills` (`:103-108`)
  - `$CODEX_HOME/skills/.system`: embedded system skills from `skills/src/assets/samples/{imagegen,openai-docs,review-agent,skill-creator,skill-installer}`, installed by `skills/src/lib.rs:58-71`
  - project `.codex/skills` (`:86-93`)
  - system config folder `skills` (Admin scope)
  - plugin skill roots
  - `.agents/skills` in every directory between the project root and cwd (`:137-175`)
- **Invocation:** the skill catalog is rendered into context (`ext/skills/src/catalog_prompt.rs`). Invocation is implicit (the model decides) or explicit via `$name` mention (`utils/plugins/src/mention_syntax.rs:4`; `@` is for plugins, `:7`). There are also `skills.list` / `skills.read` tools (`ext/skills/src/tools/mod.rs:55`, `tools/read.rs:66`).
- **Custom prompts:** the legacy `~/.codex/prompts/*.md` slash prompts are gone. `core-plugins/src/command_migration.rs:14` converts old commands into "source-command" skills, so **skills are the slash-prompt mechanism now**.

### (b) MCP servers: zero Rust, zero merge cost
- **Config:** `[mcp_servers.<name>]` in `~/.codex/config.toml` or project `.codex/config.toml` (`config/src/config_toml.rs:294`). The struct is at `config/src/mcp_types.rs:223`. Fields:
  - `enabled`, `required` (exec fails if the server is down)
  - `startup_timeout_sec`, `tool_timeout_sec`
  - `enabled_tools` / `disabled_tools`
  - `default_tools_approval_mode`, per-tool `tools.<name>`
  - `supports_parallel_tool_calls`
  - `environment_id`: which exec environment runs the server
  - `omit_tools_from`, OAuth settings
- **Transports** (`mcp_types.rs:614-645`):
  - stdio (`command`, `args`, `env`, `env_vars`, `cwd`)
  - streamable HTTP (`url`, `bearer_token_env_var`, headers, `http_headers_helper`)
- **Tool exposure:** names are namespaced by server. The legacy `mcp__server__tool` prefix is at `codex-mcp/src/tools.rs:22,111,225`. Tools can be deferred behind `tool_search` when there are many (`spec_plan.rs` "search_tool_enabled").
- **Guidance:** AGENTS.md says MCP tool mutation should go through `codex-mcp/src/mcp_connection_manager.rs`.
- **Programmatic MCP:** an extension can add or remove servers at runtime through `McpServerContributor` (`ext/extension-api/src/contributors.rs:83-102`). Example: `ext/mcp/src/lib.rs:38-67` injects the hosted apps server.
- **Ukis caveat:** under the Claude bridge, hosted OpenAI `web_search` is dropped (`scripts/providers/README.md`, "Scope and limitations"). Literature search must therefore be an MCP or extension tool to work with both providers.

### (c) Native tools, two flavours

**c1. Extension tool (recommended; lives in a new crate).**
- The trait is `ToolContributor::tools` / `tools_for_step` (`ext/extension-api/src/contributors.rs:357-374`). Each tool implements `ToolExecutor<ToolCall<'_>>`: `tool_name`, `spec`, `exposure`, `supports_parallel_tool_calls`, `handle` (`tools/src/tool_executor.rs:106-130`).
- **Template: `ext/web-search`** (896 LoC).
  - `ext/web-search/src/extension.rs`: per-thread config is inserted on thread start (`:97-108`) and refreshed on config change (`:110-120`). `ToolContributor` builds the tool from the thread store (`:122-150`). `install()` registers 3 contributors (`:152-157`).
  - `ext/web-search/src/tool.rs:55-95`: `ToolExecutor` impl with a namespaced `ToolSpec::Namespace` (namespace `web`, tool `run`). The description comes from `web_run_description.md` via `include_str!`. The handler emits started/completed `ExtensionTurnItem`s for the UI (`:130+`).
- **Files to touch for a new `ext/research` crate:**
  1. `ext/research/{Cargo.toml,BUILD.bazel,src/*.rs}`. Use `compile_data` for any `include_str!` templates, like `ext/goal/BUILD.bazel`.
  2. `Cargo.toml`: add a workspace member (list starting at line 1) and a `[workspace.dependencies]` entry (next to `codex-goal-extension` at `:227`).
  3. `app-server/Cargo.toml`: add the dependency (like `:54`, `:76`).
  4. `app-server/src/extensions.rs:67-121`: one `codex_research_extension::install(&mut builder, ...)` line.
  5. Run `just bazel-lock-update` to refresh `MODULE.bazel.lock` (AGENTS.md rule).
  6. Optional: `cli/src/main.rs:2038-2055` (debug prompt-input command) if the prompt dump should show it.
- **Result:** about 3-4 small hunks in upstream files (workspace member line, workspace dep line, app-server Cargo dep, one install line). Low rebase pain.

**c2. Core built-in handler (avoid).**
- Template: `CurrentTimeHandler`, `core/src/tools/handlers/current_time.rs:24-110`. It is namespace `clock`, tool `curr_time`, implements `ToolExecutor<ToolInvocation>` with a custom `ToolOutput`.
- It is exported in `core/src/tools/handlers/mod.rs:60` and registered in `core/src/tools/spec_plan.rs:1224-1233`, gated by `Feature::CurrentTimeReminder` or model metadata.
- A new built-in touches `handlers/mod.rs`, `spec_plan.rs` (a 1.5k-line hot file) and likely `features/src/lib.rs`. All of that is core churn; AGENTS.md explicitly pushes back on it.

**c3. Dynamic tools over app-server (zero Rust, client-owned).**
- `thread/start` accepts `dynamicTools` (experimental, `app-server-protocol/src/protocol/v2/thread.rs:145`).
- When the model calls one, the server sends the client a JSON-RPC **server request** `item/tool/call` (`app-server-protocol/src/protocol/common.rs:1802-1805`). The client executes it and replies.
- Core side: `DynamicToolHandler`, appended by `append_dynamic_tool_runtimes` in `spec_plan.rs`.
- This lets an external research orchestrator (Python/TS) own the tools without touching Rust. It requires the experimental API capability.

### (d) Slash commands / custom prompts
- TUI slash commands are a closed enum, `tui/src/slash_command.rs:9-85`, with descriptions at `:137`. Dispatch is in `tui/src/chatwidget/slash_dispatch.rs`; see `/goal` at `:346-358` and `:927+`. The popup filter is at `tui/src/bottom_pane/slash_commands.rs:79`.
- Adding `/research` means editing TUI hot files and snapshot tests, which is a medium upstream-merge cost. AGENTS.md warns against growing `chatwidget.rs`.
- Cheaper alternative: ship a `research` skill invoked as `$research ...`. This needs no TUI edits.

### (e) Subagents / multi-agent
- **Built in and stable:** `multi_agent` (Collab) is Stable and on by default (`features/src/lib.rs:1351-1356`). `multi_agent_v2` is Stable but off by default (`:1357-1362`).
- **Tools:** spawn/wait/send/close/resume (v1) and spawn/send_message/followup_task/wait/list/interrupt (v2). Registration is at `core/src/tools/spec_plan.rs:1298-1420` (`add_collaboration_tools`); handlers are in `core/src/tools/handlers/multi_agents*/`; runtime is in `core/src/agent/` (`control.rs`, `registry.rs`, `role.rs`).
- **Roles:** agent role files are TOML (`agent-roles/src/agent_role_config.rs:21-35`). A role file has `name`, `description`, `nickname_candidates`, plus a flattened full `ConfigToml` (model, instructions, MCP servers, and so on).
  - Loaded from `<config folder>/agents/` (`agent-roles/src/loader.rs:78`) or from `[agents.<role>]` with `config_file` in config.toml (`loader.rs:130-290`).
  - `spawn_agent` exposes `agent_type` when roles exist (`spec_plan.rs:1327`).
  - This covers "literature-reviewer", "experiment-runner" and "analyst" research roles with **zero Rust**.
- `agent_message_board` (UnderDevelopment, `features/src/lib.rs:1369-1374`; `ext/agent-message-board`) is a shared board between agents.
- `thread/fork` (`common.rs:563`) and the `worktree` crate (`worktree/src/lib.rs`, Desktop-compatible managed git worktrees) are useful primitives for "experiment branch = worktree + forked thread".

### (f) Hooks
- **Events** (`hooks/src/schema.rs:102-125`; protocol `protocol/src/protocol.rs:1579`): PreToolUse, PermissionRequest, PostToolUse, PreCompact, PostCompact, SessionStart, SessionEnd, UserPromptSubmit, SubagentStart, SubagentStop, Stop, Interrupt. The `hooks` feature is Stable and on (`features/src/lib.rs:1239-1243`).
- **Config:** `hooks.json` in any config folder (`hooks/src/engine/discovery.rs:339-343`) or a `[hooks]` table in config.toml (`:387`).
- **Runners:** command (`hooks/src/engine/command_runner.rs`) and MCP (`hooks/src/engine/mcp_runner.rs`).
- **Uses for research:**
  - A `Stop` hook can block termination and inject a continuation prompt (`core/src/session/turn.rs:668-690`). That gives "run until metric target met" from a shell script.
  - `PostToolUse` can log metrics from a training command's output into a tracker.
- Zero Rust.

### (g) Extension contributors beyond tools (in-process, new crate)
Registry builder: `ext/extension-api/src/registry.rs:21-150`.

| Contributor | Location | What a research mode would use it for |
|---|---|---|
| `ContextContributor` | `contributors.rs:109-157` | Thread/turn prompt fragments into `PromptSlot::{DeveloperPolicy, DeveloperCapabilities, ContextWindow}` (`contributors/prompt.rs:7-13`), plus "world state" sections that survive compaction (`:146-156`). This is where an experiment-tree summary or research state goes. |
| `TurnInputContributor` | `:282-293` | Per-turn `ContextualUserFragment`s. AGENTS.md requires bounded, at most 10k tokens, and defined as structs. |
| `ThreadLifecycleContributor` | `:164-209` | start/ready/resume/idle/stop. `on_thread_idle` is the autonomous-continuation hook. |
| `TurnLifecycleContributor` | `:216-274` | turn start/stop/abort/error, `on_item_completed` |
| `ToolLifecycleContributor` | `:381-418` | Observe every tool/command start/finish, MCP results. Example use: capture `exec_command` of a training script and parse metrics. |
| `TokenUsageContributor` | `:316-330` | Budget accounting |
| `ApprovalReviewContributor` | `:422-430` | Auto-approve or deny policy |
| `ModelRequestContributor` / `ModelResponseInterceptor` | `model_request.rs:31-40` | Request metadata and stream interception |
| `TurnItemContributor` | `:437-444` | Mutate turn items before emit |

Other pieces:
- **State stores:** `ExtensionData` exists at session, thread, turn and step scopes (`ext/extension-api/src/state.rs`).
- **Custom UI items:** `ExtensionItem` enum in `ext/items/src/lib.rs:35-46` (`image_gen.generation`, `clock.sleep`, `web.search`). A new variant also needs an app-server wrapper and TUI rendering, so it is a small multi-crate touch.
- **Event sink to clients:** `app-server/src/extensions.rs:124-294` only forwards `ThreadQueueChanged`, `ThreadGoalUpdated` and warnings. Other events are dropped (`:233-235`), so custom research notifications would need a protocol addition.

### (h) App-server protocol for external UIs
- **Transports and API:** JSON-RPC 2.0 over stdio, ws or UDS. v2 methods are defined in `app-server-protocol/src/protocol/common.rs`: 265 method mappings in total. Thread methods start at `:551` (`thread/start`, `resume`, `fork` `:563`, `goal/set|get|clear` `:614-628`, `queue/*` `:630-664`, `metadata/update` `:665`, `turns/list`, and more).
- **Server requests** include approvals, elicitation, and `item/tool/call` (`:1802`).
- **Environments:** `environment/add` (experimental, `:1205-1210`).
- **Docs:** `app-server/README.md` (514 lines).
- **Schema export:** the TS/JSON schema is exported for SDKs. `sdk/typescript` wraps the CLI with JSONL; `sdk/python` also exists.

### (i) Remote execution (for remote GPU runs), zero Rust
- Start `codex exec-server` on the GPU box (ws, optional bearer token; `exec-server/README.md:1-60`).
- Register it in `$CODEX_HOME/environments.toml` (`exec-server/src/environment_toml.rs:24-47`). Fields:
  - `default`, `include_local`
  - `[[environments]]` entries with `id`, `url`, `auth_bearer_token`, `program`/`args`/`env`/`cwd`, and timeouts
- With multiple environments, `exec_command` gains an `environment_id` parameter (`core/src/tools/handlers/shell_spec.rs:24-28,82-90`; `core/src/tools/handlers/unified_exec.rs:56`).
- MCP servers can also be pinned to an environment (`mcp_types.rs` `environment_id`).
- Unverified: the exact multi-environment trigger conditions (`include_environment_id`) were not traced end to end.

---

## 4. What UkisAI changed vs upstream

**Git facts**
- Only an `origin` remote (`git@github.com:UkisAI/ukis-code.git`). No `upstream` remote is configured in this checkout, although README says to keep one pointing at `openai/codex`.
- The 15 Ukis commits (author Dakaa289, all 2026-09-30) sit linearly on upstream `d42056091a` ("Add a fork shortcut to the TUI command center (#49517)", Eric Traut, 2026-09-30). No merge commits, so the fork was cut or rebased very recently.
- Rust delta vs base: `git diff --stat d42056091a HEAD -- codex-rs` gives 63 files, +155/-153. It covers:
  - `cli/src/main.rs`: clap `name="ukis-code"`, `bin_name="ukis"`
  - `exec/src/event_processor_with_human_output.rs:219`: "Ukis Code vX"
  - `tui/src/history_cell/session.rs:47,365`: "Ukis Code" title
  - `tui/src/onboarding/welcome.rs:129-130`
  - `tui/src/empty_state_animation/{paths,lighting,geometry}.rs`: traced chain logo, pink/violet lighting
  - `tui/src/style/contrast*.rs`
  - many `.snap` updates
- No core, protocol, config or app-server changes.

**Commit themes (oldest to newest)**
1. `f891d3954f` Branding: README rewritten, the original is kept as `UPSTREAM.md`; `branding/` (SVG, `trace_logo.py`, `preview.rs`); TUI logo and colors; `.github/workflows/ukis-branding.yml`.
2. `50de42121c` Snapshot verification and preview PNG.
3. `17c39d14df`, `ab468e21fe`, `94516bd1fb` Windows preview build, `ukis` Windows launcher, code-mode host in the distro.
4. `e4b34a5063`, `9139d37c9b`, `90270f54ec`, `66bd15a80f` **Claude provider bridge** (`scripts/providers/*.mjs`, about 1.3k lines of Node). `launch.mjs:72-86` injects `-c model_provider="ukis"`, `model_catalog_json`, and `model_providers.ukis.*` (`base_url` = a local 127.0.0.1 bridge, `wire_api=responses`).
   - The bridge routes per model: OpenAI requests pass through; Claude requests are translated into Claude Agent SDK turns.
   - Codex tools are exposed to Claude through an in-process MCP server, and Claude's built-in tools are disabled.
   - Codex still owns history, tools, sandbox and approvals. `/model` switches OpenAI and Claude mid-conversation.
   - Tests: `node --test scripts/providers/provider.test.mjs scripts/providers/unified.test.mjs` (CI: `.github/workflows/ukis-providers.yml`).
5. `e6fd2c25c0`, `86fdf1d5f7`, `9d40182586`, `58518bbb3d`, `3a6c865e0f`, `77a4296720` IBM Plex Mono Windows Terminal profile, `ukis` window command, rename to "Ukis Code", temp-dir fix.

**Fork conventions (README.md, branding/README.md)**
- "Brand changes stay in the TUI presentation layer; optional provider integration lives in the launcher and `scripts/providers`."
- "Do not rename wire-protocol fields, authentication providers, or `.codex` paths." It reuses `~/.codex` config and login.
- Keep LICENSE/NOTICE, Apache-2.0.
- **Build:** `cd codex-rs && cargo build --release --bin codex --bin codex-code-mode-host`, then `cd scripts/providers && npm ci`, then `node scripts/ukis-code.mjs` (the launcher; `UKIS_CODE_BIN` overrides the binary).
- **Dev** (inherited AGENTS.md):
  - `just fmt`
  - `just test -p <crate>` (nextest; never plain `cargo test`)
  - `just fix -p <crate>`
  - `just write-config-schema` after `ConfigToml` changes
  - `just bazel-lock-update` after Cargo dependency changes
  - insta snapshots are required for any UI change
  - core-agent changes need integration tests in `core/suite` via `test_codex`
  - changes should stay under 800 lines; modules under 500 LoC
  - resist adding to codex-core
- **Inherited OpenAI CI workflows** remain enabled; some need upstream infrastructure (`branding/README.md`, Validation).
- **Direction:** a branded, multi-provider (OpenAI plus Claude subscription) Codex. The engine is intentionally kept upstream-identical, and all Ukis logic so far lives outside Rust.

---

## 5. Recommendation: integration points for a "research mode" (harness side only)

Ordered by value-to-merge-cost ratio.

### R1. Zero-Rust layer: skill + MCP server + agent roles + environments.toml (+ optional Stop hook)
- **What:**
  - A `research` skill in `.agents/skills/research/SKILL.md`, with `agents/openai.yaml` declaring the MCP dependency. It holds the method: literature, then hypothesis, then experiment branch, then run, then metrics, then decide.
  - A `research` MCP server (stdio or HTTP) exposing:
    - `lit.search` / `lit.read` (arXiv, alphaXiv, Semantic Scholar)
    - `exp.branch` / `exp.list` / `exp.compare`, as git branches or worktrees plus JSON/SQLite metrics
    - `run.submit` / `run.status` / `run.logs` for remote GPU
    - `metrics.log` / `metrics.query`
  - Role TOMLs in `.codex/agents/` for lit-reviewer, experimenter and analyst, used via `spawn_agent`.
  - The GPU box as an exec-server environment in `environments.toml`.
  - Optionally, a Stop hook that blocks until a metric target or budget is hit.
- **Why:** it works identically under OpenAI and the Claude bridge. That matters because hosted `web_search` is dropped for Claude. It also works in TUI, exec and app-server at once, and matches the fork's stated convention of keeping logic outside Rust.
- **Merge cost:** none. It can also be launcher-injected (`scripts/providers/launch.mjs` already passes `-c` overrides) so a `ukis --research` flag adds `-c mcp_servers.research...`.
- **Limits:**
  - No native UI items; the TUI shows plain MCP tool calls.
  - No cross-turn autonomy beyond the Stop hook and `/goal`.
  - The model sees state only through tool results.

### R2. New `ext/research` crate on `codex-extension-api`, modelled on `ext/goal` + `ext/web-search`
- **What:**
  - Native namespaced tools via `ToolContributor`.
  - A bounded "research state / experiment tree" world-state section via `ContextContributor` (survives compaction).
  - Autonomous iteration via `ThreadLifecycleContributor::on_thread_idle` plus `start_turn_if_idle`, exactly like `ext/goal/src/runtime.rs:425-505`. That gives loop-until-budget or loop-until-verified with a token budget via `TokenUsageContributor`.
  - Metric capture from training commands via `ToolLifecycleContributor::on_command_start` / `on_tool_finish`.
  - The existing `/goal` UI can be reused: set a goal whose objective is the research question.
- **Upstream touch:**
  - about 4 one-line hunks: `Cargo.toml` members and workspace dep, `app-server/Cargo.toml`, one `install` call in `app-server/src/extensions.rs:67-121`
  - `MODULE.bazel.lock` regeneration
- **Merge cost:**
  - Low. The conflict surface is a few list lines in frequently edited files, trivial to re-resolve.
  - The real risk is **API churn in `ext/extension-api`**: it is new and evolving, e.g. the "All this file should be replaced..." comment at `contributors/prompt.rs:1`, and the `ToolContributor`/`TurnLifecycleContributor` signatures. Expect occasional compile fixes in our crate, not textual conflicts.
- **Config:**
  - Avoid adding fields to `ConfigToml` (`config/src/config_toml.rs:166`), because that forces a schema regeneration and conflicts.
  - Prefer reading a separate `$CODEX_HOME/research.toml` or env vars.
  - A new `Feature` flag in `features/src/lib.rs` is a one-enum-plus-spec hunk (small conflict risk) if gating is wanted.
- **Persistence:**
  - Do not add migrations to `codex-state` (upstream-owned SQLite, the way goals do).
  - Keep our own SQLite or JSONL under `$CODEX_HOME/research/` or the repo `.research/`.

### R3. External orchestrator over the app-server protocol (dynamic tools + thread/fork + goals)
- **What:** a research driver (Python via `sdk/python`, or TS) that:
  - starts threads with `dynamicTools` (`v2/thread.rs:145`) and answers `item/tool/call` (`common.rs:1802`)
  - forks threads per experiment branch (`thread/fork`, `:563`)
  - drives iteration with `thread/goal/set` (`:614`) or `turn/start`
  - adds GPU environments dynamically with `environment/add` (`:1205`)
- **Why:** this path supports a separate research UI or dashboard (tree view, metrics plots) without touching the TUI, and the harness stays upstream-pure.
- **Merge cost:**
  - None in Rust.
  - Depends on **experimental** protocol surfaces (`dynamicTools`, `environment/add` are `#[experimental]`), which can change between upstream releases. Pin versions and add a contract test.

### Not recommended now
- New core built-in tools (`core/src/tools/handlers/*` + `spec_plan.rs`), because they touch hot core files.
- A new `/research` TUI slash command, because it touches TUI hot files and snapshots.
- New `ExtensionItem` variants with app-server/TUI rendering, which is multi-crate protocol churn.

Do these only if R2 proves out and a native UI is really needed. Each means rebasing against hot files that upstream changes daily; the base commit is from 2026-09-30, the same day as the fork's commits.

**Suggested sequencing:** R1 first (days, zero risk, provider-agnostic), then R2 once the loop semantics are validated (the goal-extension pattern), with R3 only if an external research UI is on the roadmap.
