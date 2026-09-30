# F - Recording everything in ukis-code (rollouts, reasoning, bridge, hooks, extensions)

Repo: `/home/pavle/projekti/ai-tooling/ukis-code` (origin `UkisAI/ukis-code`, HEAD 77a4296720). This was a read-only study: nothing was built or modified.
Note: the prior report `D-ukis-code-arch.md` was not on disk (the scratchpad `study/` dir did not exist), so this report was built from the source alone.
All paths below are relative to `codex-rs/` unless they start with `scripts/`.

---

## 1. Rollout recording (what Codex already writes)

### Where
- `rollout/src/lib.rs:86-87`: `SESSIONS_SUBDIR = "sessions"`, `ARCHIVED_SESSIONS_SUBDIR = "archived_sessions"`.
- `rollout/src/recorder.rs:1721-1744`: the path is `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<local-ts>-<thread_id>[_<rollout_id>].jsonl`. `CODEX_HOME` defaults to `~/.codex` (`utils/home-dir/src/lib.rs:52-60`). The fork does not rename it.
- `rollout/src/compression.rs:26,335-336`: rollouts older than **7 days** are compressed to `.jsonl.zst` (zstd level 3, lossless) by a startup worker (`core/src/thread_manager.rs:477-495`). A consumer must read both `.jsonl` and `.jsonl.zst`.
- The writer is an async task (`recorder.rs:92-160`). The per-item filter runs at `thread-store/src/local/live_writer.rs:330` (`items.retain(is_persisted_rollout_item(..))`).
- The codex-state sqlite DB (`state/migrations/*`) holds only thread metadata and indexes (title, preview, model, cwd, history_mode...). It holds no content. The `logs` table was dropped (`0023_drop_logs.sql`).
- `~/.codex/history.jsonl` (message-history) holds only user prompts. `HistoryPersistence::{SaveAll,None}` at `config/src/types.rs:213-219` controls that file, not rollouts.
- `codex exec --ephemeral` (`exec/src/cli.rs:36`) disables rollout writing. Never use it for data collection.

### Format
- One JSON object per line: `RolloutLine { timestamp, ordinal?, #[flatten] item }` (`history/src/lib.rs:350-357`).
- `item` is tagged `{"type": <snake_case variant>, "payload": ...}` (`history/src/rollout_payload.rs:31-78`). The variants (`history/src/lib.rs:200-215`) are:
  `session_meta`, `response_item` (+ optional `metadata`), `inter_agent_communication[_metadata]`, `compacted`, `turn_context`, `token_usage_record`, `world_state`, `security_risk_score`, `retained_context`, `event_msg`, `realtime_item`.
- `session_meta` holds `cwd`, `cli_version`, `originator`, `source`, **`base_instructions`** (the system prompt), `dynamic_tools`, `history_mode`, and git info (`protocol/src/protocol.rs:3120-3170, 3236-3241`). It does not hold the built-in tool schemas. Those appear only in the raw request, which rollout-trace captures (see below).
- `turn_context` holds `turn_id`, `cwd`, `workspace_roots`, `current_date`, `timezone`, `approval_policy`, `sandbox_policy`, `permission_profile`, `network`, **`model`**, `personality`, `collaboration_mode`, **`effort`**, and `summary` (`protocol/src/protocol.rs:3301-3358`).

### Which items are persisted (`rollout/src/policy.rs`)
- Always persisted (`:17-24`): `SessionMeta`, `TurnContext`, `TokenUsageRecord`, `Compacted`, `WorldState`, `RetainedContext`, `SecurityRiskScore`, and inter-agent communication.
- `ResponseItem` (`:44-65`): **persists** `Message` (user, assistant and developer, including AGENTS.md and env-context fragments), `AgentMessage`, **`Reasoning`**, `LocalShellCall`, **`FunctionCall`**, **`FunctionCallOutput`**, `CustomToolCall`/`Output` (apply_patch), `ToolSearchCall`/`Output`, `WebSearchCall`, `ImageGenerationCall`, `ConfigurationUpdate`, `Compaction`, `ContextCompaction`. It **drops** `AdditionalTools`, `CompactionTrigger`, and `Other` (unknown item types, from `#[serde(other)]` at `protocol/src/models.rs:1254`).
- `EventMsg` (`:94-206`) is always persisted for `TokenCount`, `TurnStarted`, `TurnComplete`, `TurnAborted`, `ThreadRolledBack`, `ThreadGoalUpdated`, and `ThreadSettingsApplied`.
  - `ItemCompleted(TurnItem)` is persisted **in full in Paginated mode**. In Legacy mode only FunctionCallOutput, Plan, Sleep, and completed SubAgent items are kept (`:96-112`).
  - The legacy `UserMessage`, `AgentMessage`, `AgentReasoning`, `AgentReasoningRawContent`, `McpToolCallEnd`, `PatchApplyEnd` and similar events are persisted **only in Legacy mode** (`:123-135`).
  - All deltas (`ReasoningContentDelta`, `ReasoningRawContentDelta`, `AgentMessageContentDelta`), `ExecCommandBegin`/`End`, `ExecCommandOutputDelta`, `HookStarted`/`Completed`, `RawResponseItem`, `SessionConfigured`, and `Error`/`Warning` are **never** persisted (`:141-204`).
