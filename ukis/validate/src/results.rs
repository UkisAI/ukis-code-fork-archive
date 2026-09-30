//! `run <run_dir>`: recompute `results.json` from `contract.json` + `items.jsonl`.
//!
//! A run dir holds ONE arm. Nothing is read from a summary the runner (or a model) wrote;
//! a runner summary, when present, is only checked against the recomputation (V7).

use crate::contract;
use crate::contract::Contract;
use crate::contract::MetricKind;
use crate::items;
use crate::items::ItemRow;
use crate::items::Status;
use crate::stats;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

pub const ITEMS_FILE: &str = "items.jsonl";
pub const RESULTS_FILE: &str = "results.json";
/// Optional: the benchmark runner's own summary, copied verbatim (Benchmark_configs
/// `score_<bench>.json` format: mean, per_seed, n, errors, truncated, *_output_tokens).
pub const RUNNER_SUMMARY_FILE: &str = "runner_summary.json";
/// Keep reason lists readable: list this many examples, then count the rest.
const MAX_EXAMPLES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RunStatus {
    Valid,
    Invalid,
    Incomplete,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Valid => "VALID",
            RunStatus::Invalid => "INVALID",
            RunStatus::Incomplete => "INCOMPLETE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricResult {
    pub kind: String,
    pub unit: String,
    pub direction: String,
    pub primary: bool,
    /// Rows in the denominator (every row for accuracy, ok rows for scalars).
    pub n: u64,
    pub per_seed: BTreeMap<i64, f64>,
    /// Mean of the per-seed values.
    pub mean: Option<f64>,
    pub sd_seed: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correct: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wilson95_pooled: Option<[f64; 2]>,
    /// Same metric over rows that were not truncated: shows whether the cap binds (rule 18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluding_truncated: Option<Box<MetricResult>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TokenStats {
    pub n: u64,
    pub mean: f64,
    pub median: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SummaryCheck {
    pub path: String,
    pub sha256: String,
    pub agrees: bool,
    pub mismatches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Results {
    pub schema: String,
    pub status: RunStatus,
    /// Every reason the run is not VALID, prefixed with the status it causes.
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub contract_sha256: Option<String>,
    pub family: Option<String>,
    pub benchmark: Option<String>,
    pub tier: Option<String>,
    pub arm: Option<String>,
    pub role: Option<String>,
    pub n_expected: Option<u64>,
    pub n_rows: u64,
    pub n_distinct: u64,
    pub status_counts: BTreeMap<String, u64>,
    pub metrics: BTreeMap<String, MetricResult>,
    /// Token stats over rows with a positive count (the Benchmark_configs `report()` rule).
    pub tokens: BTreeMap<String, TokenStats>,
    pub runner_summary: Option<SummaryCheck>,
    pub source_files: Vec<SourceFile>,
}

/// Everything `compare` and `table` need: the results plus the typed contract and rows
/// they were computed from, so nobody re-reads a results.json as a source of numbers.
pub struct RunEval {
    pub dir: PathBuf,
    pub results: Results,
    pub contract: Option<Contract>,
    pub rows: Vec<ItemRow>,
}

#[derive(Default)]
struct Problems {
    invalid: Vec<String>,
    incomplete: Vec<String>,
    warnings: Vec<String>,
}

pub fn evaluate(dir: &Path) -> RunEval {
    let mut p = Problems::default();
    let mut source_files = Vec::new();

    // V1: the contract must be the one that was locked at launch.
    let mut contract = None;
    let mut contract_sha = None;
    match contract::load(dir) {
        Err(e) => p.invalid.push(e),
        Ok((value, parsed)) => {
            let sha = contract::canonical_sha256(&value);
            match std::fs::read_to_string(dir.join(contract::LOCK_FILE)) {
                Err(_) => p.invalid.push(format!(
                    "contract was never locked ({} missing; run check-contract before launch)",
                    contract::LOCK_FILE
                )),
                Ok(locked) if locked.trim() != sha => p.invalid.push(format!(
                    "contract hash mismatch: locked {}, now {sha} (edited after launch)",
                    locked.trim()
                )),
                Ok(_) => {}
            }
            match parsed {
                Ok(c) => contract = Some(c),
                Err(refusals) => p.invalid.extend(
                    refusals
                        .into_iter()
                        .map(|r| format!("contract refused: {r}")),
                ),
            }
            contract_sha = Some(sha);
        }
    }
    hash_source(dir, "contract.json", &mut source_files);

    let rows = match std::fs::read_to_string(dir.join(ITEMS_FILE)) {
        Err(_) => {
            p.incomplete.push(format!(
                "no {ITEMS_FILE} (run not finished or adapter not run)"
            ));
            Vec::new()
        }
        Ok(text) => match items::parse(&text) {
            Ok(rows) => rows,
            Err(errors) => {
                p.invalid.extend(examples(&errors));
                Vec::new()
            }
        },
    };
    hash_source(dir, ITEMS_FILE, &mut source_files);

    let mut status_counts = BTreeMap::new();
    for row in &rows {
        *status_counts
            .entry(row.status.as_str().to_string())
            .or_insert(0) += 1;
    }
    let keys: BTreeSet<(&str, i64)> = rows.iter().map(|r| (r.item_id.as_str(), r.seed)).collect();

    let mut arm = None;
    let mut metrics = BTreeMap::new();
    if let Some(c) = &contract {
        arm = check_rows(c, &rows, &mut p);
        for m in &c.metrics {
            metrics.insert(m.name.clone(), metric_result(m, &rows));
        }
    }
    let tokens = token_stats(&rows);

    let runner_summary = contract.as_ref().and_then(|c| {
        let primary = metrics.get(&c.primary().name)?;
        let check = check_runner_summary(dir, primary, &rows, &tokens)?;
        if !check.agrees {
            // Disagreement is INVALID, never "pick one" (study/E 3.3).
            p.invalid.extend(
                check
                    .mismatches
                    .iter()
                    .map(|m| format!("runner summary disagrees: {m}")),
            );
        }
        source_files.push(SourceFile {
            path: check.path.clone(),
            sha256: check.sha256.clone(),
        });
        Some(check)
    });

    let status = if !p.invalid.is_empty() {
        RunStatus::Invalid
    } else if !p.incomplete.is_empty() {
        RunStatus::Incomplete
    } else {
        RunStatus::Valid
    };
    let reasons = p
        .invalid
        .iter()
        .map(|r| format!("INVALID: {r}"))
        .chain(p.incomplete.iter().map(|r| format!("INCOMPLETE: {r}")))
        .collect();
    let role = contract
        .as_ref()
        .zip(arm.as_deref())
        .and_then(|(c, a)| c.arm(a))
        .map(|a| a.role.as_str().to_string());
    let results = Results {
        schema: "ukis.results/v1".to_string(),
        status,
        reasons,
        warnings: p.warnings,
        contract_sha256: contract_sha,
        family: contract.as_ref().map(|c| c.family.clone()),
        benchmark: contract.as_ref().map(|c| c.benchmark.clone()),
        tier: contract.as_ref().map(|c| c.tier.as_str().to_string()),
        arm,
        role,
        n_expected: contract.as_ref().map(|c| c.n_items * c.seeds.len() as u64),
        n_rows: rows.len() as u64,
        n_distinct: keys.len() as u64,
        status_counts,
        metrics,
        tokens,
        runner_summary,
        source_files,
    };
    RunEval {
        dir: dir.to_path_buf(),
        results,
        contract,
        rows,
    }
}

fn hash_source(dir: &Path, name: &str, out: &mut Vec<SourceFile>) {
    if let Ok(bytes) = std::fs::read(dir.join(name)) {
        out.push(SourceFile {
            path: name.to_string(),
            sha256: contract::sha256_hex(&bytes),
        });
    }
}

fn examples(all: &[String]) -> Vec<String> {
    let mut out: Vec<String> = all.iter().take(MAX_EXAMPLES).cloned().collect();
    if all.len() > MAX_EXAMPLES {
        out.push(format!("... and {} more", all.len() - MAX_EXAMPLES));
    }
    out
}

/// Integrity checks V2-V5 plus per-metric row checks. Returns the run's arm name.
fn check_rows(c: &Contract, rows: &[ItemRow], p: &mut Problems) -> Option<String> {
    if rows.is_empty() {
        p.incomplete.push("no rows".to_string());
        return None;
    }
    // One run dir = one arm, and it must be an arm the contract declared before launch.
    let arms: BTreeSet<&str> = rows.iter().map(|r| r.arm.as_str()).collect();
    let arm = if arms.len() > 1 {
        p.invalid.push(format!(
            "items.jsonl mixes arms {arms:?}; one run dir holds one arm"
        ));
        None
    } else {
        let name = arms.first().map(|a| a.to_string());
        if let Some(name) = &name
            && c.arm(name).is_none()
        {
            p.invalid
                .push(format!("arm \"{name}\" is not declared in the contract"));
        }
        name
    };

    // V3: a duplicate key means a resume did not strip a stale row; it would double-count.
    let mut counts: BTreeMap<(&str, i64), u64> = BTreeMap::new();
    for r in rows {
        *counts.entry((r.item_id.as_str(), r.seed)).or_insert(0) += 1;
    }
    let dups: Vec<String> = counts
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|((item, seed), n)| format!("item {item} seed {seed} x{n}"))
        .collect();
    if !dups.is_empty() {
        p.invalid.push(format!(
            "{} duplicate (item_id, seed) keys: {}",
            dups.len(),
            examples(&dups).join(", ")
        ));
    }

    // V5: seeds observed must equal seeds declared. An undeclared seed smells of seed shopping;
    // a missing one means the run is not finished.
    let declared: BTreeSet<i64> = c.seeds.iter().copied().collect();
    let observed: BTreeSet<i64> = rows.iter().map(|r| r.seed).collect();
    let extra: Vec<i64> = observed.difference(&declared).copied().collect();
    let missing: Vec<i64> = declared.difference(&observed).copied().collect();
    if !extra.is_empty() {
        p.invalid
            .push(format!("seed set != contract: undeclared seeds {extra:?}"));
    }
    if !missing.is_empty() {
        p.incomplete
            .push(format!("seed set != contract: missing seeds {missing:?}"));
    }

    // V2: exactly n_items x seeds distinct keys, the same item set in every seed.
    let items: BTreeSet<&str> = rows.iter().map(|r| r.item_id.as_str()).collect();
    if items.len() as u64 > c.n_items {
        p.invalid.push(format!(
            "{} distinct item ids, contract freezes n_items = {}",
            items.len(),
            c.n_items
        ));
    }
    let expected = c.n_items * declared.len() as u64;
    let have = counts.keys().filter(|(_, s)| declared.contains(s)).count() as u64;
    if have < expected {
        p.incomplete.push(format!(
            "n != items x seeds: {have} of {expected} expected (item, seed) rows"
        ));
    }
    if let Some(frozen) = &c.item_ids_sha256
        && items.len() as u64 == c.n_items
    {
        let joined: Vec<&str> = items.iter().copied().collect();
        let actual = contract::sha256_hex(joined.join("\n").as_bytes());
        if &actual != frozen {
            p.invalid.push(format!(
                "item ids differ from the frozen list: sha256 {actual} != {frozen}"
            ));
        }
    }

    for m in &c.metrics {
        match m.kind {
            MetricKind::Accuracy => {
                let unscored = rows
                    .iter()
                    .filter(|r| r.status == Status::Ok && r.correct.is_none())
                    .count();
                if unscored > 0 {
                    p.invalid.push(format!(
                        "{unscored} ok rows have no `correct` for {}",
                        m.name
                    ));
                }
                let claimed = rows
                    .iter()
                    .filter(|r| r.status != Status::Ok && r.correct == Some(true))
                    .count();
                if claimed > 0 {
                    p.warnings.push(format!(
                        "{claimed} non-ok rows say correct=true; scored WRONG (errors, timeouts, empty and truncated always count wrong)"
                    ));
                }
            }
            MetricKind::Scalar => {
                let unmeasured = rows
                    .iter()
                    .filter(|r| r.status == Status::Ok && r.scalar(&m.name).is_none())
                    .count();
                if unmeasured > 0 {
                    p.invalid
                        .push(format!("{unmeasured} ok rows have no value for {}", m.name));
                }
                // A failed scalar measurement has no honest fill value (0 tok/s? infinite KLD?),
                // so the run is incomplete until those rows are re-measured.
                let failed = rows.iter().filter(|r| r.status != Status::Ok).count();
                if failed > 0 {
                    p.incomplete.push(format!(
                        "{failed} non-ok rows have no measurement for {}",
                        m.name
                    ));
                }
            }
        }
    }
    arm
}

fn metric_result(m: &contract::Metric, rows: &[ItemRow]) -> MetricResult {
    let mut result = metric_over(m, rows);
    if m.kind == MetricKind::Accuracy {
        let kept: Vec<ItemRow> = rows
            .iter()
            .filter(|r| r.status != Status::Truncated)
            .cloned()
            .collect();
        result.excluding_truncated = Some(Box::new(metric_over(m, &kept)));
    }
    result
}

fn metric_over(m: &contract::Metric, rows: &[ItemRow]) -> MetricResult {
    let seeds: BTreeSet<i64> = rows.iter().map(|r| r.seed).collect();
    let mut per_seed = BTreeMap::new();
    let (mut n, mut correct) = (0_u64, 0_u64);
    for seed in seeds {
        let in_seed: Vec<&ItemRow> = rows.iter().filter(|r| r.seed == seed).collect();
        let value = match m.kind {
            MetricKind::Accuracy => {
                let k = in_seed.iter().filter(|r| r.scored_correct()).count() as u64;
                n += in_seed.len() as u64;
                correct += k;
                Some(100.0 * k as f64 / in_seed.len() as f64)
            }
            MetricKind::Scalar => {
                let values: Vec<f64> = in_seed
                    .iter()
                    .filter(|r| r.status == Status::Ok)
                    .filter_map(|r| r.scalar(&m.name))
                    .collect();
                n += values.len() as u64;
                stats::mean(&values)
            }
        };
        if let Some(v) = value {
            per_seed.insert(seed, v);
        }
    }
    let values: Vec<f64> = per_seed.values().copied().collect();
    let accuracy = m.kind == MetricKind::Accuracy;
    MetricResult {
        kind: if accuracy { "accuracy" } else { "scalar" }.to_string(),
        unit: m.unit.clone(),
        direction: m.direction.as_str().to_string(),
        primary: m.primary,
        n,
        mean: stats::mean(&values),
        sd_seed: stats::sample_sd(&values),
        min: values.iter().copied().reduce(f64::min),
        max: values.iter().copied().reduce(f64::max),
        per_seed,
        correct: accuracy.then_some(correct),
        wilson95_pooled: if accuracy {
            stats::wilson95(correct, n).map(|(lo, hi)| [100.0 * lo, 100.0 * hi])
        } else {
            None
        },
        excluding_truncated: None,
    }
}

fn token_stats(rows: &[ItemRow]) -> BTreeMap<String, TokenStats> {
    let mut out = BTreeMap::new();
    for field in ["completion_tokens", "reasoning_tokens"] {
        let values: Vec<f64> = rows
            .iter()
            .filter_map(|r| r.tokens(field))
            .filter(|t| *t > 0)
            .map(|t| t as f64)
            .collect();
        if let (Some(mean), Some(median)) = (stats::mean(&values), stats::median(&values)) {
            out.insert(
                field.to_string(),
                TokenStats {
                    n: values.len() as u64,
                    mean,
                    median,
                },
            );
        }
    }
    out
}

/// `run` command: evaluate and write `<run_dir>/results.json`.
pub fn run_and_write(dir: &Path) -> Result<Results, String> {
    let eval = evaluate(dir);
    let text = serde_json::to_string_pretty(&eval.results).map_err(|e| e.to_string())?;
    let out = dir.join(RESULTS_FILE);
    std::fs::write(&out, text + "\n")
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    Ok(eval.results)
}

mod summary;
use summary::check_runner_summary;

#[cfg(test)]
#[path = "results_tests.rs"]
mod tests;
