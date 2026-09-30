//! `items.jsonl`: the evidence. One line per arm x item x seed, written by the launched command
//! or by a thin adapter that reads only that command's own output files. Never by the model.

use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Error,
    Timeout,
    Empty,
    Truncated,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Error => "error",
            Status::Timeout => "timeout",
            Status::Empty => "empty",
            Status::Truncated => "truncated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemRow {
    pub arm: String,
    pub item_id: String,
    pub seed: i64,
    pub status: Status,
    /// Accuracy metrics read this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correct: Option<bool>,
    /// Scalar metrics read `values[<metric name>]` first, then this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<BTreeMap<String, f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    /// Where the raw generation lives and its hash, so a number traces back to a file (V8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_sha256: Option<String>,
}

impl ItemRow {
    /// The only definition of "counted correct": an ok row that says correct. Error, timeout,
    /// empty and truncated rows are WRONG and stay in the denominator (rule 17, V4), whatever
    /// their `correct` field claims.
    pub fn scored_correct(&self) -> bool {
        self.status == Status::Ok && self.correct == Some(true)
    }

    pub fn scalar(&self, metric: &str) -> Option<f64> {
        self.values
            .as_ref()
            .and_then(|v| v.get(metric).copied())
            .or(self.value)
    }

    pub fn tokens(&self, field: &str) -> Option<u64> {
        match field {
            "completion_tokens" => self.completion_tokens,
            "reasoning_tokens" => self.reasoning_tokens,
            _ => None,
        }
    }
}

/// Parse JSONL, reporting every bad line (1-based) instead of stopping at the first.
pub fn parse(text: &str) -> Result<Vec<ItemRow>, Vec<String>> {
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ItemRow>(line) {
            Ok(row) => rows.push(row),
            Err(e) => errors.push(format!("items.jsonl line {}: {e}", i + 1)),
        }
    }
    if errors.is_empty() {
        Ok(rows)
    } else {
        Err(errors)
    }
}
