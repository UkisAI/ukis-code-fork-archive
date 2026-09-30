# ukis-validate

Deterministic core of ukis-research: the model proposes, this code decides. Turns recorded run
data into results and verdicts. Rules: `docs/ukis-research/study/E-ukis-bench-cli.md`.
Standalone Cargo project (own `[workspace]`), not part of codex-rs.

```bash
cargo build --release          # binary: target/release/ukis-validate
cargo test                     # unit tests + end-to-end fixture
```

A run dir holds ONE arm: `contract.json`, `contract.sha256` (lock), `items.jsonl`, optional
`runner_summary.json`, and the computed `results.json`.

```bash
ukis-validate check-contract runs/current        # refuse or lock (writes contract.sha256), before launch
ukis-validate import benchmark-configs runs/current --arm current   # raw/ + score_<bench>.json -> items.jsonl
ukis-validate run runs/current                   # recompute results.json; exit 0 only if VALID
ukis-validate compare runs/base runs/current --out comparison.json
ukis-validate table comparison.json runs/prior   # canonical table from VALID data only
```

- **contract.json** (`ukis.contract/v1`): tier, question, decision {if, then, else}, falsify,
  family, command {argv, runner_commit}, dataset {id, revision, n_items, item_ids_sha256?},
  grader?, sampler, max_output_tokens, seeds, metrics [{name, definition, unit,
  direction higher_better|lower_better, kind accuracy|scalar, primary}], arms [{name, role
  baseline|prior|treatment|control}], controls [{arm, rules_out}], thinking_tokens?.
  Refused: missing fields, floating revisions outside smoke, no baseline arm, confirmation/final
  with < 5 seeds, metrics without name/unit/direction/definition.
- **items.jsonl**: `{arm, item_id, seed, status ok|error|timeout|empty|truncated, correct?,
  value?, values?, completion_tokens?, reasoning_tokens?, raw_ref?, raw_sha256?}`. Non-ok rows
  count wrong and stay in the denominator.
- **results.json**: VALID / INVALID / INCOMPLETE with reasons (contract hash, seed set,
  n = items x seeds, duplicates, arms, runner summary disagreement); per-seed, mean, sd, min,
  max, n, Wilson 95% and accuracy excluding truncated.
- **comparison.json**: delta, per-seed deltas, `MDE = 2.8 * sd_diff / sqrt(n_seeds)`, exact
  McNemar (accuracy) or exact sign test (scalars) on (item, seed) pairs, verdict SUPPORTED /
  REFUTED / UNRESOLVED / NOT_COMPARABLE. < 5 seeds or pilot tier is never SUPPORTED; a
  non-significant drop is UNRESOLVED.
- **table**: every input is recomputed; a results/comparison file that does not match its
  recomputation is refused. Missing or non-VALID cells are `- (n)` with a footnote.

Not yet: cluster bootstrap CI, sd history for MDE priors, agent-benchmark attempts, mechanical
evaluation of the `decision` expression, observed-sampler check (V6).
