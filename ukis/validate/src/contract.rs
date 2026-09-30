//! `contract.json`: the preregistration, written BEFORE compute and frozen by its hash.
//!
//! Parsing is two-step on purpose: serde reads every field as optional (`RawContract`) so one
//! pass can list ALL problems at once, then `Contract::from_value` either returns a fully typed
//! contract or the complete list of refusals. Downstream code never sees a half-valid contract.

use serde::Deserialize;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;

pub const SCHEMA: &str = "ukis.contract/v1";
/// Name of the lock file written next to the contract at launch time.
pub const LOCK_FILE: &str = "contract.sha256";
/// Fewer seeds than this can never back a claim (experiment skill R3, rule 11).
pub const MIN_CLAIM_SEEDS: usize = 5;

/// Revision strings that move under you. A number measured against them cannot be reproduced,
/// so they are allowed for smoke runs only (Benchmark_configs README: "Empty = latest, smoke only").
const FLOATING: &[&str] = &[
    "",
    "main",
    "master",
    "head",
    "latest",
    "none",
    "null",
    "to_pin_before_run",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Smoke,
    Pilot,
    Confirmation,
    Final,
}

impl Tier {
    fn parse(s: &str) -> Option<Tier> {
        match s {
            "smoke" => Some(Tier::Smoke),
            "pilot" => Some(Tier::Pilot),
            "confirmation" => Some(Tier::Confirmation),
            "final" => Some(Tier::Final),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Smoke => "smoke",
            Tier::Pilot => "pilot",
            Tier::Confirmation => "confirmation",
            Tier::Final => "final",
        }
    }

    /// Only confirmation and final runs may back a SUPPORTED claim; pilots are directional.
    pub fn can_claim(self) -> bool {
        matches!(self, Tier::Confirmation | Tier::Final)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    HigherBetter,
    LowerBetter,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::HigherBetter => "higher_better",
            Direction::LowerBetter => "lower_better",
        }
    }

    /// +1 when a larger number is an improvement, -1 when smaller is. Multiplying a raw delta
    /// by this gives "improvement", so one code path serves accuracy, tok/s and KLD alike.
    pub fn sign(self) -> f64 {
        match self {
            Direction::HigherBetter => 1.0,
            Direction::LowerBetter => -1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    /// Per-row `correct` (0/1), reported in percent. Supports Wilson and McNemar.
    Accuracy,
    /// Per-row number (`value` or `values.<name>`): score, tok/s, KLD, latency...
    Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Baseline,
    Prior,
    Treatment,
    Control,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Baseline => "baseline",
            Role::Prior => "prior",
            Role::Treatment => "treatment",
            Role::Control => "control",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    pub name: String,
    pub unit: String,
    pub direction: Direction,
    pub kind: MetricKind,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    pub name: String,
    pub role: Role,
}

/// A contract that passed every launch refusal.
#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub tier: Tier,
    pub family: String,
    /// Display name for the canonical table; defaults to `family`.
    pub benchmark: String,
    pub n_items: u64,
    pub item_ids_sha256: Option<String>,
    pub seeds: Vec<i64>,
    pub metrics: Vec<Metric>,
    pub arms: Vec<Arm>,
    /// Which token count the "thinking" columns use; units must be named (rule 21).
    pub thinking_tokens: Option<String>,
    /// sha256 of the canonical JSON of the whole contract.
    pub sha256: String,
    /// sha256 of only the fields that must match for two runs to be comparable (C1).
    pub fingerprint: String,
}

impl Contract {
    pub fn primary(&self) -> &Metric {
        // from_value guarantees exactly one primary metric.
        self.metrics
            .iter()
            .find(|m| m.primary)
            .expect("validated: one primary metric")
    }

    pub fn arm(&self, name: &str) -> Option<&Arm> {
        self.arms.iter().find(|a| a.name == name)
    }
}

#[derive(Deserialize, Default)]
struct RawContract {
    schema: Option<String>,
    tier: Option<String>,
    question: Option<String>,
    decision: Option<RawDecision>,
    falsify: Option<String>,
    family: Option<String>,
    benchmark: Option<String>,
    command: Option<RawCommand>,
    dataset: Option<RawDataset>,
    grader: Option<RawGrader>,
    max_output_tokens: Option<u64>,
    seeds: Option<Vec<i64>>,
    metrics: Option<Vec<RawMetric>>,
    arms: Option<Vec<RawArm>>,
    controls: Option<Vec<RawControl>>,
    thinking_tokens: Option<String>,
}

#[derive(Deserialize)]
struct RawDecision {
    #[serde(rename = "if")]
    if_: Option<String>,
    then: Option<String>,
    #[serde(rename = "else")]
    else_: Option<String>,
}

