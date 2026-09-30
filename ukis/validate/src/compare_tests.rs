use super::*;
use crate::test_support::good_contract;
use crate::test_support::kld_contract;
use crate::test_support::rows;
use crate::test_support::write_run;
use serde_json::json;

const SEEDS: [i64; 5] = [0, 1, 2, 3, 4];

/// Base: items < 10 of 20 correct (50%) in every seed.
fn base_correct(i: u32, _seed: i64) -> bool {
    i < 10
}

fn run_pair(contract: &Value, seeds: &[i64], cand: impl Fn(u32, i64) -> bool) -> Comparison {
    let base = write_run(
        "base",
        contract,
        &rows("base", 20, seeds, base_correct),
        None,
    );
    let current = write_run("current", contract, &rows("current", 20, seeds, cand), None);
    compare_dirs(&base, &current)
}

#[test]
fn five_seed_clear_win_is_supported() {
    // +3 items every seed, +1 more on even seeds: 18 wins, no losses, per-seed deltas 15-20 pp.
    let cmp = run_pair(&good_contract(), &SEEDS, |i, s| {
        i < 13 || (i == 13 && s % 2 == 0)
    });
    assert_eq!(cmp.verdict, Verdict::Supported, "{:?}", cmp.reasons);
    assert_eq!(cmp.n_seeds, 5);
    assert_eq!(cmp.n_pairs, 100);
    assert_eq!(cmp.test.as_ref().map(|t| (t.wins, t.losses)), Some((18, 0)));
    assert!(cmp.delta.unwrap() > 15.0);
}

#[test]
fn three_seed_win_is_never_supported() {
    let mut c = good_contract();
    c["tier"] = json!("pilot");
    c["seeds"] = json!([0, 1, 2]);
    // A huge, consistent win: still only directional with 3 seeds.
    let cmp = run_pair(&c, &[0, 1, 2], |i, _| i < 18);
    assert_eq!(cmp.verdict, Verdict::Unresolved);
    assert!(
        cmp.reasons.iter().any(|r| r.contains("3 seeds < 5")),
        "{:?}",
        cmp.reasons
    );
}

#[test]
fn non_significant_drop_is_unresolved_not_a_pass() {
    // Lose item 9 on seeds 0 and 1 only: a small drop, p = 0.5.
    let cmp = run_pair(&good_contract(), &SEEDS, |i, s| i < 9 || (i == 9 && s >= 2));
    assert!(cmp.delta.unwrap() < 0.0);
    assert_eq!(cmp.verdict, Verdict::Unresolved);
    assert!(
        cmp.reasons.iter().any(|r| r.contains("not significant")),
        "{:?}",
        cmp.reasons
    );
}

#[test]
fn significant_drop_is_refuted() {
    let cmp = run_pair(&good_contract(), &SEEDS, |i, _| i < 6);
    assert_eq!(cmp.verdict, Verdict::Refuted, "{:?}", cmp.reasons);
}

#[test]
fn significant_but_below_mde_is_unresolved() {
    // Seed 4 carries almost all of the gain: item-level p is tiny, but per-seed deltas are so
    // spread that this design cannot resolve an effect of this size.
    let cmp = run_pair(&good_contract(), &SEEDS, |i, s| {
        i < 10 || (i == 10 && s != 4) || (s == 4 && i < 20)
    });
    assert!(cmp.test.as_ref().unwrap().p < 0.05);
    assert_eq!(cmp.verdict, Verdict::Unresolved);
    assert!(
        cmp.reasons.iter().any(|r| r.contains("< MDE")),
        "{:?}",
        cmp.reasons
    );
}

#[test]
fn family_mismatch_is_not_comparable() {
    let base = write_run(
        "base",
        &good_contract(),
        &rows("base", 20, &SEEDS, base_correct),
        None,
    );
    let mut other = good_contract();
    other["family"] = json!("toy/xhigh_32k");
    let current = write_run(
        "current",
        &other,
        &rows("current", 20, &SEEDS, |_, _| true),
        None,
    );
    let cmp = compare_dirs(&base, &current);
    assert_eq!(cmp.verdict, Verdict::NotComparable);
    assert!(
        cmp.reasons[0].contains("family mismatch"),
        "{:?}",
        cmp.reasons
    );
    assert_eq!(cmp.delta, None, "no delta is ever computed across families");
}

#[test]
fn config_mismatch_is_not_comparable() {
    let base = write_run(
        "base",
        &good_contract(),
        &rows("base", 20, &SEEDS, base_correct),
        None,
    );
    let mut other = good_contract();
    other["max_output_tokens"] = json!(32000);
    let current = write_run(
        "current",
        &other,
        &rows("current", 20, &SEEDS, |_, _| true),
        None,
    );
    let cmp = compare_dirs(&base, &current);
    assert_eq!(cmp.verdict, Verdict::NotComparable);
    assert!(
        cmp.reasons[0].contains("contract config differs"),
        "{:?}",
        cmp.reasons
    );
}

#[test]
fn invalid_arm_is_not_comparable() {
    let base = write_run(
        "base",
        &good_contract(),
        &rows("base", 20, &SEEDS, base_correct),
        None,
    );
    // Candidate is missing a seed: INCOMPLETE, so it cannot enter a comparison.
    let current = write_run(
        "current",
        &good_contract(),
        &rows("current", 20, &[0, 1, 2, 3], |_, _| true),
        None,
    );
    let cmp = compare_dirs(&base, &current);
    assert_eq!(cmp.verdict, Verdict::NotComparable);
    assert!(
        cmp.reasons[0].contains("candidate run is INCOMPLETE"),
        "{:?}",
        cmp.reasons
    );
}

#[test]
fn lower_better_scalar_improvement_is_supported() {
    let kld = |arm: &str, v: fn(u32, i64) -> f64| {
        let mut out = rows(arm, 20, &SEEDS, |_, _| true);
        for r in &mut out {
            let item: u32 = r.item_id.parse().unwrap();
            r.correct = None;
            r.value = Some(v(item, r.seed));
        }
        out
    };
    let base = write_run(
        "base",
        &kld_contract(),
        &kld("base", |i, _| 0.05 + f64::from(i) * 1e-3),
        None,
    );
    let current = write_run(
        "current",
        &kld_contract(),
        &kld("current", |i, s| {
            0.03 + f64::from(i) * 1e-3 + s as f64 * 1e-4
        }),
        None,
    );
    let cmp = compare_dirs(&base, &current);
    assert_eq!(cmp.verdict, Verdict::Supported, "{:?}", cmp.reasons);
    assert!(
        cmp.delta.unwrap() < 0.0,
        "KLD went down, which is the good direction"
    );
    assert_eq!(cmp.test.as_ref().unwrap().name, "sign_test_exact");
}