- History mode: `ThreadHistoryMode::{Legacy (serde default), Paginated}` at `protocol/src/protocol.rs:778-782`. In practice **Paginated is the default**: the app-server picks it whenever the store supports it (`app-server/src/request_processors/thread_processor.rs:1462-1465`), the TUI requests it (`tui/src/app_server_session.rs:238`), and exec uses it when not ephemeral (`exec/src/lib.rs:1380`). Paginated rollouts also carry `TurnItem`s: `UserMessage`, `AgentMessage`, `Reasoning{summary_text, raw_content}` (`protocol/src/items.rs:192-197`), `CommandExecution{command, cwd, stdout, stderr, aggregated_output, exit_code, duration}` (`:244-290`), `McpToolCall`, `FileChange`, and so on.
- There is **no "limited/extended" persistence knob**. The only switches are the history mode (API-level, not in config.toml), `ephemeral`, and `history.persistence` (prompt history only).

### Truncation
- History truncation does **not** touch the rollout. `core/src/context_manager/history.rs:495-509` says: *"Tool output truncation applies only to live history, preserving full rollout payloads."* `core/src/session/mod.rs:3486-3505` only stamps `metadata.history_truncation_token_limit` on outputs, then `persist_rollout_items` (`:3586-3588`, `:4465`) writes the untruncated items.
- Tool-level caps still apply **before** the output exists, so the rollout holds what the model saw:
  - unified exec uses a head/tail buffer of `UNIFIED_EXEC_OUTPUT_MAX_BYTES = 1 MiB` (`core/src/unified_exec/mod.rs:80`, `head_tail_buffer.rs`);
  - the shell tool uses `EXEC_OUTPUT_MAX_BYTES = DEFAULT_OUTPUT_BYTES_CAP = 1 MiB` (`core/src/exec.rs:81`, `utils/pty/src/lib.rs:25`);
  - there is also a per-call `max_output_tokens` argument.
  
  Raw stdout beyond those caps is not recorded anywhere. That is acceptable for training data, because the model-visible output is the right target.

### Reasoning items in the rollout
- `ResponseItem::Reasoning { id, summary: Vec<SummaryText>, content: Option<Vec<ReasoningText|Text>>, encrypted_content }` is defined at `protocol/src/models.rs:1048-1060` and `:1984-1993`.
- `content` is serialized only if it contains at least one `reasoning_text` entry (the misnamed `should_serialize_reasoning_content` at `models.rs:1624-1631` returns *skip* otherwise). So **raw reasoning text is persisted when the provider put it in the final item**. OpenAI hosted models give `summary` plus `encrypted_content` (opaque). Local vLLM-style servers give `content[reasoning_text]`, stored verbatim.

---

## 2. Reasoning from local/OSS models

- **The Chat Completions wire API is gone.** `WireApi` has only `Responses` (`model-provider-info/src/lib.rs:104-130`), and `wire_api = "chat"` is a hard error (`CHAT_WIRE_API_REMOVED_ERROR`). There is no `reasoning_content` or `reasoning` field parsing anywhere in codex-rs (grep finds nothing). A local server must speak **`/v1/responses`**. vLLM does, and so does Ollama's OpenAI layer. llama.cpp `llama-server` support for `/v1/responses` is **unverified**. Without it you need a shim (LiteLLM, or the Ukis bridge, see section 5).
- SSE parsing (`codex-api/src/sse/responses.rs`):
  - `response.output_item.done` becomes `OutputItemDone(ResponseItem)` (`:353-360`). **This final item is the one that gets persisted.**
  - `response.reasoning_text.delta` becomes `ResponseEvent::ReasoningContentDelta` (`:396-403`).
  - `response.reasoning_summary_text.delta` and `.done` are handled at `:377-394`.
  - Unknown `*.delta` events are ignored (`:480-497`).
