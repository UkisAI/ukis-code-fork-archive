# E - Evaluation methodology contract for the ukis-code research extension

Scope (per revised brief): a benchmark-agnostic methodology the harness must enforce natively.
Principle carried through every section: **the agent never reports a number. Only files written
by the launched command count, and a deterministic validator decides what may be claimed.**

Read-only study, 2026-09-30. Nothing was run, modified, or ssh'd.

---

## 0. Correction on "the CLI is not on this machine"

The benchmark runner source IS on disk, at:

`/home/pavle/projekti/ai-tooling/ukis-ai/Benchmark_configs/` (github.com/UkisAI/Benchmark_configs,
HEAD `8f748ee` "Initial release", 2026-09-25, author FireeF)

It is not a single binary. It is one Python script per benchmark plus a shared `common.py`:

```
bash benchmarks/setup.sh                 # once, in a venv; clones graders into benchmarks/upstream at pins.env revisions
python benchmarks/gpqa.py                # model served at 127.0.0.1:8000/v1, auto-detects the single served model
python benchmarks/gpqa.py --limit 2 --seeds 0   # smoke
```

Flags (all single-turn scripts): `--api-base` (env API_BASE), `--model` (env MODEL), `--api openai|anthropic`,
`--seeds 0,1,2,3,4` (default), `--max-tokens`, `--timeout` (default 10800 s), `--limit N`, `--conc`, `--out`
(default `benchmarks/runs/<bench>-<model>`). Key from env `API_KEY`. Agent benches (`tb21.py --variant 5h|native`,
`deepswe.py`) take only `--model --api-base --conc --out`.

Output per run dir: `status.json` (live done/total/errors/per_min/eta_min/at), `raw/<key>.json` (one file per
item x seed; key like `s0_017`), `score_<bench>.json`. Actual schema, from `common.report()`:

```json
{"benchmark": "gpqa", "model": "...", "mean": 88.58, "per_seed": {"0": 89.39, "1": 87.37, ...},
 "n": 990, "errors": 0, "truncated": 5, "mean_output_tokens": 8895.4, "median_output_tokens": 3003.0}
```

Hard-coded sampler (`common.py`): temperature 1.0, top_p 0.95, top_k 20, min_p 0, presence 0, repetition 1.0,
`chat_template_kwargs={"enable_thinking": true, "reasoning_effort": "xhigh"}`, per-request `seed=<seed>`.
Errors, timeouts and `finish_reason=length` count as wrong and stay in the denominator. Rerun resumes and retries
request errors only. IFBench writes two files (`score_ifbench_strict.json`, `score_ifbench_loose.json`).

Gaps in this runner that the harness must cover itself (they matter for Section 3):
- no dataset/grader revision is written into the score file (pins live only in git-ignored `pins.env`);
- no sampler, cap, seed list or runner commit in the score file; `per_seed` is keyed by seed but the file
  does not say which seeds were *requested*;
- no std / CI, no min/max; mean is mean of per-seed accuracies;
- `mean_output_tokens` = completion_tokens (thinking + answer), not a retokenized thinking count;
- no run/start timestamp, no host/GPU/vLLM version.

Also note: the installed skill `~/.claude/skills/ukis-benchmark-configs/SKILL.md` is **stale**. It points at
`protocol.json` and `benchmarks/<id>.json`, which the 2026-09-25 history rewrite deleted. The pinned revisions
from that older catalog still exist locally in orphaned commit `7c08475` (`git show 7c08475:PROVENANCE.md`),
e.g. GPQA dataset `633f5ee8...`, LCB dataset `0fe84c39...` + 1,055-question SHA256 `b5baada8...`, MMLU-Pro
`b189ec76...`, IFBench harness `1091c4c3...`. Not relevant to the generic contract, but the harness should
not read that skill as truth.

---

## 1. Sources read

