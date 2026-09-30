# Recording (P0)

Requirement 5 in `SYNTHESIS.md`, evidence in `study/F-recording.md`. No Rust changes.

## What is recorded where

| Store | On | Contents |
|---|---|---|
| Rollout `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` (`.jsonl.zst` after 7 days) | always (upstream) | user prompts, assistant messages, tool calls with args and model-visible output (tools cap output at 1 MiB), `turn_context` (model, effort, cwd, sandbox), token usage, reasoning items |
| Trace bundle `~/.ukis/traces/trace-<uuid>-<thread>/` (`manifest.json`, `trace.jsonl`, `payloads/`) | `UKIS_RECORD=1` | the exact request of every inference (instructions, full tool schemas, input), response items, raw tool payloads. Reduce with `codex debug trace-reduce <bundle>` |

Reasoning by provider:
- OpenAI hosted: `summary` plus opaque `encrypted_content`.
- Claude (bridge): summarized thinking only; Anthropic never exposes raw chain of thought. The bridge asks the SDK for `thinking: {type: "adaptive", display: "summarized"}` and turns each non-empty thinking block into a `reasoning` item with id `rs_ukis_*`, `summary` filled and no `encrypted_content`, so Codex stores it like any other reasoning.
- Local `/v1/responses` servers: raw text is stored when the final `output_item.done` reasoning item carries `content: [{type: "reasoning_text"}]` and a `summary` array.

## On / off

- `UKIS_RECORD=1 ukis ...` turns trace bundles on (off by default because of disk growth). A non-empty `CODEX_ROLLOUT_TRACE_ROOT` you set yourself wins over `~/.ukis/traces`.
- Rollouts are always written. Do not pass `--ephemeral` (it skips the rollout; the launcher warns when `UKIS_RECORD=1`).
- `UKIS_CLAUDE_THINKING_DISPLAY=summarized` (default), `omitted` (no thinking text), or `default` (do not pass `thinking`, SDK decides).

## Provider switches

- Claude -> OpenAI: `openai-forward.mjs` removes `rs_ukis_*` items from the outgoing `input` (OpenAI never issued those ids and would reject them). The body is rewritten only when something was removed.
- OpenAI -> Claude: `claude-turn.mjs` drops every `reasoning` item from the transcript it sends.

## Known gaps

- Local-model raw reasoning is UNVERIFIED: a done-item without `summary` fails to parse and the whole item is dropped silently; text sent only as `reasoning_text.delta` is not accumulated; `reasoning_content` style fields are ignored. Test with the real vLLM / llama.cpp / Ollama build.
- Trace disk growth: the full request is written on every inference, O(turns x context), with no rotation or compression. Not measured yet.
- `--thinking adaptive` is now always sent for Claude. Behavior on models without adaptive thinking (Haiku) is UNVERIFIED; use `UKIS_CLAUDE_THINKING_DISPLAY=default` if a model misbehaves.
- `ukis --provider openai` bypasses the bridge, so resuming a session that holds `rs_ukis_*` items there is not stripped (likely rejected by OpenAI). Resume through plain `ukis`.
- Not recorded: thinking signatures, `redacted_thinking` blocks, empty (omitted) thinking.
- Verified by node tests only (mapping, event order, stripping, launcher env). No real session has been run; that Codex parses the emitted item is checked by reading `protocol/src/models.rs` and `codex-api/src/sse/responses.rs`, not by a build.