- Core flow:
  - `core/src/session/turn.rs:2688` handles `OutputItemDone`, which goes to `handle_output_item_done`, then `record_completed_response_item` (`core/src/stream_events_utils.rs:79-100`), then `record_conversation_items`, then the rollout.
  - Reasoning deltas become the transient `EventMsg::ReasoningRawContentDelta` (`turn.rs:3129-3147`). They are streamed to the UI and **never persisted**.
  - `event_mapping.rs:201-226` maps the final item to `TurnItem::Reasoning.raw_content`, which Paginated mode persists.
  - `show_raw_agent_reasoning` (`core/src/config/mod.rs:681-683`) only gates the legacy UI events (`protocol/src/legacy_events.rs:163-175`), not persistence.
- **Result:** raw local reasoning is captured **if and only if** the server's `output_item.done` reasoning item contains `content:[{type:"reasoning_text",text}]` and a `summary` array.
- Gaps to close:
  1. `summary` has no `#[serde(default)]` (`models.rs:1052`). A server that omits it makes `ResponseItem` deserialization fail. The failure is logged at `debug!` and the **whole reasoning item is silently dropped** (`responses.rs:354-358`).
  2. A server that streams `reasoning_text.delta` but sends a done-item with empty or missing content loses the text, because deltas are not accumulated.
  3. A server that uses a non-standard shape (a `reasoning` field on the message, or `reasoning_content`) is ignored completely.
- Fix options:
  - **(a)** Ideally touch no core code: use a `ModelRequestContributor` interceptor (section 4) that accumulates `ReasoningContentDelta` and back-fills `content` on the `OutputItemDone(Reasoning)` before core sees it. Interceptors run **before** both the rollout-trace and the persistence path (`core/src/client.rs:2357-2368`, doc at `core/src/model_request.rs:1`).
  - **(b)** Alternatively, a one-line `#[serde(default)]` on `summary` plus about 15 lines in `client.rs` or `sse/responses.rs`. Both are upstream hot files, so this has a merge cost.
- **Bonus, already built in: rollout-trace.**
  - Set `CODEX_ROLLOUT_TRACE_ROOT=<dir>` (`rollout-trace/src/thread.rs:44,106-120`, started at `core/src/session/session.rs:1218-1228`; child agents share the root bundle).
  - It writes `manifest.json`, `trace.jsonl` and `payloads/*.json`:
    - the **exact request per inference**, including instructions, full tool schemas and input (`client.rs:1756-1764`);
    - response output items and usage (`client.rs:2420-2450`);
    - failed and cancelled partials;
    - tool invocation and result payloads (`core/src/tools/tool_dispatch_trace.rs`, `rollout-trace/src/raw_event.rs:69-175`);
    - code-mode cells, terminal operations, and compaction.
  - `trace_response_item_json` explicitly restores reasoning `content` (`rollout-trace/src/inference.rs:315-340`).
  - Reduce a bundle with `codex debug trace-reduce <bundle>` (`cli/src/main.rs:269,1740`).
  - Cost: the full request is written on **every** inference call, so disk use grows O(turns x context). Plan for compression and rotation.

---

## 3. Claude bridge (`scripts/providers/*.mjs`)

- `launch.mjs:65-84`: in unified mode **all** model traffic, OpenAI included, goes to the local bridge (`model_provider="ukis"`, `wire_api="responses"`).
- `bridge.mjs:14-70` reads the raw body (`:48-49`). `openai-forward.mjs:165-178` routes: OpenAI requests are byte-proxied (`:120-163`), and Claude requests go to `runClaudeTurn`. `bridge.mjs:31-34` `emit` is the single SSE writer. **That makes it a natural tap point for raw request plus raw SSE, for both providers.**
- Thinking blocks are **dropped**:
  - `claude-turn.mjs:188-232` creates Responses items only for `text` and `tool_use` content blocks. A `thinking` or `redacted_thinking` block gets `item = null`, so nothing is emitted.
  - `:233-249` handles only `text_delta` and `input_json_delta`, so `thinking_delta` and `signature_delta` are ignored.
  - `:282-291` takes only usage from the `result` message.
  - The full `assistant` SDK messages, which also carry the thinking blocks, are ignored.
  - `:116` also strips any `reasoning` items from the history sent to Claude.