| Source | Path |
|---|---|
| `experiment` skill | `/home/pavle/.claude/skills/experiment/SKILL.md` |
| `ukis-benchmark-configs` skill (wrapper, stale) | `/home/pavle/.claude/skills/ukis-benchmark-configs/SKILL.md` |
| `posttraining-reporting` skill | `/home/pavle/projekti/ai-tooling/ukis-ai/posttraining-skills/posttraining-reporting/SKILL.md` |
| `qwen38-rack-posttraining` skill + refs | `/home/pavle/projekti/ai-tooling/ukis-ai/posttraining-skills/{SKILL.md,references/run-contract.md,references/config-registry.md}` |
| Current repo skill `ukisai-benchmarks` | `/home/pavle/projekti/ai-tooling/ukis-ai/Benchmark_configs/skills/ukisai-benchmarks/SKILL.md` |
| Measurement rules (18 rules, paid-for) | `/home/pavle/projekti/ai-tooling/ukis-ai/swift-pipeline/METHODOLOGY.md` |
| Ship gate with exact McNemar | `/home/pavle/projekti/ai-tooling/ukis-ai/swift-pipeline/verify/verify_model.py` |
| Quant gate thresholds (versioned) | `/home/pavle/projekti/ai-tooling/ukis-ai/Quant_configs/gates/{gates.json,README.md}` |
| Memory | `/home/pavle/.claude/projects/-home-pavle/memory/{reference-ukis-posttraining-skills.md,feedback-honest-empty-over-invented-metric.md}` |
| Real result examples | `/home/pavle/projekti/ai-tooling/ukis-ai/tesla1-docs/data/ft/runs/*/` (score.txt, *_aggregate.json, scores/*_SUMMARY.json) |

---

## 2. The rules, extracted (source in brackets)

### Before launch
1. **Preregister five lines** before compute: QUESTION, METRIC (exact number + where it comes from),
   DECISION (both branches, thresholds named now), FALSIFY, CONTROLS (and what each rules out). [experiment R1]
2. **Compute the MDE first.** Paired: `MDE = 2.8 * sd_diff / sqrt(n_seeds)`; unpaired: `n = 16 * sd^2 / delta^2`
   per arm (2.8 = z0.975 + z0.80). If the expected effect < MDE, do not run; enlarge the change or cut variance. [experiment R2]
3. **Named profile, never reconstructed.** Sampler, cap, effort, dataset + revision, question count, scorer come
   from a registry file, not memory. Registry vs old script disagreeing = stop and report. [rack SKILL, bench skill]
4. **Pin every upstream revision** (dataset, grader commit, harness version). Empty/floating = smoke only,
   never a reported number. [Benchmark_configs README, pins.example.env]
5. **Freeze question IDs and hashes before generation;** same IDs and seeds for every arm. [old README rule 2]
6. **Context budget check:** prompt + max_tokens must fit server context (250k cap in 262,144 ctx is tight). [old README rule 3]
7. **Positive control** before trusting any intervention (adapter, plugin, logit bias): prove it changes output;
   `/v1/models` listing it or `/health` 200 is not evidence. [run-contract, METHODOLOGY #9, #15]
8. **Validate the baseline against the published number** before running arms; expect a model-specific offset. [METHODOLOGY #7]
9. **Dated run dir created first**, holding literal server command, client command, load audit, raw, log, score. [run-contract]

### Ladder
10. Smoke = "ran / did not run", never a number. Pilot = 2-3 seeds, directional only, kill if flat/wrong.
    Confirmation = full benchmark, >= 5 seeds, all controls. Final = everything frozen, held-out touched once,
    report mean/min/max/n. Never skip a tier. [experiment R3]
11. **5 seeds minimum for any claim.** Standard profile is seeds 0-4. [experiment, Benchmark_configs]

### Controls and comparability
12. Baseline always; plus zero / shuffle / wrong / held-out controls where they rule out a rival explanation.
    Run the boring control even when you "know" the answer. Tune the baseline as hard as the treatment;
    compare at matched compute. [experiment R4]
13. **One variable changed.** Distinguish result-changing changes (dtype, library, seed, numerics, cap) from
    speed-only (conc, workers). Only speed-only changes allowed mid-run. [experiment R5, bench skill "improve"]
14. **Any deviation in seeds, cap, effort, sampler, timeout, scorer, question scope = a NEW family.** Label it;
    never merge into the profile's comparisons. (32k vs 100k LCB; 175 vs 1,055 LCB; TB2.1 5h vs native;
    AIME 4-trial dev vs 5-seed.) [bench skills, PROVENANCE.md]
15. **Pair on (item_id, seed/sample_index);** report `n_pairs` beside every delta. [METHODOLOGY #2]
16. **Never compare a partial arm to a complete one** (rows land shortest-first, partial arms are biased). [METHODOLOGY #1]

### Scoring honesty
17. Errors, empty answers, timeouts, truncations count WRONG and stay in the denominator. A rerun retries request
    errors, never wrong answers. [all]
18. Report accuracy twice when a cap binds: headline (truncations wrong) and neither-arm-truncated intersection. [METHODOLOGY #3]
19. Completeness counts rows carrying a generation, not lines; match the error key exactly. Count distinct n. [METHODOLOGY #4, experiment R8]
20. Score the visible answer only (IFBench), never the thinking. [bench skill]
21. Name units: `reasoning_tokens` != `completion_tokens`; say which one the thinking column uses. [METHODOLOGY #11]
22. Resume must strip stale error rows first, else duplicates. [METHODOLOGY #16]
23. Don't edit benchmark scripts mid-run to "fix" a score. [bench skill]

### Reporting
24. Real metric names, defined on first use (name, what it measures, units, good direction). [experiment R0]
25. Lead with min and spread, then mean; always n. With n < 10, report spread, not a p-value. [experiment R9]
26. A drop that fails significance is **UNRESOLVED**, not a pass. [verify_model.py]
27. Validity label on every result: VALID / INVALID / INCOMPLETE / CANCELLED; never merge partial or cancelled
    output into an aggregate; never delete it. [run-contract, rack SKILL]
28. Append-only ledger entry for EVERY measurement (including null/failed/cancelled) before calling it complete.
    Can't write the ledger = can't claim completion. [rack SKILL]
29. **Blank beats invented.** No source = empty cell plus the command that would produce it; name the source
    next to every metric. A wrong number ends a question, a blank prompts one. [feedback memory]
30. Progress reports: `done/total` (unknown total = `x/unknown`, never invent a denominator), KV cache with source
    and unit or `N/A (no serving)`, t/s saying output-vs-all tokens or `unavailable`. Partial scores never shown as final. [posttraining-reporting]
31. Say what the result does NOT show, beside the result. Report how many configs were tried (winner's curse). [experiment R7, R10; METHODOLOGY #12]

---

## 3. Generic contract the harness enforces

Three artifacts per run, plus one comparison artifact. The agent may write the contract; it may never write
`results.json` or `items.jsonl` values. Those come from the launched command or a thin adapter that reads only
the command's own output files.

### 3.1 `contract.json` (preregistration, frozen before launch)

Hashed (sha256) at launch; any later edit invalidates the run.

```json
{
  "schema": "ukis.contract/v1",
  "run_id": "2026-09-30T14-02Z_gpqa_t20-vs-base",
  "tier": "smoke | pilot | confirmation | final",
  "question": "Does checkpoint X keep GPQA accuracy within 1.5 pp of base while cutting thinking?",
  "decision": {"if": "delta_pp >= -1.5 AND thinking_mean_change <= -15%", "then": "promote X",
               "else": "keep base; investigate"},
  "falsify": "delta_pp < -1.5 with McNemar p < 0.05, or thinking not reduced",
  "family": "gpqa_diamond/qwen38_xhigh_k5_100k",
  "command": {"argv": ["python", "benchmarks/gpqa.py", "--seeds", "0,1,2,3,4", "--out", "runs/..."],
              "cwd": "/abs/path", "env_allowlist": ["API_BASE", "MODEL"], "runner_commit": "8f748ee"},
  "dataset": {"id": "Idavidrein/gpqa:gpqa_diamond.csv", "revision": "633f5ee8...",
              "n_items": 198, "item_ids_sha256": "..."},
  "grader": {"id": "last 'Answer: X'", "revision": "runner_commit"},
  "sampler": {"temperature": 1.0, "top_p": 0.95, "top_k": 20, "min_p": 0.0,
              "presence_penalty": 0.0, "repetition_penalty": 1.0, "reasoning_effort": "xhigh"},
  "max_output_tokens": 100000,
  "timeout_s": 10800,
  "seeds": [0, 1, 2, 3, 4],
  "metrics": [
    {"name": "accuracy", "definition": "correct / (items x seeds), errors+truncations wrong",
     "unit": "percent", "direction": "higher", "primary": true},
    {"name": "completion_tokens_mean", "definition": "mean usage.completion_tokens (thinking+answer)",
     "unit": "tokens", "direction": "lower", "primary": false}
  ],
  "arms": [
    {"name": "base",    "role": "baseline",  "model": "...", "model_revision": "sha/path", "adapter": null, "plugin": "off"},
    {"name": "current", "role": "treatment", "model": "...", "model_revision": "...", "adapter": "path@sha256", "plugin": "off"}
  ],
  "controls": [{"arm": "base", "rules_out": "effect exists without the change"}],
  "mde": {"sd_diff_source": "prior run id or 'unknown'", "sd_diff": 0.9, "mde_pp": 1.13,
          "expected_effect_pp": 3.0},
  "reference_score": {"value": 88.38, "source": "published / prior VALID run id", "tolerance_pp": 2.0},
  "environment": {"serving_engine": "vLLM", "engine_version": "0.27.1", "precision": "BF16",
                  "max_model_len": 262144, "gpus": "..."}
}
```

Launch refusals (before any compute):
- missing any of question / decision (both branches) / falsify / controls / metrics / seeds / dataset.revision / max_output_tokens;
- `tier in {confirmation, final}` and `len(seeds) < 5`;
- `tier != smoke` and any revision empty or floating (`main`, `latest`, `TO_PIN_BEFORE_RUN`);
- no baseline arm for a comparative question;
- `mde.expected_effect < mde.mde` (warn for pilot, refuse for confirmation/final unless an accepted-risk note is attached);
- a metric without name + definition + unit + direction.

### 3.2 `items.jsonl` (one line per item x seed x arm; the evidence)

```json
{"arm": "current", "item_id": "gpqa_017", "seed": 0, "status": "ok|error|timeout|truncated|empty",
 "correct": true, "score": 1.0, "pred": "B", "gold": "B",
 "finish_reason": "stop", "prompt_tokens": 412, "completion_tokens": 5210, "reasoning_tokens": 4980,
 "latency_s": 131.2, "raw_ref": "raw/s0_017.json", "raw_sha256": "..."}
```

`score` generalizes to non-binary metrics (reward, pass rate, IFBench strict/loose as two metric fields).
For multi-part scores use `scores: {"strict": 1, "loose": 1}` and declare both metrics in the contract.

### 3.3 `results.json` (aggregate, recomputed by the validator from items.jsonl)

```json
{
  "schema": "ukis.results/v1",
  "run_id": "...", "contract_sha256": "...", "arm": "current",
  "status": "VALID | INVALID | INCOMPLETE | CANCELLED",
  "status_reason": null,
  "n_expected": 990, "n_rows": 990, "n_distinct": 990,
  "errors": 0, "timeouts": 0, "truncated": 5, "empty": 7,
  "metrics": {
    "accuracy": {"unit": "percent", "mean": 88.59, "sd_seed": 0.85, "min": 87.37, "max": 89.39,
                 "per_seed": {"0": 89.39, "1": 87.37, "2": 87.88, "3": 88.89, "4": 89.39},
                 "wilson95_pooled": [86.4, 90.5], "n": 990},
    "accuracy_no_trunc": {"...": "same shape, rows where status != truncated"},
    "completion_tokens_mean": {"unit": "tokens", "mean": 8895.4, "median": 3003.0, "p90": 27204, "n": 990}
  },
  "source_files": [{"path": "runs/.../score_gpqa.json", "sha256": "..."}],
  "runner_summary_agrees": true,
  "env_observed": {"engine_version": "...", "models_endpoint": ["..."]},
  "started_at": "...", "finished_at": "..."
}
```

If the benchmark CLI already writes its own summary (e.g. `score_<bench>.json`), the validator stores it as a
source file and asserts that its `mean` / `per_seed` / `n` / `errors` / `truncated` match the recomputation
(`runner_summary_agrees`). Disagreement = INVALID, not "pick one".

### 3.4 `comparison.json` (only between arms that pass the comparability check)

```json
{"primary_metric": "accuracy", "baseline": "base", "treatment": "current", "n_pairs": 990,
 "delta_pp": -0.10, "per_seed_delta": {"0": 0.0, "1": -1.01, "...": "..."},
 "sd_diff": 0.9, "mde_pp": 1.13,
 "mcnemar_exact": {"b_base_only": 21, "c_treat_only": 19, "p": 0.87},
 "cluster_bootstrap95_pp": [-1.6, 1.4],
 "verdict": "SUPPORTED | REFUTED | UNRESOLVED | NOT_COMPARABLE",
 "not_shown": ["single benchmark", "one effort level"]}
```

### 3.5 Deterministic validator checks (all must be able to fail; a skipped check is a failure)

Run integrity
- V1 `contract_sha256` unchanged since launch; `command.argv` in the run log equals the contract.
- V2 `n_rows == n_distinct == n_items x len(seeds)`; the set of `(item_id, seed)` equals the frozen ID list x seeds. Count rows with a generation, not lines.
- V3 no duplicate keys; stale error rows stripped on resume.
- V4 every non-ok row is scored wrong (`score = 0`) and is in the denominator.
- V5 seeds observed == seeds declared (the runner's `per_seed` keys must match the contract list).
- V6 observed sampler/cap/effort (from request log or raw rows) == contract; `/v1/models` served name == arm model.
- V7 recomputed aggregates == runner's own summary file (tolerance 1e-6).
- V8 raw files hashed; results reference hashes, so a report number traces to a file.

Status
- V9 any missing expected row -> INCOMPLETE; manual stop -> CANCELLED; V1-V8 failure -> INVALID. Only VALID enters any comparison or table.
- V10 tier gating: smoke never emits a metric to a table; pilot results are rendered as "directional (pilot, k seeds)" and cannot produce SUPPORTED.

Comparability (per pair of arms)
- C1 same `family`: dataset id + revision + item_ids_sha256, seeds, max_output_tokens, timeout, sampler, effort, grader id + revision. Any mismatch -> NOT_COMPARABLE (show separately, never a delta).
- C2 both arms VALID and complete (no partial vs complete).
- C3 exactly one declared variable differs between arms (model/adapter/plugin); otherwise flag confounding.
- C4 baseline sanity: baseline within `reference_score.tolerance` of its reference, else the whole comparison is INVALID.

Claim acceptance
- S1 compute `delta = mean(treat_s - base_s)` over paired seeds, `sd_diff`, `MDE = 2.8 * sd_diff / sqrt(n_seeds)`.
- S2 item-level paired test: exact McNemar on discordant (item, seed) pairs (as in `verify_model.py`), plus a cluster bootstrap over items (resampling items, keeping all seeds of an item together; seeds of one item are not independent). This bootstrap is my proposal; the local tooling uses McNemar and Wilson only.
- S3 verdict: SUPPORTED only if `|delta| >= MDE` and the 95% interval excludes 0 and the direction matches the preregistered decision; REFUTED if the interval excludes the hypothesized direction; else UNRESOLVED. `n_seeds < 5` can never be SUPPORTED. With n < 10 seeds the report shows per-seed spread and does not headline a p-value.
- S4 if any arm truncates, report both headline and no-truncation-intersection accuracy; flag "cap binds".
- S5 decision branch is evaluated mechanically from the preregistered `decision` expression; the agent does not interpret it.
- S6 winner's curse: comparison.json records how many arms/configs were tried in the family.

Rendering
- R1 every cell in a rendered table is either a value pulled from a VALID results/comparison file (with its source path) or `—` plus a short reason (`pending`, `INVALID: V2`, `not comparable: cap 32k vs 100k`, `no run - <command>`). The renderer never accepts a number from the agent's text.
- R2 progress lines: `done/total` from `status.json`; unknown -> `x/unknown`; KV and t/s with source or `N/A (no serving)` / `unavailable`.
- R3 metric names exactly as declared in the contract; definition shown on first appearance.

---

## 4. Canonical reporting table (quoted verbatim)

From `posttraining-reporting/SKILL.md` (identical in `Benchmark_configs/skills/ukisai-benchmarks/SKILL.md`):

| Benchmark | Base accuracy | Prior model accuracy | Current model accuracy | Current vs base | Mean thinking vs base | Median thinking vs base |
|---|---:|---:|---:|---:|---:|---:|
| Benchmark name | 0.00% | 0.00% | 0.00% | +0.00 pp | -0.00% | -0.00% |

Rules attached to it:
- `Current vs base` = current - base, in percentage points.
- Thinking change = `(current thinking / base thinking - 1) * 100%`, same token metric, comparable runs; negative = less thinking.
- No change computed when base is zero, missing, invalid or not comparable. Use `—` with a short reason (in cell or under the table). Keep the Prior column even when empty.
- Replace placeholders with real model names; label multi-part scores (IFBench strict / loose) instead of merging; state run/config and raw-output sources next to the table.

Suggested harness extension (not in the canonical format, my proposal): a second, companion table per
benchmark with `n`, `seeds`, `min`, `max`, `sd_seed`, `MDE`, `McNemar p`, `verdict`, `cap binds`, since
the canonical table carries no spread and rule 25 requires min/spread/n.

Other local formats seen (not canonical, for reference): rack `score.txt`
(`ACCURACY : 85.86%   Wilson95 80.3-90.0`, finish-reason counts, reasoning/completion mean/p50/p90/max),
`*_aggregate.json` (per-arm n, per_seed_accuracy, token stats, per-row list), and Quant_configs `LINE.md`
(`quant | GB | KLD mean | KLD p99 | same top % | gpqa | aime_2025 | verdict`, verdict carries the failing
reason, thresholds from versioned `gates/gates.json`).

---

## 5. Adapter note (Benchmark_configs -> this schema)

A thin adapter can build `items.jsonl` from `runs/<bench>-<model>/raw/*.json`: key `s<seed>_<id>` gives seed
and item_id; fields `content`, `reasoning`, `finish_reason`, `completion_tokens`, `error`, `pred`, `gold`,
`seconds`. `correct` is not stored in raw for most benches (computed in `report()`), so the adapter must
re-apply the benchmark's own grader, or the runner should be patched to write `correct` into `raw/`. IFBench
and LCB grading needs the upstream grader (subprocess / code execution), so re-scoring there means calling
the same grader, not reimplementing it. Agent benches (TB2.1, DeepSWE) have per-trial `result.json` with
`verifier_result.rewards.reward`; attempts are not seeded, so "seed" = attempt index and must be labeled so.

## 6. Gaps and open questions

- Seed semantics: `seed` is a per-request vLLM sampling seed; with batching/continuous batching vLLM output
  is not bitwise reproducible even with a seed. Treat seeds as "5 independent repeats", not as reproducible draws.
- No stored per-benchmark `sd_diff` history, so MDE has to start as "unknown" until a first paired
  confirmation run exists. The harness should cache `sd_seed`/`sd_diff` per family from VALID runs.
- Registry for families does not exist in the current repo (removed with protocol.json). The harness needs
  its own `families/*.json` (dataset, revision, n_items, item-id hash, cap, sampler, grader) or must read
  them from the private run manifest the team mentions but which is not on this machine.
- Thinking-token metric differs across sources (API completion_tokens vs retokenized reasoning). The contract
  must name which; the canonical table's thinking columns are meaningless across mixed units.
