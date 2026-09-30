use super::*;
use crate::test_support::good_contract;
use crate::test_support::temp_dir;
use serde_json::json;

fn refusals(v: &Value) -> Vec<String> {
    Contract::from_value(v).expect_err("expected refusal")
}

fn has(refusals: &[String], needle: &str) -> bool {
    refusals.iter().any(|r| r.contains(needle))
}

#[test]
fn good_contract_is_accepted() {
    let c = Contract::from_value(&good_contract()).expect("valid");
    assert_eq!(c.tier, Tier::Confirmation);
    assert_eq!(c.seeds, vec![0, 1, 2, 3, 4]);
    assert_eq!(c.primary().name, "accuracy");
    assert_eq!(c.benchmark, "Toy");
}

#[test]
fn hash_ignores_formatting_but_not_values() {
    let a: Value = serde_json::from_str(r#"{"b": 1, "a": [1, 2]}"#).unwrap();
    let b: Value = serde_json::from_str("{\n  \"a\":[1,2],\"b\":1}").unwrap();
    let c: Value = serde_json::from_str(r#"{"b": 2, "a": [1, 2]}"#).unwrap();
    assert_eq!(canonical_sha256(&a), canonical_sha256(&b));
    assert_ne!(canonical_sha256(&a), canonical_sha256(&c));
}

#[test]
fn empty_contract_lists_every_missing_field() {
    let r = refusals(&json!({}));
    for needle in [
        "schema",
        "tier",
        "missing question",
        "missing falsify",
        "missing family",
        "decision",
        "command.argv",
        "dataset.revision",
        "dataset.id",
        "n_items",
        "missing seeds",
        "missing metrics",
        "no baseline arm",
        "missing controls",
    ] {
        assert!(
            has(&r, needle),
            "no refusal mentioning {needle:?} in {r:#?}"
        );
    }
}

#[test]
fn decision_needs_both_branches() {
    let mut c = good_contract();
    c["decision"] = json!({"if": "x", "then": "promote"});
    assert!(has(&refusals(&c), "decision"));
}

#[test]
fn floating_revision_is_smoke_only() {
    let mut c = good_contract();
    c["dataset"]["revision"] = json!("main");
    c["tier"] = json!("pilot");
    c["seeds"] = json!([0, 1, 2]);
    assert!(has(&refusals(&c), "dataset.revision \"main\" is floating"));

    c["tier"] = json!("smoke");
    assert!(Contract::from_value(&c).is_ok(), "smoke may float");

    // Missing is refused even for smoke: "which data" must always be written down.
    c.as_object_mut()
        .unwrap()
        .get_mut("dataset")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("revision");
    assert!(has(&refusals(&c), "missing dataset.revision"));
}

#[test]
fn unpinned_runner_commit_refused_outside_smoke() {
    let mut c = good_contract();
    c["command"]["runner_commit"] = json!("TO_PIN_BEFORE_RUN");
    assert!(has(&refusals(&c), "command.runner_commit"));
}

#[test]
fn confirmation_with_three_seeds_is_refused() {
    let mut c = good_contract();
    c["seeds"] = json!([0, 1, 2]);
    assert!(has(&refusals(&c), "needs >= 5 seeds"));
    c["tier"] = json!("final");
    assert!(has(&refusals(&c), "needs >= 5 seeds"));
    c["tier"] = json!("pilot");
    assert!(Contract::from_value(&c).is_ok(), "pilots may run 2-3 seeds");
}

#[test]
fn duplicate_seeds_refused() {
    let mut c = good_contract();
    c["seeds"] = json!([0, 1, 2, 3, 3]);
    assert!(has(&refusals(&c), "duplicates"));
}

#[test]
fn metric_without_unit_or_direction_refused() {
    let mut c = good_contract();
    c["metrics"] = json!([{"name": "tok_s", "definition": "decode tokens per second", "kind": "scalar", "primary": true}]);
    let r = refusals(&c);
    assert!(has(&r, "metric tok_s: missing unit"));
    assert!(has(&r, "metric tok_s: direction"));

    c["metrics"] = json!([{"definition": "x", "unit": "tok/s", "direction": "higher_better", "kind": "scalar", "primary": true}]);
    assert!(has(&refusals(&c), "metrics[0]: missing name"));
}

#[test]
fn accuracy_cannot_be_lower_better() {
    let mut c = good_contract();
    c["metrics"][0]["direction"] = json!("lower_better");
    assert!(has(&refusals(&c), "accuracy must be higher_better"));
}

#[test]
fn exactly_one_primary_metric() {
    let mut c = good_contract();
    c["metrics"][0]["primary"] = json!(false);
    assert!(has(&refusals(&c), "exactly one metric must be primary"));
}

#[test]
fn no_baseline_arm_refused() {
    let mut c = good_contract();
    c["arms"] = json!([{"name": "current", "role": "treatment"}]);
    c["controls"] = json!([{"arm": "current", "rules_out": "x"}]);
    assert!(has(&refusals(&c), "no baseline arm"));
}

#[test]
fn lock_is_written_once_and_detects_edits() {
    let dir = temp_dir("lock");
    let mut c = good_contract();
    std::fs::write(dir.join("contract.json"), c.to_string()).unwrap();
    let first = check_and_lock(&dir).expect("valid contract locks");
    let locked = std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap();
    assert_eq!(locked.trim(), first.sha256);
    // Re-checking the same contract is fine.
    assert!(check_and_lock(&dir).is_ok());
    // Editing it after the lock (e.g. lowering the bar after seeing numbers) is refused.
    c["falsify"] = json!("something easier");
    std::fs::write(dir.join("contract.json"), c.to_string()).unwrap();
    let err = check_and_lock(&dir).expect_err("edited after lock");
    assert!(err[0].contains("edited after lock"), "{err:?}");
}