- No `thinking` option is passed (`:155-181`), so the SDK default applies (adaptive thinking). Whether the text is summarized or omitted depends on the model/SDK default (**unverified**).
- SDK 0.3.x exposes `thinking: {type:'adaptive', display:'summarized'|'omitted'}` (checked in the local sdk.d.ts of v0.3.263; `package.json` pins 0.3.285). So the bridge can request `display:'summarized'`. Claude never exposes raw chain-of-thought, only summarized thinking.
- The bridge can log everything itself: it is plain Node, owned by Ukis, with no upstream merge risk. It could add:
  - (i) a JSONL tee in `bridge.mjs` (request body plus every emitted or forwarded SSE event, per `x-ukis-session`);
  - (ii) translation of thinking into Responses reasoning events and items, so Codex persists them natively.

---

## 4. Hooks vs extension API

### Codex hooks (external processes, configured in `hooks.json`/config)
- Events (`hooks/src/schema.rs:102-122`): PreToolUse, PermissionRequest, **PostToolUse**, Pre/PostCompact, SessionStart, **UserPromptSubmit**, Subagent Start/Stop, and **Stop** (which carries `last_assistant_message`, `:600`).
- PostToolUse input (`schema.rs:320-340`): `session_id`, `turn_id`, **`transcript_path`** (the rollout file), `cwd`, `model`, `permission_mode`, `tool_name`, **`tool_input`**, **`tool_response`**, `tool_use_id`.
- Coverage: the default applies only to `ToolPayload::Function` tools, using the model-facing output (`core/src/tools/registry.rs:99-125`). Overrides exist for apply_patch (`handlers/apply_patch.rs:437`), MCP (`handlers/mcp.rs:478`, full `CallToolResult`), exec_command (`unified_exec/exec_command.rs:553`), write_stdin (`:136`) and code-mode wait. Custom or freeform tools without an override **do not fire PostToolUse**.
- Hooks give **no reasoning, no model request, and no streamed model output**. They also cost one process spawn per event.
- Verdict: hooks are good for a zero-code live sidecar (the recorder reads `transcript_path`), but they do not add anything that is not already in the rollout.

### Extension API (`ext/extension-api`, in-process Rust; `ext/research` does not exist yet)
- **`ModelRequestContributor` / `ModelResponseInterceptor`** (`src/model_request.rs:19-44`):
  - It is created per request with `{kind, thread_id, model, client_metadata}`. The **request body is not exposed**.
  - It wraps the full `ResponseEvent` stream (`OutputItemAdded`/`Done`, `ReasoningContentDelta`, `ReasoningSummaryDelta`, `Completed{token_usage}`), and it **may transform it**.
  - It is applied at `core/src/client.rs:1749-1780, 2036-2110, 2357-2368`, before trace and persistence.
- **`TurnLifecycleContributor`** (`src/contributors.rs:216-270`):
  - `on_turn_start` receives turn_id, collaboration mode and token usage.
  - **`on_item_completed(&TurnItem)`** is called for every completed item (`core/src/session/mod.rs:2529-2541`): UserMessage, AgentMessage, Reasoning(raw_content), CommandExecution(output, exit), McpToolCall, FileChange, and so on.
  - It also has `on_turn_stop`, `on_turn_abort` and `on_turn_error`.
- **`ToolLifecycleContributor`** (`src/contributors/tool_lifecycle.rs`):
  - `ToolStartInput.payload` holds the finalized arguments (`:117-148`).
  - `ToolFinishInput` has **only the outcome, no output** (`:190-207`).
  - `McpToolResultInput` has the MCP result (`:173-188`).
  - Timing and dispatch observers are also available.
- `ThreadLifecycleContributor` (`contributors.rs:164-210`) covers thread start, resume, idle and stop.
- Registration: one line in `app-server/src/extensions.rs:50-121` (`thread_extensions`), which serves TUI, exec and app-server because they all go through the in-process app-server. Builder methods are at `registry.rs:88,128,138`.

### Which is most complete with the least diff

| Source | Prompts | Assistant text | Tool call + output | Raw local reasoning | Claude thinking | Exact request (system prompt, tool schemas) | Upstream diff |
|---|---|---|---|---|---|---|---|
| Rollout JSONL (Paginated) | yes | yes | yes (model-visible) | yes, if the done-item has content | no (bridge drops it) | system prompt only (session_meta), no built-in tool schemas | 0 |
| rollout-trace (`CODEX_ROLLOUT_TRACE_ROOT`) | yes | yes | yes, plus runtime payloads | same condition | no | **yes, per inference** | 0 (env var) |
| Hooks | yes (UserPromptSubmit) | last message only | mostly (Function, MCP, exec, patch) | no | no | no | 0 (config) |
| ext/research | yes | yes | yes (via TurnItems) | **yes, even from deltas** | no | no body | new crate plus about 3 lines |
| Bridge tee | via body | yes | calls yes, outputs via next body | n/a (not on the local path) | **yes, if translated or logged** | **yes** | 0 (Ukis-owned JS) |

