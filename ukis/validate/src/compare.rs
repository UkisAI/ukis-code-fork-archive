//! `compare <baseline_run> <candidate_run>`: paired comparison and a mechanical verdict.
//!
//! The claim under test is always "the candidate improves the primary metric over the
//! baseline", in the metric's declared direction. Both runs are recomputed from their items;
//! no results.json is trusted as input.

use crate::contract::MIN_CLAIM_SEEDS;
use crate::contract::MetricKind;
use crate::contract::Role;
use crate::items::ItemRow;
use crate::items::Status;
use crate::results;
use crate::results::RunEval;
use crate::results::RunStatus;
use crate::stats;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

/// MDE = 2.8 * sd_diff / sqrt(n_seeds); 2.8 = z(0.975) + z(0.80): 95% two-sided, 80% power
/// (experiment skill R2).
const MDE_FACTOR: f64 = 2.8;
const ALPHA: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Supported,
    Refuted,
    Unresolved,
    NotComparable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairedTest {
    /// `mcnemar_exact` for accuracy, `sign_test_exact` for scalar metrics (same math).
    pub name: String,
    /// Pairs where the candidate is better / worse / tied (direction-adjusted).
    pub wins: u64,
    pub losses: u64,
    pub ties: u64,
    pub p: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoTruncIntersection {
    pub n_pairs: u64,
    pub baseline: Option<f64>,
    pub candidate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    pub schema: String,
    pub claim: String,
    pub baseline_run: String,
    pub candidate_run: String,
    pub baseline_status: RunStatus,
    pub candidate_status: RunStatus,
    pub baseline_arm: Option<String>,
    pub candidate_arm: Option<String>,
    pub family: Option<String>,
    pub primary_metric: Option<String>,
    pub unit: Option<String>,
    pub direction: Option<String>,
    pub n_seeds: u64,
    pub n_pairs: u64,
    pub baseline_mean: Option<f64>,
    pub candidate_mean: Option<f64>,
    /// candidate - baseline, raw (not direction-adjusted), in the metric's unit.
    pub delta: Option<f64>,
    pub per_seed_delta: BTreeMap<i64, f64>,
    pub sd_diff: Option<f64>,
    /// A number, or "unknown" when sd_diff cannot be computed (fewer than 2 seeds).
    pub mde: Value,
    pub test: Option<PairedTest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_trunc_intersection: Option<NoTruncIntersection>,
    pub cap_binds: bool,
    pub verdict: Verdict,
    pub reasons: Vec<String>,
    pub not_shown: Vec<String>,
}

/// Paths are canonicalized so a comparison.json can be re-verified from anywhere.
pub fn compare_dirs(baseline: &Path, candidate: &Path) -> Comparison {
    let base = results::evaluate(&canonical(baseline));
    let cand = results::evaluate(&canonical(candidate));
    compare(&base, &cand)
}

fn canonical(p: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

pub fn compare(base: &RunEval, cand: &RunEval) -> Comparison {
    let mut out = Comparison {
        schema: "ukis.comparison/v1".to_string(),
        claim: "candidate improves the primary metric over the baseline".to_string(),
        baseline_run: base.dir.display().to_string(),
        candidate_run: cand.dir.display().to_string(),
        baseline_status: base.results.status,
        candidate_status: cand.results.status,
        baseline_arm: base.results.arm.clone(),
        candidate_arm: cand.results.arm.clone(),
        family: base.results.family.clone(),
        primary_metric: None,
        unit: None,
        direction: None,
        n_seeds: 0,
        n_pairs: 0,
        baseline_mean: None,
        candidate_mean: None,
        delta: None,
        per_seed_delta: BTreeMap::new(),
        sd_diff: None,
        mde: json!("unknown"),
        test: None,
        no_trunc_intersection: None,
        cap_binds: false,
        verdict: Verdict::NotComparable,
        reasons: Vec::new(),
        not_shown: vec![
            "cluster bootstrap CI over items (not implemented)".to_string(),
            "the preregistered decision expression (not evaluated mechanically yet)".to_string(),
            "other benchmarks, efforts, caps and samplers".to_string(),
        ],
    };

    // C2: only VALID, complete runs enter a comparison. A partial arm is biased because
    // rows land shortest-first (METHODOLOGY #1).
    for (label, eval) in [("baseline", base), ("candidate", cand)] {
        if eval.results.status != RunStatus::Valid {
            out.reasons.push(format!(
                "{label} run is {}, not VALID",
                eval.results.status.as_str()
            ));
        }
    }
    let (Some(bc), Some(cc)) = (&base.contract, &cand.contract) else {
        out.reasons.push("a run has no valid contract".to_string());
        return out;
    };
    // C1: same family AND same protocol fields, or the delta mixes two measurements.
    if bc.family != cc.family {
        out.reasons
            .push(format!("family mismatch: {} vs {}", bc.family, cc.family));
    } else if bc.fingerprint != cc.fingerprint {
        out.reasons.push(
            "contract config differs (dataset, grader, sampler, cap, timeout, seeds, metrics or runner commit)"
                .to_string(),
        );
    }
    let base_role = base
        .results
        .arm
        .as_deref()
        .and_then(|a| bc.arm(a))
        .map(|a| a.role);
    if base_role != Some(Role::Baseline) {
        out.reasons
            .push("the baseline run's arm does not have role baseline".to_string());
    }
    if base.results.arm.is_some() && base.results.arm == cand.results.arm {
        out.reasons
            .push("baseline and candidate are the same arm".to_string());
    }
    // Smoke means "ran / did not run", never a number (rule 10).
    if bc.tier == crate::contract::Tier::Smoke || cc.tier == crate::contract::Tier::Smoke {
        out.reasons
            .push("smoke tier never produces a comparison".to_string());
    }
    if !out.reasons.is_empty() {
        return out;
    }

    let metric = bc.primary().clone();
    out.primary_metric = Some(metric.name.clone());
    out.unit = Some(metric.unit.clone());
    out.direction = Some(metric.direction.as_str().to_string());
    let (Some(bm), Some(cm)) = (
        base.results.metrics.get(&metric.name),
        cand.results.metrics.get(&metric.name),
    ) else {
        out.reasons
            .push("primary metric missing from results".to_string());
        return out;
    };
    out.baseline_mean = bm.mean;
    out.candidate_mean = cm.mean;

    // S1: per-seed paired deltas, their spread, and the smallest effect this design can see.
    for (seed, b) in &bm.per_seed {
        if let Some(c) = cm.per_seed.get(seed) {
            out.per_seed_delta.insert(*seed, c - b);
        }
    }
    let deltas: Vec<f64> = out.per_seed_delta.values().copied().collect();
    out.n_seeds = deltas.len() as u64;
    out.delta = stats::mean(&deltas);
    out.sd_diff = stats::sample_sd(&deltas);
    let mde = out
        .sd_diff
        .map(|sd| MDE_FACTOR * sd / (deltas.len() as f64).sqrt());
    out.mde = mde.map_or_else(|| json!("unknown"), |m| json!(m));

    // S2: item-level paired test on (item_id, seed).
    let pairs = pair_rows(&base.rows, &cand.rows);
    out.n_pairs = pairs.len() as u64;
    let sign = metric.direction.sign();
    let (mut wins, mut losses, mut ties) = (0_u64, 0_u64, 0_u64);
    for (b, c) in &pairs {
        let improvement = match metric.kind {
            MetricKind::Accuracy => {
                f64::from(u8::from(c.scored_correct())) - f64::from(u8::from(b.scored_correct()))
            }
            MetricKind::Scalar => match (b.scalar(&metric.name), c.scalar(&metric.name)) {
                (Some(bv), Some(cv)) => sign * (cv - bv),
                _ => 0.0,
            },
        };
        if improvement > 0.0 {
            wins += 1;
        } else if improvement < 0.0 {
            losses += 1;
        } else {
            ties += 1;
        }
    }
    let p = stats::sign_test_p(wins, losses);
    let test_name = match metric.kind {
        MetricKind::Accuracy => "mcnemar_exact",
        MetricKind::Scalar => "sign_test_exact",
    };
    out.test = Some(PairedTest {
        name: test_name.to_string(),
        wins,
        losses,
        ties,
        p,
    });

    // S4: when a cap binds, the headline hides it; report the neither-truncated intersection too.
    if metric.kind == MetricKind::Accuracy {
        let kept: Vec<&(&ItemRow, &ItemRow)> = pairs
            .iter()
            .filter(|(b, c)| b.status != Status::Truncated && c.status != Status::Truncated)
            .collect();
        let acc = |pick: fn(&(&ItemRow, &ItemRow)) -> bool| {
            (!kept.is_empty())
                .then(|| 100.0 * kept.iter().filter(|x| pick(x)).count() as f64 / kept.len() as f64)
        };
        out.cap_binds = kept.len() < pairs.len();
        out.no_trunc_intersection = Some(NoTruncIntersection {
            n_pairs: kept.len() as u64,
            baseline: acc(|(b, _)| b.scored_correct()),
            candidate: acc(|(_, c)| c.scored_correct()),
        });
    }

    let tiers_can_claim = bc.tier.can_claim() && cc.tier.can_claim();
    let (verdict, reasons) = decide(
        out.delta,
        mde,
        sign,
        p,
        wins,
        losses,
        deltas.len(),
        tiers_can_claim,
    );
    out.verdict = verdict;
    out.reasons = reasons;
    out
}

fn pair_rows<'a>(base: &'a [ItemRow], cand: &'a [ItemRow]) -> Vec<(&'a ItemRow, &'a ItemRow)> {
    let by_key: BTreeMap<(&str, i64), &ItemRow> = cand
        .iter()
        .map(|r| ((r.item_id.as_str(), r.seed), r))
        .collect();
    base.iter()
        .filter_map(|b| by_key.get(&(b.item_id.as_str(), b.seed)).map(|c| (b, *c)))
        .collect()
}

/// S3, the whole claim-acceptance rule in one place.
#[allow(clippy::too_many_arguments)]
fn decide(
    delta: Option<f64>,
    mde: Option<f64>,
    sign: f64,
    p: f64,
    wins: u64,
    losses: u64,
    n_seeds: usize,
    tiers_can_claim: bool,
) -> (Verdict, Vec<String>) {
    let Some(delta) = delta else {
        return (Verdict::Unresolved, vec!["no paired seeds".to_string()]);
    };
    let improvement = sign * delta;
    let significant = p < ALPHA;
    // The mean delta and the item-level test must point the same way, else there is no signal.
    let test_agrees = if improvement > 0.0 {
        wins > losses
    } else {
        losses > wins
    };

    let mut blockers = Vec::new();
    if n_seeds < MIN_CLAIM_SEEDS {
        blockers.push(format!(
            "{n_seeds} seeds < {MIN_CLAIM_SEEDS}: no claim either way"
        ));
    }
    if !tiers_can_claim {
        blockers.push("pilot tier is directional only".to_string());
    }
    if !significant {
        blockers.push(format!(
            "paired test not significant (p = {p:.4} >= {ALPHA})"
        ));
    } else if !test_agrees {
        blockers.push("mean delta and item-level test disagree in direction".to_string());
    }

    // REFUTED: a significant move in the wrong direction, on a design that can make claims.
    // A drop that fails significance is NOT a pass and NOT a refutation: it stays UNRESOLVED.
    if improvement < 0.0 && blockers.is_empty() {
        return (
            Verdict::Refuted,
            vec![format!(
                "significant change in the bad direction (p = {p:.4})"
            )],
        );
    }
    if improvement <= 0.0 {
        blockers.push("delta is not in the good direction".to_string());
    }
    match mde {
        None => blockers.push("MDE unknown (needs >= 2 seeds)".to_string()),
        Some(m) if delta.abs() < m => {
            blockers.push(format!("|delta| {:.4} < MDE {m:.4}", delta.abs()))
        }
        Some(_) => {}
    }
    if blockers.is_empty() {
        (
            Verdict::Supported,
            vec![format!(
                "|delta| >= MDE and p = {p:.4} < {ALPHA} in the good direction"
            )],
        )
    } else {
        (Verdict::Unresolved, blockers)
    }
}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;
