//! `import benchmark-configs <out_dir> --arm <name>`: thin adapter from a UkisAI/Benchmark_configs
//! run dir (`raw/s<seed>_<id>.json` + `score_<bench>.json`) to `items.jsonl`.
//!
//! Field names come from the writer code (`benchmarks/common.py` run()/chat()/report()), since no
//! real run outputs exist on this machine. Raw rows carry: key, content, reasoning, finish_reason,
//! prompt_tokens, completion_tokens, seconds, error (only on failure), seed, gold, pred.
//!
//! `correct` is NOT stored in raw/ by the runner: `report()` computes it in memory. So this
//! adapter uses, in order: a `correct` field in the raw row (a patched runner), the grader
//! outputs the runner left on disk (IFBench eval_s<seed>/), or a port of the few one-line
//! graders that are pure string compares (GPQA, C-Eval, MMLU-Pro, ERQA, AIME). Anything else
//! (HMMT sympy, LiveCodeBench execution) is refused rather than re-implemented.

use crate::contract::sha256_hex;
use crate::items::ItemRow;
use crate::items::Status;
use crate::results::ITEMS_FILE;
use crate::results::RUNNER_SUMMARY_FILE;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub struct ImportReport {
    pub rows: usize,
    pub bench: String,
    pub summary_copied: Option<String>,
}