**The most complete trace with the least diff is rollouts plus rollout-trace. Both are already built in and need zero Rust changes.**

---

## 5. Recommended minimal design

1. **Turn on what exists. Merge cost 0 (launcher only).**
   - In `scripts/ukis-code.mjs` (spawn env, about `:63`) or `launch.mjs:105`, set `CODEX_ROLLOUT_TRACE_ROOT=~/.ukis-traces` (opt-in via `UKIS_RECORD=1`).
   - Never pass `--ephemeral`.
   - Keep Paginated mode, which is already the default.
   - Result: prompts, assistant messages, tool calls and outputs, token usage, turn_context (model/cwd/sandbox/effort), and the exact request payloads with tool schemas are all recorded, plus raw reasoning from spec-compliant local servers.
2. **Claude thinking in the bridge. Merge cost 0 (Ukis-owned JS, about 60 lines plus tests).**
   - In `claude-turn.mjs`, pass `thinking:{type:'adaptive', display:'summarized'}` (or a configurable value).
   - Map the `thinking` content_block to `output_item.added` with `{type:"reasoning", id:"rs_ukis_…", summary:[], encrypted_content:null}`, map `thinking_delta` to `response.reasoning_summary_text.delta`, and on stop send `output_item.done` with `summary:[{type:"summary_text",text}]`.
   - Codex then persists it as a normal reasoning item in both the rollout and the trace.
   - **Required companion change:** `openai-forward.mjs` must strip `rs_ukis_*` reasoning items from `input` before forwarding. Otherwise OpenAI rejects the unknown reasoning ids after a Claude-to-OpenAI model switch. Claude-side history already drops reasoning (`claude-turn.mjs:116`).
   - Optionally, add a `UKIS_BRIDGE_LOG=<dir>` JSONL tee in `bridge.mjs` (request body plus emitted SSE) as a provider-exact raw log.
3. **Robust local reasoning. Choose one:**
   - **(a, preferred) New crate `ext/research`.** It holds a `ModelRequestContributor` whose interceptor accumulates `ReasoningContentDelta` per item and back-fills an empty `content` on `OutputItemDone(Reasoning)`. It can also add a `TurnLifecycleContributor` that writes a compact per-turn trace (`on_item_completed`) if you want a custom schema.
     - Merge cost: a new directory (conflict-free), plus about 1 line in `app-server/src/extensions.rs`, 1 dependency in `app-server/Cargo.toml`, 1 workspace member in `codex-rs/Cargo.toml`, and a Bazel `BUILD.bazel` entry. That is about 4 trivial upstream-file touches.
     - It cannot fix a missing `summary` field, because parsing fails before the interceptor sees the item.
   - **(b) Or patch core directly:**
     - add `#[serde(default)]` on `summary` (`protocol/src/models.rs:1052`), 1 line;
     - add delta back-fill in `codex-api/src/sse/responses.rs` or `core/src/client.rs`, about 15 lines;
     - handle non-standard `reasoning`/`reasoning_content` fields, about 20 lines in the SSE parser.
     
     These are hot upstream files, so expect a small but recurring conflict risk. Do this only after checking that the actual vLLM, llama.cpp or Ollama build misbehaves.
   - **(c) If llama.cpp has no /v1/responses**, add a "local" route in the Ukis bridge that translates chat-completions (`reasoning_content`) into Responses events (`reasoning_text`). It is JS-only with zero Rust diff, and it doubles as the capture point.
4. **Offline exporter** (Python, outside the repo). It walks `sessions/**/*.jsonl[.zst]` and the trace bundles and joins them by thread_id, turn_id and call_id into SFT or trajectory records.
   - Use the trace request payload as the ground truth for the system prompt and tools.
   - Use the rollout for turn structure.
   - Treat `encrypted_content` as opaque (OpenAI) and `content[reasoning_text]` as raw CoT (local models).
   - Mark Claude reasoning as "summarized".
5. **Skip** hooks for recording (they are redundant and less complete) and OTel (`otel.log_user_prompt` exists at `config/src/types.rs:605`, but it has no model outputs or reasoning).

Verification still pending, all **unverified**:
- whether vLLM, Ollama and llama.cpp `output_item.done` reasoning items include `summary` and `content`;
- the default thinking display for the current Claude models through the SDK;
- rollout-trace disk growth on long sessions.
