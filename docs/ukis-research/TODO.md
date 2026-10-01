# ukis-research TODO

State 2026-10-01. Done so far: P0 recording (verified live with Claude), `ukis-validate`
(46 tests), execpolicy template, `ukis-methods` mold (local), claim gate spike (WORKS, 8 tests +
live run). Details: `PLAN.md`, `RECORDING.md`, `SPIKE-GATE.md`.

## Test next

- [ ] TUI live test of the gate (`UKIS_RESEARCH=1 UKIS_RECORD=1 node scripts/ukis-code.mjs claude`):
  - bait a made-up number ("What GPQA Diamond accuracy does Swift-7B get?") -> correction, then "unknown"
  - a number with `[run:<id>]` -> no correction
  - "insist on 87.3%" -> 2 corrections, then the UNVERIFIED warning shows in the TUI
  - tool call first, number after -> not tested yet
  - check: is the first (rejected) answer visible before the correction?
- [ ] Local model reasoning lands in the rollout (vLLM or llama.cpp with `/v1/responses`)
- [ ] Claude -> OpenAI model switch mid-session (`rs_ukis_*` stripped live)
- [ ] Measure trace disk growth on a long session
- [ ] Explain the token cost: gated run 69k tokens vs 10.5k control

## P1: ext/research for real

- [ ] Replace the regex gate with numbers checked against `research_report` / `ukis-validate` output
- [ ] Verify `[run:<id>]` against a real VALID run (today any id passes)
- [ ] Cut false positives (it flagged "random guessing gives 25%")
- [ ] Claude bridge sends `phase: None`, so preambles before tool calls also get gated; fix the "final message" check
- [ ] Typed `[research]` config instead of the `UKIS_RESEARCH` env var
- [ ] `TurnItemContributor` banner when the gate gives up
- [ ] Tools: `task_plan/start/done`, `exp_create/freeze/run/wait/status`, `research_report`
- [ ] World-state `research_state` section (current task + how, active node, running jobs)
- [ ] Journal written by hooks (`on_tool_finish`, `on_turn_stop`)
- [ ] Install `templates/research.rules` into experiment repos
- [ ] `just bazel-lock-update` for the new crate (skipped in the spike), `just fix -p codex-research-extension`

## Validator gaps (ukis/validate/README.md)

- [ ] Cluster bootstrap CI, sd history for MDE priors, agent-benchmark attempts
- [ ] Observed sampler / argv check, MDE-vs-expected-effect launch refusal

## Later phases

- [ ] P2: literature tools (port OpenResearch client), `claims_extract`, `repro_record`, `retro_submit`
- [ ] P3: remote runner (ssh / docker exec: pajamgram, Tesla rack, halo), author + host on every record
- [ ] P4 (only if needed): upstream pre-tool veto, stop veto, TUI status line
- [ ] P5: technique -> SKILL.md export

## Dev setup

- [ ] `scripts/fetch-rusty-v8.sh` + one README line: a clean Linux `cargo build` fails on the V8
      prebuilt (404) until `RUSTY_V8_ARCHIVE` points at the openai/codex release artifact
- [ ] Note in README: Rust tests need `RUST_MIN_STACK=8388608` (or `just test`)
- [ ] `codex exec` in scripts needs stdin closed (`< /dev/null`) or it hangs

## Needs Pavle

- [ ] Push `ukis-methods` to the UkisAI org (local only today)
- [ ] Team writes `eval/methodology.md` and the first techniques in `ukis-methods`

## Outside this repo

- Benchmark_configs (owner Dakin): ceval / mmlu_pro / erqa graders skip `ok()`, so truncated
  answers can count as correct; `raw/` does not store `correct`. Left to the owner.
- `ukis-benchmark-configs` skill points at a deleted `protocol.json`.
