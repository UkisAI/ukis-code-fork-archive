//! V7: check a runner's own summary file against the recomputation.

use super::MetricResult;
use super::RUNNER_SUMMARY_FILE;
use super::SummaryCheck;
use super::TokenStats;
use crate::contract;
use crate::items::ItemRow;
use crate::items::Status;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Float tolerance when checking a runner summary against the recomputation.
const SUMMARY_TOLERANCE: f64 = 1e-6;

/// V7: when the runner wrote its own summary, every number it states must match the
/// recomputation. A summary that states a number we cannot recompute is also a mismatch.
pub(super) fn check_runner_summary(
    dir: &Path,
    primary: &MetricResult,
    rows: &[ItemRow],
    tokens: &BTreeMap<String, TokenStats>,
) -> Option<SummaryCheck> {
    let bytes = std::fs::read(dir.join(RUNNER_SUMMARY_FILE)).ok()?;
    let mut mismatches = Vec::new();
    let summary: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            mismatches.push(format!("not JSON: {e}"));
            Value::Null
        }
    };
    let mut check = |name: &str, stated: Option<f64>, actual: Option<f64>, required: bool| match (
        stated, actual,
    ) {
        (None, _) if !required => {}
        (None, _) => mismatches.push(format!("{name} missing")),
        (Some(s), Some(a)) if (s - a).abs() <= SUMMARY_TOLERANCE => {}
        (Some(s), a) => mismatches.push(format!("{name}: runner says {s}, recomputed {a:?}")),
    };
    let count = |statuses: &[Status]| {
        Some(rows.iter().filter(|r| statuses.contains(&r.status)).count() as f64)
    };
    let field = |key: &str| summary.get(key).and_then(Value::as_f64);

    check("mean", field("mean"), primary.mean, /*required*/ true);
    check(
        "n",
        field("n"),
        Some(rows.len() as f64),
        /*required*/ true,
    );
    check(
        "errors",
        field("errors"),
        count(&[Status::Error, Status::Timeout]),
        /*required*/ false,
    );
    check(
        "truncated",
        field("truncated"),
        count(&[Status::Truncated]),
        /*required*/ false,
    );
    let completion = tokens.get("completion_tokens");
    check(
        "mean_output_tokens",
        field("mean_output_tokens"),
        completion.map(|t| t.mean),
        /*required*/ false,
    );
    check(
        "median_output_tokens",
        field("median_output_tokens"),
        completion.map(|t| t.median),
        /*required*/ false,
    );

    match summary.get("per_seed").and_then(Value::as_object) {
        None => mismatches.push("per_seed missing".to_string()),
        Some(per_seed) => {
            let stated: BTreeMap<Option<i64>, Option<f64>> = per_seed
                .iter()
                .map(|(k, v)| (k.parse().ok(), v.as_f64()))
                .collect();
            let stated_seeds: Vec<Option<i64>> = stated.keys().copied().collect();
            let actual_seeds: Vec<Option<i64>> =
                primary.per_seed.keys().map(|s| Some(*s)).collect();
            if stated_seeds != actual_seeds {
                mismatches.push(format!(
                    "per_seed seeds {stated_seeds:?} != observed {actual_seeds:?}"
                ));
            } else {
                for (seed, value) in &primary.per_seed {
                    let s = stated.get(&Some(*seed)).copied().flatten();
                    if !s.is_some_and(|s| (s - value).abs() <= SUMMARY_TOLERANCE) {
                        mismatches.push(format!(
                            "per_seed[{seed}]: runner says {s:?}, recomputed {value}"
                        ));
                    }
                }
            }
        }
    }
    Some(SummaryCheck {
        path: RUNNER_SUMMARY_FILE.to_string(),
        sha256: contract::sha256_hex(&bytes),
        agrees: mismatches.is_empty(),
        mismatches,
    })
}
