# ukis-research: implementation plan

Status: draft for Pavle's review, 2026-09-30. Requirements 1-9: `SYNTHESIS.md`.
Evidence (file:line): `study/A..G-*.md`.

Principle: **the model proposes, code decides.** Every number, verdict, journal fact and commit in
the research system is produced by deterministic code. The model supplies intent (hypothesis,
"how", narrative) and nothing it says can become a result by itself.

## Architecture

```
 Codex agent loop (core/, untouched)
   │ extension API: tools · world-state context · turn/tool/thread lifecycle
   ▼
 ext/research  (new crate, ~4 one-line upstream registrations)
   ├── tasks      paper/plan -> tasks.json, exactly one in_progress + "how", pinned every step
   ├── tree       experiment node = git branch/worktree, frozen by code (sha + tree hash)
   ├── runner     detached run per seed (local or `ssh host`), meta.json, exit code, timeout
   ├── validator  contract checks, recompute from items.jsonl, MDE/CI/McNemar -> verdict
   ├── gate       research_report = only path from number to result; prose number check
   ├── journal    written by hooks (tool end, turn end: diff stat, commits), never by the model
   └── methods    ukis-methods repo: techniques/<slug>/..., deterministic commits
 recording: CODEX_ROLLOUT_TRACE_ROOT (upstream) + Claude bridge thinking passthrough
```

Two stores:
- `<code repo>/.ukis/research/` : tree.json, tasks.json, contracts/, runs/<id>/{meta,items,results},
  reports/, journal.jsonl. Committed with the experiment it measured.
- `ukis-methods` repo: eval/methodology.md + techniques/<slug>/{paper, claims.json,
  reproductions/, methodology.md, README.md}. Shared by the team.

Results contract (metric-agnostic, from `study/E`): `contract.json` (argv, tier, family,
dataset+revision, seeds, metrics {name, unit, direction}, baseline, controls, MDE inputs; hashed
at first run) -> run emits `items.jsonl` ({arm, item?, seed, value|correct, status, tokens?})
directly or via a thin adapter -> Rust computes `results.json` and `comparison.json`
(SUPPORTED / REFUTED / UNRESOLVED / NOT_COMPARABLE). Runner's own summary must agree or the run
is INVALID.

## Phases

Each phase ends with a verify gate; no phase starts before the previous one passes.

### P0: foundation, zero Rust diff
- Launcher: `UKIS_RECORD=1` sets `CODEX_ROLLOUT_TRACE_ROOT`; never `--ephemeral`.
- Claude bridge (`scripts/providers/claude-turn.mjs`, ~60 lines): request summarized thinking,
  emit reasoning items `rs_ukis_*`, strip them in `openai-forward.mjs` on provider switch.
- `.codex/rules/research.rules` (execpolicy): forbid `git commit --amend`, `push --force`,
  `rebase` inside experiment worktrees.
- `ukis-methods` local repo: the mold only (layout, technique template, results contract,
  `new-technique.sh`). No technique content and no authored methodology; the team fills those.
  Done 2026-10-01. Execpolicy rules shipped as a template (`templates/research.rules`).
- **Verify:** one Claude and one local-model session each leave a trace with reasoning on disk
  (local server reasoning format is UNVERIFIED today, this step tests it).

### P1: ext/research MVP, local runs
- Crate modeled on `ext/goal` + `ext/web-search`; install in `app-server/src/extensions.rs`
  behind a feature flag.
- Tools: `task_plan/start/done`, `exp_create/freeze/run/wait/status`, `research_report`.
- World-state section `research_state` (current task + how, active node, running jobs, last
  validated numbers, open gate violations). Re-rendered every step, survives compaction.
- Journal from `on_tool_finish` / `on_turn_stop`. Built-in policy via `include_str!` + system skill.
- Validator as a pure module with unit tests (contract refusal cases, recompute, verdicts).
- **Verify:** end to end on a small seeded benchmark (Benchmark_configs `--limit` against a local
  server, or a synthetic seeded script): the gate REJECTS an invented number and a 3-seed "win",
  ACCEPTS a correct 5-seed report, and the canonical table renders only from VALID files.

### P2: literature and methods repo
- Port OpenResearch lit client (MIT, keep copyright; replace contact emails): alphaXiv search +
  full text, OpenAlex, PubMed. Tools `lit_search`, `paper_read`.
- `technique_open`, `claims_extract` (quote must be verbatim in the full text, value must be in
  the quote), `repro_record`, `retro_submit`. Extension commits with fixed argv + templated
  messages; changes to `eval/methodology.md` need Pavle's approval.
- **Verify:** a reasoning-shortening paper -> claims.json -> one reproduction recorded with verdict.

### P3: remote compute and team
- Runner over ssh / docker exec (pajamgram, Tesla rack, halo): detached supervisor, one lock per
  run, cancel by flag, timeouts, author + host on every record.
- `ukis-methods` pushed to the UkisAI org (needs Pavle's explicit go).
- **Verify:** a 5-seed run on a GPU box survives a laptop disconnect and reattaches.

### P4: small upstream hooks (optional, only if P1 shows the need)
- Pre-tool veto on `ToolLifecycleContributor` (~60 lines), stop veto (~50 lines), generic
  extension status line in the TUI. Candidate upstream PRs so we don't carry them.

### P5: technique -> skill
- Export a technique with VALID reproductions + stable methodology.md as a SKILL.md.

## Known limits (honest)
- Extensions cannot block tool calls today; P1 relies on detection (dirty tree / HEAD != frozen
  sha / tree hash) + execpolicy, P4 adds a real veto.
- "You're absolutely right" capitulation without numbers is only pattern-detectable; the gate can
  demand an evidence reference, it cannot judge argument quality.
- vLLM seeds are not bitwise reproducible: 5 seeds = 5 independent repeats.
- First MDE per benchmark family is "unknown" until sd history exists.
- `ukis-benchmark-configs` skill points at a deleted `protocol.json` (fix separately).
