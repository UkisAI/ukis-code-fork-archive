//! Pure claim detection for the final-answer gate.
//!
//! A sentence is a numeric performance claim when it contains a percentage, or a metric
//! word together with a decimal number. A claim is backed only when the same sentence
//! carries an evidence tag `[run:<id>]`. Detection is deliberately conservative (spike):
//! integers next to metric words are ignored so model names such as `Swift-7B` and
//! `pass@1` do not trigger the gate.

use std::sync::LazyLock;

use regex_lite::Regex;

/// Corrections injected per turn before the gate gives up and only warns.
pub const MAX_GATE_CORRECTIONS_PER_TURN: u32 = 2;

/// Hard cap on quoted claims in one correction, keeps the injected item bounded.
const MAX_QUOTED_CLAIMS: usize = 5;
/// Hard cap on the characters quoted per claim.
const MAX_CLAIM_CHARS: usize = 160;

static SENTENCE_SPLIT: LazyLock<Regex> = LazyLock::new(|| compile(r"[.!?](?:\s+|$)|\n"));
static PERCENT: LazyLock<Regex> = LazyLock::new(|| compile(r"\d(?:\s*%|\s*percent\b)"));
static METRIC_WORD: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?i)\b(?:accuracy|score|scored|scores|f1|bleu|rouge|exact match|win rate|perplexity|pass rate)\b",
    )
});
static DECIMAL: LazyLock<Regex> = LazyLock::new(|| compile(r"\b\d+\.\d+\b"));
static EVIDENCE_TAG: LazyLock<Regex> = LazyLock::new(|| compile(r"\[run:[A-Za-z0-9._-]+\]"));

fn compile(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(regex) => regex,
        Err(err) => panic!("claim gate pattern {pattern:?} is invalid: {err}"),
    }
}

/// Returns the claim sentences in `text` that state a performance number without an
/// evidence tag, trimmed and capped for quoting.
pub(crate) fn unbacked_claims(text: &str) -> Vec<String> {
    SENTENCE_SPLIT
        .split(text)
        .map(str::trim)
        .filter(|sentence| is_numeric_claim(sentence) && !EVIDENCE_TAG.is_match(sentence))
        .take(MAX_QUOTED_CLAIMS)
        .map(|sentence| sentence.chars().take(MAX_CLAIM_CHARS).collect())
        .collect()
}

fn is_numeric_claim(sentence: &str) -> bool {
    PERCENT.is_match(sentence) || (METRIC_WORD.is_match(sentence) && DECIMAL.is_match(sentence))
}

/// Model-visible correction listing the unbacked claims.
pub(crate) fn correction_text(claims: &[String]) -> String {
    let quoted = claims
        .iter()
        .map(|claim| format!("- \"{claim}\""))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Research gate: your final answer states performance numbers that are not backed by a validated run:\n{quoted}\n\nNumbers must come from a validated run. For each number either cite the run that produced it as [run:<id>], or say plainly that the number is unknown. Do not guess, estimate or repeat an unbacked number. Write your final answer again now."
    )
}

#[cfg(test)]
#[path = "claim_gate_tests.rs"]
mod tests;