#[derive(Deserialize)]
struct RawCommand {
    argv: Option<Vec<String>>,
    runner_commit: Option<String>,
}

#[derive(Deserialize)]
struct RawDataset {
    id: Option<String>,
    revision: Option<String>,
    n_items: Option<u64>,
    item_ids_sha256: Option<String>,
}

#[derive(Deserialize)]
struct RawGrader {
    id: Option<String>,
    revision: Option<String>,
}

#[derive(Deserialize)]
struct RawMetric {
    name: Option<String>,
    definition: Option<String>,
    unit: Option<String>,
    direction: Option<String>,
    kind: Option<String>,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct RawArm {
    name: Option<String>,
    role: Option<String>,
}

#[derive(Deserialize)]
struct RawControl {
    arm: Option<String>,
    rules_out: Option<String>,
}

/// Canonical JSON: serde_json's default map is a BTreeMap, so keys come out sorted and the
/// output is compact. Whitespace or key-order edits do not change the hash; any value edit does.
pub fn canonical_sha256(value: &Value) -> String {
    sha256_hex(value.to_string().as_bytes())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Fields that define "the same measurement". Anything here differing between two runs means
/// a different family (rule 14): the delta between them would mix two protocols.
const FINGERPRINT_FIELDS: &[&str] = &[
    "family",
    "dataset",
    "grader",
    "sampler",
    "max_output_tokens",
    "timeout_s",
    "seeds",
    "metrics",
    "thinking_tokens",
];

fn fingerprint(value: &Value) -> String {
    let mut picked = serde_json::Map::new();
    for key in FINGERPRINT_FIELDS {
        picked.insert(
            (*key).to_string(),
            value.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    // The runner code is part of the protocol too (grading, prompt, extraction).
    let runner = value
        .pointer("/command/runner_commit")
        .cloned()
        .unwrap_or(Value::Null);
    picked.insert("runner_commit".to_string(), runner);
    canonical_sha256(&Value::Object(picked))
}

fn check_revision(
    refusals: &mut Vec<String>,
    name: &str,
    rev: Option<&str>,
    required: bool,
    pinned: bool,
) {
    match rev {
        None if required => refusals.push(format!("missing {name}")),
        Some(r) if pinned && is_floating(r) => {
            refusals.push(format!(
                "{name} \"{r}\" is floating; pin it (floating is allowed for smoke only)"
            ));
        }
        _ => {}
    }
}

fn is_floating(revision: &str) -> bool {
    FLOATING.contains(&revision.trim().to_ascii_lowercase().as_str())
}

fn text(field: &Option<String>) -> Option<&str> {
    field.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

impl Contract {
    /// Every launch refusal from study/E section 3.1. Returns all problems, not just the first,
    /// so one edit round fixes the whole contract.
    pub fn from_value(value: &Value) -> Result<Contract, Vec<String>> {
        let raw: RawContract = match serde_json::from_value(value.clone()) {
            Ok(raw) => raw,
            Err(e) => return Err(vec![format!("contract is not well-formed: {e}")]),
        };
        let mut refusals: Vec<String> = Vec::new();

        if raw.schema.as_deref() != Some(SCHEMA) {
            refusals.push(format!("schema must be \"{SCHEMA}\""));
        }
        let tier = raw.tier.as_deref().and_then(Tier::parse);
        if tier.is_none() {
            refusals.push("tier must be one of smoke, pilot, confirmation, final".to_string());
        }
        // Preregistration: the question, both decision branches and the falsifier are written
        // before the numbers exist, so the numbers cannot shape them (experiment R1).
        for (name, field) in [
            ("question", &raw.question),
            ("falsify", &raw.falsify),
            ("family", &raw.family),
        ] {
            if text(field).is_none() {
                refusals.push(format!("missing {name}"));
            }
        }
        let decided = raw.decision.as_ref().is_some_and(|d| {
            text(&d.if_).is_some() && text(&d.then).is_some() && text(&d.else_).is_some()
        });
        if !decided {
            refusals.push(
                "decision needs all of if / then / else (both branches decided up front)"
                    .to_string(),
            );
        }
        let argv_ok = raw
            .command
            .as_ref()
            .and_then(|c| c.argv.as_ref())
            .is_some_and(|a| !a.is_empty());
        if !argv_ok {
            refusals.push(
                "missing command.argv (the exact command is the provenance of every number)"
                    .to_string(),
            );
        }

        // Pinned revisions: floating = smoke only (rule 4). A missing dataset revision is refused
        // at every tier; the runner commit may be missing only for smoke.
        let non_smoke = tier.is_some_and(|t| t != Tier::Smoke);
        let dataset = raw.dataset.as_ref();
        check_revision(
            &mut refusals,
            "dataset.revision",
            dataset.and_then(|d| text(&d.revision)),
            /*required*/ true,
            non_smoke,
        );
        let runner_commit = raw.command.as_ref().and_then(|c| text(&c.runner_commit));
        check_revision(
            &mut refusals,
            "command.runner_commit",
            runner_commit,
            non_smoke,
            non_smoke,
        );
        if let Some(grader) = &raw.grader {
            check_revision(
                &mut refusals,
                "grader.revision",
                text(&grader.revision),
                /*required*/ true,
                non_smoke,
            );
            if text(&grader.id).is_none() {
                refusals.push("missing grader.id".to_string());
            }
        }
        if dataset.and_then(|d| text(&d.id)).is_none() {
            refusals.push("missing dataset.id".to_string());
        }
        let n_items = dataset.and_then(|d| d.n_items).filter(|n| *n > 0);
        if n_items.is_none() {
            refusals.push(
                "dataset.n_items must be a positive integer (the frozen item count)".to_string(),
            );
        }

        let mut seeds = raw.seeds.clone().unwrap_or_default();
        seeds.sort_unstable();
        let declared = seeds.len();
        seeds.dedup();
        if seeds.is_empty() {
            refusals.push("missing seeds".to_string());
        } else if seeds.len() != declared {
            refusals.push("seeds contain duplicates".to_string());
        }
        if tier.is_some_and(Tier::can_claim) && seeds.len() < MIN_CLAIM_SEEDS {
            refusals.push(format!(
                "tier {} needs >= {MIN_CLAIM_SEEDS} seeds, contract has {}",
                tier.map(Tier::as_str).unwrap_or_default(),
                seeds.len()
            ));
        }

        let metrics = parse_metrics(raw.metrics.as_deref(), &mut refusals);
        // The output cap changes accuracy (truncation scores wrong), so it is part of the protocol.
        if metrics.iter().any(|m| m.kind == MetricKind::Accuracy) && raw.max_output_tokens.is_none()
        {
            refusals
                .push("missing max_output_tokens (required with an accuracy metric)".to_string());
        }
        let arms = parse_arms(raw.arms.as_deref(), &mut refusals);
        parse_controls(raw.controls.as_deref(), &arms, &mut refusals);
        if let Some(t) = raw.thinking_tokens.as_deref()
            && t != "completion_tokens"
            && t != "reasoning_tokens"
        {
            refusals.push(format!(
                "thinking_tokens \"{t}\" must be completion_tokens or reasoning_tokens"
            ));
        }

        if !refusals.is_empty() {
            return Err(refusals);
        }
        let family = text(&raw.family).unwrap_or_default().to_string();
        Ok(Contract {
            tier: tier.expect("checked above"),
            benchmark: text(&raw.benchmark).map_or_else(|| family.clone(), str::to_string),
            family,
            n_items: n_items.expect("checked above"),
            item_ids_sha256: dataset
                .and_then(|d| text(&d.item_ids_sha256))
                .map(str::to_string),
            seeds,
            metrics,
            arms,
            thinking_tokens: raw.thinking_tokens.clone(),
            sha256: canonical_sha256(value),
            fingerprint: fingerprint(value),
        })
    }
}

/// Read and parse `contract.json` (a directory means `<dir>/contract.json`).
pub fn load(path: &Path) -> Result<(Value, Result<Contract, Vec<String>>), String> {
    let path = contract_path(path);
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
    let parsed = Contract::from_value(&value);
    Ok((value, parsed))
}

/// `check-contract`: refuse, or lock the contract by writing its hash next to it. The lock is
/// written once; if it already exists it must match, because a contract edited after launch
/// could be tuned to the numbers it was supposed to predict.
pub fn check_and_lock(path: &Path) -> Result<Contract, Vec<String>> {
    let (value, parsed) = load(path).map_err(|e| vec![e])?;
    let contract = parsed?;
    let file = contract_path(path);
    let lock = file.parent().unwrap_or(Path::new(".")).join(LOCK_FILE);
    match std::fs::read_to_string(&lock) {
        Ok(locked) if locked.trim() == contract.sha256 => Ok(contract),
        Ok(locked) => Err(vec![format!(
            "contract edited after lock: {} says {}, contract is now {}",
            lock.display(),
            locked.trim(),
            canonical_sha256(&value)
        )]),
        Err(_) => {
            std::fs::write(&lock, format!("{}\n", contract.sha256))
                .map_err(|e| vec![format!("cannot write {}: {e}", lock.display())])?;
            Ok(contract)
        }
    }
}

pub fn contract_path(path: &Path) -> std::path::PathBuf {
    if path.is_dir() {
        path.join("contract.json")
    } else {
        path.to_path_buf()
    }
}

mod fields;
use fields::parse_arms;
use fields::parse_controls;
use fields::parse_metrics;

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;
