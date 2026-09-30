//! CLI over the ukis_validate library. Plain std args: the surface is five commands.
//! Exit codes: 0 ok / VALID, 1 refused / not VALID, 2 usage or IO error.

use std::path::PathBuf;
use std::process::ExitCode;
use ukis_validate::compare;
use ukis_validate::contract;
use ukis_validate::import_bc;
use ukis_validate::results;
use ukis_validate::results::RunStatus;
use ukis_validate::table;

const USAGE: &str = "usage:
  ukis-validate check-contract <contract.json | run_dir>
  ukis-validate run <run_dir>
  ukis-validate compare <baseline_run> <candidate_run> [--out comparison.json]
  ukis-validate table <run_dir | results.json | comparison.json>... [--out table.md]
  ukis-validate import benchmark-configs <out_dir> --arm <name> [--bench <name>] [--into <run_dir>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("{msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

type Flags = Vec<(String, String)>;

/// Split `--flag value` pairs from positional args.
fn split(args: &[String]) -> Result<(Vec<String>, Flags), String> {
    let (mut pos, mut flags) = (Vec::new(), Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
            flags.push((name.to_string(), value.clone()));
        } else {
            pos.push(a.clone());
        }
    }
    Ok((pos, flags))
}

fn flag<'a>(flags: &'a [(String, String)], name: &str) -> Option<&'a str> {
    flags
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn write_or_print(text: &str, out: Option<&str>) -> Result<(), String> {
    print!("{text}");
    if let Some(path) = out {
        std::fs::write(path, text).map_err(|e| format!("cannot write {path}: {e}"))?;
    }
    Ok(())
}

fn dispatch(args: &[String]) -> Result<ExitCode, String> {
    let (pos, flags) = split(args)?;
    let pos: Vec<&str> = pos.iter().map(String::as_str).collect();
    match pos.as_slice() {
        ["check-contract", path] => match contract::check_and_lock(&PathBuf::from(path)) {
            Ok(c) => {
                println!(
                    "OK contract sha256 {} (tier {}, family {})",
                    c.sha256,
                    c.tier.as_str(),
                    c.family
                );
                Ok(ExitCode::SUCCESS)
            }
            Err(refusals) => {
                for r in refusals {
                    println!("REFUSED: {r}");
                }
                Ok(ExitCode::from(1))
            }
        },
        ["run", dir] => {
            let r = results::run_and_write(&PathBuf::from(dir))?;
            println!("{} ({dir}/{})", r.status.as_str(), results::RESULTS_FILE);
            for reason in r.reasons.iter().chain(&r.warnings) {
                println!("  {reason}");
            }
            Ok(if r.status == RunStatus::Valid {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        ["compare", base, cand] => {
            let cmp = compare::compare_dirs(&PathBuf::from(base), &PathBuf::from(cand));
            let text = serde_json::to_string_pretty(&cmp).map_err(|e| e.to_string())? + "\n";
            write_or_print(&text, flag(&flags, "out"))?;
            Ok(ExitCode::SUCCESS)
        }
        ["table", inputs @ ..] if !inputs.is_empty() => {
            let inputs: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
            match table::render(&inputs) {
                Ok(text) => {
                    write_or_print(&text, flag(&flags, "out"))?;
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    println!("REFUSED: {e}");
                    Ok(ExitCode::from(1))
                }
            }
        }
        ["import", "benchmark-configs", out_dir] => {
            let arm = flag(&flags, "arm").ok_or("--arm is required")?;
            let into = PathBuf::from(flag(&flags, "into").unwrap_or(out_dir));
            let report =
                import_bc::import(&PathBuf::from(out_dir), arm, flag(&flags, "bench"), &into)?;
            println!(
                "imported {} rows of {} into {}",
                report.rows,
                report.bench,
                into.join(results::ITEMS_FILE).display()
            );
            match report.summary_copied {
                Some(s) => println!(
                    "runner summary {s} copied to {}",
                    results::RUNNER_SUMMARY_FILE
                ),
                None => println!("no runner summary found (score_{}.json)", report.bench),
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => Err("unknown command".to_string()),
    }
}