pub fn import(
    out_dir: &Path,
    arm: &str,
    bench: Option<&str>,
    into: &Path,
) -> Result<ImportReport, String> {
    let bench = match bench {
        Some(b) => b.to_string(),
        None => detect_bench(out_dir)?,
    };
    let ifbench = load_ifbench_grades(out_dir, &bench)?;
    let raw_dir = out_dir.join("raw");
    let mut files: Vec<_> = std::fs::read_dir(&raw_dir)
        .map_err(|e| format!("cannot read {}: {e}", raw_dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json")) // skip *.tmp partial writes
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("{} has no raw/*.json files", out_dir.display()));
    }

    let mut rows = Vec::new();
    for path in &files {
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let x: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let (seed, item_id) = parse_key(stem)
            .ok_or_else(|| format!("{}: key is not s<seed>_<id>", path.display()))?;
        if x.get("seed")
            .and_then(Value::as_i64)
            .is_some_and(|s| s != seed)
        {
            return Err(format!(
                "{}: seed field disagrees with the file name",
                path.display()
            ));
        }
        let correct = match x.get("correct").and_then(Value::as_bool) {
            Some(c) => c,
            None => grade(&bench, &x, ifbench.as_ref(), seed)
                .map_err(|e| format!("{}: {e}", path.display()))?,
        };
        let rel = format!(
            "raw/{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
        );
        rows.push(ItemRow {
            arm: arm.to_string(),
            item_id,
            seed,
            status: status_of(&x),
            correct: Some(correct),
            value: None,
            values: None,
            completion_tokens: x.get("completion_tokens").and_then(Value::as_u64),
            reasoning_tokens: None, // the runner stores only completion_tokens (thinking + answer)
            raw_ref: Some(rel),
            raw_sha256: Some(sha256_hex(&bytes)),
        });
    }
    rows.sort_by(|a, b| (a.seed, &a.item_id).cmp(&(b.seed, &b.item_id)));

    std::fs::create_dir_all(into).map_err(|e| format!("cannot create {}: {e}", into.display()))?;
    let mut text = String::new();
    for row in &rows {
        text.push_str(&serde_json::to_string(row).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    std::fs::write(into.join(ITEMS_FILE), text)
        .map_err(|e| format!("cannot write items.jsonl: {e}"))?;

    // Copy the runner's own summary byte for byte; `run` checks it against the recomputation.
    let score = out_dir.join(format!("score_{bench}.json"));
    let summary_copied = if score.exists() {
        std::fs::copy(&score, into.join(RUNNER_SUMMARY_FILE))
            .map_err(|e| format!("cannot copy summary: {e}"))?;
        Some(score.display().to_string())
    } else {
        None
    };
    Ok(ImportReport {
        rows: rows.len(),
        bench,
        summary_copied,
    })
}

fn detect_bench(out_dir: &Path) -> Result<String, String> {
    let names: Vec<String> = std::fs::read_dir(out_dir)
        .map_err(|e| format!("cannot read {}: {e}", out_dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter_map(|n| {
            n.strip_prefix("score_")
                .and_then(|n| n.strip_suffix(".json"))
                .map(str::to_string)
        })
        .collect();
    match names.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err("no score_<bench>.json found; pass --bench".to_string()),
        many => Err(format!(
            "several score files {many:?} (IFBench writes strict + loose); pass --bench"
        )),
    }
}

/// `s0_017` -> (0, "017"). The id may itself contain underscores, so split at the first one.
fn parse_key(stem: &str) -> Option<(i64, String)> {
    let (seed, id) = stem.strip_prefix('s')?.split_once('_')?;
    Some((seed.parse().ok()?, id.to_string()))
}

fn text<'a>(x: &'a Value, key: &str) -> &'a str {
    x.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Python truthiness of `x.get("error")`: missing, null and "" are all "no error".
fn has_error(x: &Value) -> bool {
    match x.get("error") {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn status_of(x: &Value) -> Status {
    if has_error(x) {
        let e = text(x, "error").to_ascii_lowercase();
        if e.contains("timeout") || e.contains("timed out") {
            Status::Timeout
        } else {
            Status::Error
        }
    } else if text(x, "finish_reason") == "length" {
        Status::Truncated
    } else if text(x, "content").trim().is_empty() {
        Status::Empty
    } else {
        Status::Ok
    }
}

/// `common.ok()`: finished cleanly with visible content.
fn runner_ok(x: &Value) -> bool {
    !has_error(x) && text(x, "finish_reason") != "length" && !text(x, "content").trim().is_empty()
}

fn pred_eq_gold(x: &Value) -> bool {
    match (
        x.get("pred").and_then(Value::as_str),
        x.get("gold").and_then(Value::as_str),
    ) {
        (Some(p), Some(g)) => p == g,
        _ => false,
    }
}

/// Ports of the runner's own one-line graders, faithful even where they are lenient, so the
/// runner summary check compares like with like. Where a runner grader counts a truncated row
/// correct (C-Eval, MMLU-Pro, ERQA), `run` still scores it wrong (rule 17) and the summary
/// check will flag the disagreement as INVALID: that is the rule working, not a bug.
fn grade(
    bench: &str,
    x: &Value,
    ifbench: Option<&IfbenchGrades>,
    seed: i64,
) -> Result<bool, String> {
    match bench {
        "gpqa" => Ok(runner_ok(x) && pred_eq_gold(x)),
        "ceval" | "mmlu_pro" => Ok(!has_error(x) && pred_eq_gold(x)),
        "erqa" => {
            let content = text(x, "content").replace('.', "").trim().to_lowercase();
            Ok(content == text(x, "gold").trim().to_lowercase())
        }
        "aime_2024" | "aime_2025" => {
            let pred = x.get("pred").and_then(Value::as_str);
            let gold = text(x, "gold").trim_start_matches('0');
            Ok(runner_ok(x) && pred.is_some_and(|p| p.trim().trim_start_matches('0') == gold))
        }
        "ifbench_strict" | "ifbench_loose" => {
            let grades = ifbench.ok_or("IFBench grades not loaded")?;
            let follow = grades
                .get(&seed)
                .and_then(|g| g.get(text(x, "prompt")))
                .copied();
            let follow = follow.ok_or("prompt not found in eval_s<seed> results")?;
            Ok(runner_ok(x) && follow)
        }
        other => Err(format!(
            "no `correct` in raw and no faithful port of the {other} grader (needs sympy / code execution); patch the runner to write `correct` into raw/"
        )),
    }
}

/// seed -> prompt -> follow_all_instructions, from the runner's own `eval_s<seed>/` outputs.
type IfbenchGrades = BTreeMap<i64, BTreeMap<String, bool>>;

fn load_ifbench_grades(out_dir: &Path, bench: &str) -> Result<Option<IfbenchGrades>, String> {
    let Some(mode) = bench.strip_prefix("ifbench_") else {
        return Ok(None);
    };
    let mut grades = IfbenchGrades::new();
    for entry in std::fs::read_dir(out_dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(seed) = name
            .strip_prefix("eval_s")
            .and_then(|s| s.parse::<i64>().ok())
        else {
            continue;
        };
        let suffix = format!("eval_results_{mode}.jsonl");
        let file = std::fs::read_dir(entry.path())
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .find(|f| f.file_name().to_string_lossy().ends_with(&suffix))
            .ok_or_else(|| format!("{name}: no *{suffix}"))?;
        let body = std::fs::read_to_string(file.path()).map_err(|e| e.to_string())?;
        let per_seed = grades.entry(seed).or_default();
        for line in body.lines().filter(|l| !l.trim().is_empty()) {
            let g: Value = serde_json::from_str(line).map_err(|e| format!("{name}: {e}"))?;
            let follow = g
                .get("follow_all_instructions")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            per_seed.insert(text(&g, "prompt").to_string(), follow);
        }
    }
    Ok(Some(grades))
}

#[cfg(test)]
#[path = "import_bc_tests.rs"]
mod tests;
