# Spike: final-answer claim gate (2026-10-01)

Question: can an in-process Codex extension stop a turn that ends with an unbacked number, by
injecting a correction so the same turn continues? **Verdict: WORKS** (no core changes).

## What was built
- `codex-rs/ext/research` (crate `codex-research-extension`): one `TurnLifecycleContributor`.
  Installed in `app-server/src/extensions.rs` only when `UKIS_RESEARCH=1` (covers TUI + exec).
- Rule (`claim_gate.rs`): a sentence with a percentage, or a metric word (accuracy, score, F1,
  ...) plus a decimal number, must carry `[run:<id>]` in the same sentence. Otherwise inject a
  correction listing the unbacked sentences (max 5, 160 chars each). Max 2 corrections per turn,
  then `tracing::warn!` + `ExtensionEventSink::emit_warning` ("gave up ... UNVERIFIED").
- Test support: `TestCodexBuilder::with_extension_installer` (core_test_support, ~20 lines) so a
  test can install an extension that needs `Weak<ThreadManager>`.

## Mechanism (first try worked, no fallback needed)
`on_item_completed` (final `AgentMessage`, phase != Commentary) -> `ThreadManager::get_thread`
-> `CodexThread::inject_if_running(vec![InternalModelContextFragment("research_gate")])`.
Why it continues: core awaits `on_item_completed` inline in `handle_output_item_done`
(`stream_events_utils.rs:385`), before the turn loop computes
`needs_follow_up = model_needs_follow_up || has_pending_input` (`turn.rs:556-566`). The injected
item sits in pending input, so the loop drains it, records it after the assistant message and
samples again. Same turn id, one `task_complete`. `start_turn_if_idle` was not needed.
Lock ordering: no core lock is held across `on_item_completed`; `inject_if_running` takes
`active_turn` then the turn-state lock, same order as `has_pending_input`. The injected item is a
`ResponseItem`, not `UserInput`, so the mid-sampling preempt watcher
(`input_queue.rs:246-265`) does not cancel the stream. Verified by tests (no hang).

## Evidence
Tests (`cargo test -p codex-research-extension`): 5 unit + 3 integration, all pass.
- `unbacked_final_claim_continues_turn_with_correction`: mocked SSE, answer 1
  "Swift-7B scores 87.3% on GPQA Diamond." -> exactly 2 `/responses` requests in ONE submitted
  turn; request 2 has the assistant answer followed by the user-role correction.
- `backed_final_claim_ends_turn`: `[run:...]` answer -> 1 request.
- `gate_gives_up_visibly_after_two_corrections`: 3 unbacked answers -> 3 requests, 1 warning.

Live, release build, Claude bridge (sonnet), `UKIS_RESEARCH=1 UKIS_RECORD=1`, rollout
`~/.codex/sessions/2026/10/01/rollout-2026-10-01T11-46-46-01a0f6db-...jsonl`:
- line 12, assistant (phase None): "...usually land around 28-35% on GPQA Diamond ... I'd say
  about 31%. That is a guess about the model class, not a measurement of Swift-7B."
- line 15, user: `<codex_internal_context source="research_gate">` Research gate: ... "If I had to
  give one number for a general-purpose 7B, I'd say about 31%" ... cite [run:<id>] or say unknown.
- line 17, assistant: "I don't know Swift-7B's GPQA Diamond accuracy. I have no validated run for
  it, so I can't give you a percentage. ... I'm withdrawing them."
- line 20: single `task_complete` for turn `01a0f6db-f8ee-...`. Log: `research gate injected a
  correction ... attempt=1`.
- Control (`UKIS_RESEARCH=0`, rollout `...01a0f6dc-7fd2-...`): answer with "25%", "high 20s to
  the 40s" ends the turn, no gate.

## Gotchas
- **The rejected text is visible.** `codex exec` printed both `codex` blocks (first with 31%,
  then the corrected one); only the final summary line shows the corrected answer. TUI: not run
  live, but the first message is already streamed and its `ItemCompleted` is sent right after the
  hook, so it will show too (UNVERIFIED live). The gate controls what the turn ends with, not
  what the user saw on the way.
- Claude bridge sends `phase: None`, so every assistant message counts as final. A mid-turn
  preamble with a number followed by tool calls also gets corrected (one step later, spends the
  cap).
- Pattern rule is crude: it flagged "random guessing gives 25%" (a true fact). P1 must check
  numbers against validated reports, not regexes alone.
- Tokens: gated run 68,956 vs control 10,526 (bridge resends the full transcript per sample;
  ratio not explained, measure in P1).
- `[run:<id>]` is not validated here: any id passes. P1 must resolve it against the ledger.

## What P1 should do
- Keep this mechanism; put the gate behind a typed `[research]` config / feature flag instead
  of an env var. Replace regex-only rule with "every number must match a value in a validated
  `research_report` of this thread (or a quoted paper claim id)".
- Add a `TurnItemContributor` banner for the given-up case; consider holding final text until
  the gate passes only if the upstream hook below lands.
- Run `just bazel-lock-update` (new crate + Cargo.lock change, skipped in the spike), `just fix
  -p codex-research-extension`, and the app-server test suite.

## Upstream changes
None required. Nice to have: `on_turn_stop_request` (G report sec. 6, ~50 LOC) for an exact
"after final message" hook that reuses `stop_hook_active`, and a host `ResponseItemInjector`
capability so extensions need no `Weak<ThreadManager>`.
