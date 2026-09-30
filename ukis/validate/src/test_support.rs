//! Shared builders for unit tests: a known-good contract and run dirs written to a temp dir.

use crate::contract;
use crate::items::ItemRow;
use crate::items::Status;
use serde_json::Value;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

pub fn good_contract() -> Value {
    json!({
        "schema": "ukis.contract/v1",
        "tier": "confirmation",
        "question": "Does current beat base on toy accuracy?",
        "decision": {"if": "verdict == SUPPORTED", "then": "promote current", "else": "keep base"},
        "falsify": "no significant gain",
        "family": "toy/xhigh_100k",
        "benchmark": "Toy",
        "command": {"argv": ["python", "toy.py"], "runner_commit": "8f748ee"},
        "dataset": {"id": "toy", "revision": "633f5ee8", "n_items": 20},
        "sampler": {"temperature": 1.0},
        "max_output_tokens": 100000,
        "seeds": [0, 1, 2, 3, 4],
        "metrics": [{"name": "accuracy", "definition": "correct / rows, failures wrong",
                     "unit": "percent", "direction": "higher_better", "kind": "accuracy", "primary": true}],
        "arms": [{"name": "base", "role": "baseline"}, {"name": "current", "role": "treatment"}],
        "controls": [{"arm": "base", "rules_out": "effect exists without the change"}],
        "thinking_tokens": "completion_tokens"
    })
}

/// A scalar-metric contract (e.g. KLD per prompt), lower is better.
pub fn kld_contract() -> Value {
    let mut c = good_contract();
    c["family"] = json!("toy/kld");
    c["metrics"] = json!([{"name": "kld", "definition": "mean KL divergence vs BF16 per prompt",
                           "unit": "nats", "direction": "lower_better", "kind": "scalar", "primary": true}]);
    c.as_object_mut().map(|o| o.remove("max_output_tokens"));
    c
}

pub fn row(arm: &str, item: u32, seed: i64, status: Status, correct: bool) -> ItemRow {
    ItemRow {
        arm: arm.to_string(),
        item_id: format!("{item:03}"),
        seed,
        status,
        correct: Some(correct),
        value: None,
        values: None,
        completion_tokens: Some(1000),
        reasoning_tokens: None,
        raw_ref: None,
        raw_sha256: None,
    }
}

/// Rows for every item x seed with `correct(item, seed)`.
pub fn rows(
    arm: &str,
    n_items: u32,
    seeds: &[i64],
    correct: impl Fn(u32, i64) -> bool,
) -> Vec<ItemRow> {
    let mut out = Vec::new();
    for &seed in seeds {
        for item in 0..n_items {
            out.push(row(arm, item, seed, Status::Ok, correct(item, seed)));
        }
    }
    out
}

pub fn temp_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ukis-validate-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Write contract + lock + items (and an optional runner summary) as a launched run would.
pub fn write_run(
    name: &str,
    contract_value: &Value,
    rows: &[ItemRow],
    summary: Option<Value>,
) -> PathBuf {
    let dir = temp_dir(name);
    std::fs::write(
        dir.join("contract.json"),
        serde_json::to_string_pretty(contract_value).unwrap(),
    )
    .unwrap();
    contract::check_and_lock(&dir).expect("test contract must be valid");
    let text: String = rows
        .iter()
        .map(|r| serde_json::to_string(r).unwrap() + "\n")
        .collect();
    std::fs::write(dir.join("items.jsonl"), text).unwrap();
    if let Some(s) = summary {
        std::fs::write(dir.join("runner_summary.json"), s.to_string()).unwrap();
    }
    dir
}
