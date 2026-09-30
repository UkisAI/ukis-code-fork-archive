use super::*;
use crate::test_support::good_contract;
use crate::test_support::kld_contract;
use crate::test_support::row;
use crate::test_support::rows;
use crate::test_support::write_run;
use serde_json::Value;
use serde_json::json;

const SEEDS: [i64; 5] = [0, 1, 2, 3, 4];

/// 20 items; item < 14 correct, except item 13 is wrong on odd seeds: 70% / 65% per seed.
fn base_rows() -> Vec<ItemRow> {
    rows("base", 20, &SEEDS, |i, s| i < 13 || (i == 13 && s % 2 == 0))
}

fn has(results: &Results, needle: &str) -> bool {
    results.reasons.iter().any(|r| r.contains(needle))
}

#[test]
fn valid_run_recomputes_accuracy() {
    let dir = write_run("valid", &good_contract(), &base_rows(), None);
    let r = evaluate(&dir).results;
    assert_eq!(r.status, RunStatus::Valid, "{:?}", r.reasons);
    let acc = &r.metrics["accuracy"];
    let per_seed: BTreeMap<i64, f64> =
        [(0, 70.0), (1, 65.0), (2, 70.0), (3, 65.0), (4, 70.0)].into();
    assert_eq!(acc.per_seed, per_seed);
    assert_eq!(acc.mean, Some(68.0));
    assert_eq!(
        (acc.min, acc.max, acc.n, acc.correct),
        (Some(65.0), Some(70.0), 100, Some(68))
    );
    assert!(acc.wilson95_pooled.is_some());
    assert_eq!(
        (r.n_expected, r.n_rows, r.n_distinct),
        (Some(100), 100, 100)
    );
    assert_eq!(r.arm.as_deref(), Some("base"));
    assert_eq!(r.role.as_deref(), Some("baseline"));
}

#[test]
fn failures_count_wrong_and_stay_in_denominator() {
    let mut rows = base_rows();
    // Seed 0: item 0 errored, item 1 truncated. Both claim correct=true; neither may count.
    rows[0] = row("base", 0, 0, Status::Error, true);
    rows[1] = row("base", 1, 0, Status::Truncated, true);
    let dir = write_run("failures", &good_contract(), &rows, None);
    let r = evaluate(&dir).results;
    assert_eq!(r.status, RunStatus::Valid, "{:?}", r.reasons);
    let acc = &r.metrics["accuracy"];
    assert_eq!(acc.per_seed[&0], 60.0); // 12/20, the denominator is still 20
    assert_eq!(acc.n, 100);
    // Excluding truncated: seed 0 is 12/19.
    let no_trunc = acc.excluding_truncated.as_ref().unwrap();
    assert_eq!(no_trunc.per_seed[&0], 100.0 * 12.0 / 19.0);
    assert_eq!(
        r.warnings.len(),
        1,
        "non-ok rows claiming correct are flagged"
    );
}

fn honest_summary() -> Value {
    json!({"benchmark": "toy", "model": "m", "mean": 68.0,
           "per_seed": {"0": 70.0, "1": 65.0, "2": 70.0, "3": 65.0, "4": 70.0},
           "n": 100, "errors": 0, "truncated": 0,
           "mean_output_tokens": 1000.0, "median_output_tokens": 1000.0})
}

#[test]
fn honest_runner_summary_agrees() {
    let dir = write_run(
        "summary-ok",
        &good_contract(),
        &base_rows(),
        Some(honest_summary()),
    );
    let r = evaluate(&dir).results;
    assert_eq!(r.status, RunStatus::Valid, "{:?}", r.reasons);
    assert!(r.runner_summary.as_ref().unwrap().agrees);
    assert_eq!(r.source_files.len(), 3);
}

#[test]
fn invented_runner_summary_is_invalid() {
    let mut summary = honest_summary();
    summary["mean"] = json!(88.0); // a number nobody measured
    let dir = write_run(
        "summary-invented",
        &good_contract(),
        &base_rows(),
        Some(summary),
    );
    let r = evaluate(&dir).results;
    assert_eq!(r.status, RunStatus::Invalid);
    assert!(has(&r, "runner summary disagrees: mean"), "{:?}", r.reasons);
}

