//! `table <inputs...>`: the canonical posttraining table (study/E section 4), rendered only from
//! recomputed VALID runs. Inputs are run dirs, results.json or comparison.json files; a file is
//! accepted only if recomputing it from its run dirs reproduces it exactly, so a number typed
//! into a JSON file by a user or a model is refused instead of rendered (R1).

use crate::compare;
use crate::compare::Comparison;
use crate::compare::Verdict;
use crate::contract::MetricKind;
use crate::contract::Role;
use crate::contract::Tier;
use crate::results;
use crate::results::RunEval;
use crate::results::RunStatus;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;

// Header and alignment row are the canonical format, verbatim.
const HEADER: &str = "| Benchmark | Base accuracy | Prior model accuracy | Current model accuracy | Current vs base | Mean thinking vs base | Median thinking vs base |";
const ALIGN: &str = "|---|---:|---:|---:|---:|---:|---:|";

/// Resolve every input to run dirs, refusing any file that does not match its recomputation.
fn resolve(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for input in inputs {
        let found = if input.is_dir() {
            vec![input.clone()]
        } else {
            let text = std::fs::read_to_string(input)
                .map_err(|e| format!("cannot read {}: {e}", input.display()))?;
            let stated: Value = serde_json::from_str(&text)
                .map_err(|e| format!("{} is not JSON: {e}", input.display()))?;
            let schema = stated
                .get("schema")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let (recomputed, runs) = match schema {
                "ukis.results/v1" => {
                    let dir = input
                        .parent()
                        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
                    let dir = if dir.as_os_str().is_empty() {
                        PathBuf::from(".")
                    } else {
                        dir
                    };
                    let eval = results::evaluate(&dir);
                    (
                        serde_json::to_value(&eval.results).map_err(|e| e.to_string())?,
                        vec![dir],
                    )
                }
                "ukis.comparison/v1" => {
                    let run =
                        |key: &str| stated.get(key).and_then(Value::as_str).map(PathBuf::from);
                    let (Some(b), Some(c)) = (run("baseline_run"), run("candidate_run")) else {
                        return Err(format!(
                            "{} names no baseline_run / candidate_run",
                            input.display()
                        ));
                    };
                    let cmp = compare::compare_dirs(&b, &c);
                    (
                        serde_json::to_value(&cmp).map_err(|e| e.to_string())?,
                        vec![b, c],
                    )
                }
                other => return Err(format!("{}: unknown schema \"{other}\"", input.display())),
            };
            if recomputed != stated {
                return Err(format!(
                    "{} does not match a recomputation from its run data (edited or stale); rerun `run`/`compare` instead of editing it",
                    input.display()
                ));
            }
            runs
        };
        for dir in found {
            let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    Ok(dirs)
}

struct Notes(Vec<String>);

impl Notes {
    /// Adds a footnote and returns its marker.
    fn add(&mut self, text: String) -> String {
        self.0.push(text);
        format!("({})", self.0.len())
    }

    fn blank(&mut self, where_: &str, reason: &str) -> String {
        format!("- {}", self.add(format!("{where_}: {reason}")))
    }
}

struct Row<'a> {
    benchmark: String,
    base: Vec<&'a RunEval>,
    prior: Vec<&'a RunEval>,
    current: Vec<&'a RunEval>,
}

pub fn render(inputs: &[PathBuf]) -> Result<String, String> {
    let evals: Vec<RunEval> = resolve(inputs)?
        .iter()
        .map(|d| results::evaluate(d))
        .collect();
    let mut notes = Notes(Vec::new());
    let mut sources = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    for eval in &evals {
        let r = &eval.results;
        let role = r
            .arm
            .as_deref()
            .zip(eval.contract.as_ref())
            .and_then(|(a, c)| c.arm(a))
            .map(|a| a.role);
        sources.push(format!(
            "- {}: {} (arm {}, role {}, {}, thinking = {}, contract sha256 {})",
            r.benchmark.as_deref().unwrap_or("?"),
            eval.dir.display(),
            r.arm.as_deref().unwrap_or("?"),
            role.map_or("?", Role::as_str),
            r.status.as_str(),
            // Units are named, never implied: completion_tokens != reasoning_tokens (rule 21).
            eval.contract
                .as_ref()
                .and_then(|c| c.thinking_tokens.as_deref())
                .unwrap_or("not declared"),
            r.contract_sha256
                .as_deref()
                .map_or("?", |s| &s[..12.min(s.len())]),
        ));
        let Some(benchmark) = r.benchmark.clone() else {
            let first = r.reasons.first().map_or("", String::as_str);
            notes.add(format!(
                "{} not placed: {} {first}",
                eval.dir.display(),
                r.status.as_str()
            ));
            continue;
        };
        let idx = match rows.iter().position(|row| row.benchmark == benchmark) {
            Some(i) => i,
            None => {
                rows.push(Row {
                    benchmark,
                    base: vec![],
                    prior: vec![],
                    current: vec![],
                });
                rows.len() - 1
            }
        };
        match role {
            Some(Role::Baseline) => rows[idx].base.push(eval),
            Some(Role::Prior) => rows[idx].prior.push(eval),
            Some(Role::Treatment) => rows[idx].current.push(eval),
            Some(Role::Control) | None => {
                notes.add(format!(
                    "{} not placed: role is not baseline, prior or treatment",
                    eval.dir.display()
                ));
            }
        }
    }

    let mut lines = vec![HEADER.to_string(), ALIGN.to_string()];
    let mut verdicts = Vec::new();
    for row in &rows {
        let b = &row.benchmark;
        let base = accuracy_cell(&row.base, &format!("{b} / Base accuracy"), &mut notes);
        let prior = accuracy_cell(
            &row.prior,
            &format!("{b} / Prior model accuracy"),
            &mut notes,
        );
        let current = accuracy_cell(
            &row.current,
            &format!("{b} / Current model accuracy"),
            &mut notes,
        );
        let [delta, mean_think, median_think] = match (row.base.as_slice(), row.current.as_slice())
        {
            ([base], [cur]) => {
                let cmp = compare::compare(base, cur);
                verdicts.push(verdict_line(b, &cmp));
                change_cells(b, base, cur, &cmp, &mut notes)
            }
            _ => {
                let why = "needs exactly one base run and one current run";
                [
                    notes.blank(&format!("{b} / Current vs base"), why),
                    notes.blank(&format!("{b} / Mean thinking vs base"), why),
                    notes.blank(&format!("{b} / Median thinking vs base"), why),
                ]
            }
        };
        lines.push(format!(
            "| {b} | {base} | {prior} | {current} | {delta} | {mean_think} | {median_think} |"
        ));
    }

    let mut out = lines.join("\n");
    out.push('\n');
    if !notes.0.is_empty() {
        out.push_str("\nNotes:\n");
        for (i, n) in notes.0.iter().enumerate() {
            out.push_str(&format!("({}) {n}\n", i + 1));
        }
    }
    if !verdicts.is_empty() {
        out.push_str("\nVerdicts (claim: current improves the primary metric over base):\n");
        out.push_str(&verdicts.join("\n"));
        out.push('\n');
    }
    out.push_str("\nSources (every number above is recomputed from these run dirs):\n");
    out.push_str(&sources.join("\n"));
    out.push('\n');
    Ok(out)
}

fn accuracy_cell(runs: &[&RunEval], where_: &str, notes: &mut Notes) -> String {
    let eval = match runs {
        [] => return notes.blank(where_, "no run given"),
        [one] => *one,
        many => return notes.blank(where_, &format!("{} runs claim this column", many.len())),
    };
    let r = &eval.results;
    if r.status != RunStatus::Valid {
        let first = r.reasons.first().map_or("", String::as_str);
        return notes.blank(where_, first);
    }
    let Some(contract) = &eval.contract else {
        return notes.blank(where_, "no valid contract");
    };
    // Smoke = "ran / did not run", never a number (rule 10, V10).
    if contract.tier == Tier::Smoke {
        return notes.blank(where_, "smoke tier never reports a number");
    }
    let primary = contract.primary();
    if primary.kind != MetricKind::Accuracy {
        return notes.blank(
            where_,
            &format!("primary metric {} is not accuracy", primary.name),
        );
    }
    let Some(mean) = r.metrics.get(&primary.name).and_then(|m| m.mean) else {
        return notes.blank(where_, "no accuracy computed");
    };
    let cell = format!("{mean:.2}%");
    if contract.tier == Tier::Pilot {
        let marker = notes.add(format!(
            "{where_}: directional only (pilot, {} seeds)",
            contract.seeds.len()
        ));
        return format!("{cell} {marker}");
    }
    cell
}

fn change_cells(
    b: &str,
    base: &RunEval,
    cur: &RunEval,
    cmp: &Comparison,
    notes: &mut Notes,
) -> [String; 3] {
    let (w_delta, w_mean, w_median) = (
        format!("{b} / Current vs base"),
        format!("{b} / Mean thinking vs base"),
        format!("{b} / Median thinking vs base"),
    );
    if cmp.verdict == Verdict::NotComparable {
        let why = format!(
            "not comparable: {}",
            cmp.reasons.first().map_or("", String::as_str)
        );
        return [
            notes.blank(&w_delta, &why),
            notes.blank(&w_mean, &why),
            notes.blank(&w_median, &why),
        ];
    }
    let accuracy = base
        .contract
        .as_ref()
        .is_some_and(|c| c.primary().kind == MetricKind::Accuracy);
    let delta = match cmp.delta {
        Some(d) if accuracy => format!("{d:+.2} pp"),
        Some(_) => notes.blank(&w_delta, "primary metric is not accuracy"),
        None => notes.blank(&w_delta, "no paired seeds"),
    };
    // Thinking change = (current / base - 1) * 100%, same named token field in both runs.
    let field = base
        .contract
        .as_ref()
        .and_then(|c| c.thinking_tokens.clone());
    let mut thinking = |where_: &str, pick: fn(&results::TokenStats) -> f64| -> String {
        let Some(field) = &field else {
            return notes.blank(where_, "contract names no thinking_tokens field");
        };
        let (Some(bt), Some(ct)) = (
            base.results.tokens.get(field),
            cur.results.tokens.get(field),
        ) else {
            return notes.blank(where_, &format!("no {field} in both runs"));
        };
        if pick(bt) <= 0.0 {
            return notes.blank(where_, "base thinking is zero");
        }
        format!("{:+.2}%", (pick(ct) / pick(bt) - 1.0) * 100.0)
    };
    let mean = thinking(&w_mean, |t| t.mean);
    let median = thinking(&w_median, |t| t.median);
    [delta, mean, median]
}

fn verdict_line(b: &str, cmp: &Comparison) -> String {
    let mde = match &cmp.mde {
        Value::Number(n) => format!("{:.4}", n.as_f64().unwrap_or_default()),
        _ => "unknown".to_string(),
    };
    let test = cmp.test.as_ref().map_or_else(
        || "no test".to_string(),
        |t| {
            format!(
                "{} p = {:.4} (wins {}, losses {})",
                t.name, t.p, t.wins, t.losses
            )
        },
    );
    let verdict = serde_json::to_value(cmp.verdict)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    format!(
        "- {b}: {verdict}; seeds {}, n_pairs {}, MDE {mde}, {test}{}; {}",
        cmp.n_seeds,
        cmp.n_pairs,
        if cmp.cap_binds { ", cap binds" } else { "" },
        cmp.reasons.join("; ")
    )
}