#[test]
fn summary_with_other_seeds_is_invalid() {
    let mut summary = honest_summary();
    summary["per_seed"] = json!({"0": 70.0, "1": 65.0, "2": 70.0});
    let dir = write_run(
        "summary-seeds",
        &good_contract(),
        &base_rows(),
        Some(summary),
    );
    let r = evaluate(&dir).results;
    assert_eq!(r.status, RunStatus::Invalid);
    assert!(has(&r, "per_seed seeds"), "{:?}", r.reasons);
}

#[test]
fn contract_edited_after_lock_is_invalid() {
    let dir = write_run("edited", &good_contract(), &base_rows(), None);
    let mut c = good_contract();
    c["decision"]["then"] = json!("promote current even if flat");
    std::fs::write(dir.join("contract.json"), c.to_string()).unwrap();
    let r = evaluate(&dir).results;
    assert_eq!(r.status, RunStatus::Invalid);
    assert!(has(&r, "contract hash mismatch"), "{:?}", r.reasons);
}

#[test]
fn unlocked_contract_is_invalid() {
    let dir = write_run("unlocked", &good_contract(), &base_rows(), None);
    std::fs::remove_file(dir.join(contract::LOCK_FILE)).unwrap();
    let r = evaluate(&dir).results;
    assert_eq!(r.status, RunStatus::Invalid);
    assert!(has(&r, "never locked"), "{:?}", r.reasons);
}

#[test]
fn duplicate_rows_are_invalid() {
    let mut rows = base_rows();
    rows.push(row("base", 5, 2, Status::Ok, true));
    let r = evaluate(&write_run("dups", &good_contract(), &rows, None)).results;
    assert_eq!(r.status, RunStatus::Invalid);
    assert!(has(&r, "duplicate (item_id, seed)"), "{:?}", r.reasons);
}

#[test]
fn missing_rows_are_incomplete() {
    let mut rows = base_rows();
    rows.retain(|r| !(r.seed == 3 && r.item_id == "019"));
    let r = evaluate(&write_run("missing", &good_contract(), &rows, None)).results;
    assert_eq!(r.status, RunStatus::Incomplete);
    assert!(has(&r, "99 of 100"), "{:?}", r.reasons);
}

#[test]
fn seed_set_must_match_contract() {
    // A dropped seed is unfinished work; an extra seed is undeclared (seed shopping).
    let dropped: Vec<ItemRow> = base_rows().into_iter().filter(|r| r.seed != 4).collect();
    let r = evaluate(&write_run("seed-missing", &good_contract(), &dropped, None)).results;
    assert_eq!(r.status, RunStatus::Incomplete);
    assert!(has(&r, "missing seeds [4]"), "{:?}", r.reasons);

    let mut extra = base_rows();
    extra.extend(rows("base", 20, &[7], |_, _| true));
    let r = evaluate(&write_run("seed-extra", &good_contract(), &extra, None)).results;
    assert_eq!(r.status, RunStatus::Invalid);
    assert!(has(&r, "undeclared seeds [7]"), "{:?}", r.reasons);
}

#[test]
fn mixed_arms_are_invalid() {
    let mut rows = base_rows();
    rows[0].arm = "current".to_string();
    let r = evaluate(&write_run("mixed", &good_contract(), &rows, None)).results;
    assert_eq!(r.status, RunStatus::Invalid);
    assert!(has(&r, "mixes arms"), "{:?}", r.reasons);
}

#[test]
fn frozen_item_ids_are_checked() {
    let mut c = good_contract();
    c["dataset"]["item_ids_sha256"] = json!("0000");
    let r = evaluate(&write_run("item-hash", &c, &base_rows(), None)).results;
    assert_eq!(r.status, RunStatus::Invalid);
    assert!(has(&r, "item ids differ"), "{:?}", r.reasons);
}

#[test]
fn scalar_metric_failures_are_incomplete() {
    let mut rows = rows("base", 20, &SEEDS, |_, _| true);
    for r in &mut rows {
        r.correct = None;
        r.value = Some(0.02);
    }
    let dir = write_run("kld-ok", &kld_contract(), &rows, None);
    let r = evaluate(&dir).results;
    assert_eq!(r.status, RunStatus::Valid, "{:?}", r.reasons);
    assert!((r.metrics["kld"].mean.unwrap() - 0.02).abs() < 1e-12);

    rows[3].status = Status::Error;
    rows[3].value = None;
    let r = evaluate(&write_run("kld-fail", &kld_contract(), &rows, None)).results;
    assert_eq!(r.status, RunStatus::Incomplete);
    assert!(has(&r, "no measurement for kld"), "{:?}", r.reasons);
}
