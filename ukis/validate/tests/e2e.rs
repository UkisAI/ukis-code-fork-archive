//! End to end through the real binary on a small Benchmark_configs-shaped fixture:
//! import -> check-contract -> run -> compare -> table, plus the tamper cases that must be refused.
//! Fixture: tests/fixtures/bc_gpqa (regenerate with make_bc_fixture.py).

use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

fn bin(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ukis-validate"))
        .args(args)
        .output()
        .expect("spawn ukis-validate")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Copy the fixture to a fresh temp dir and turn both arms into locked, imported run dirs.
fn prepared(name: &str) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bc_gpqa");
    let root =
        std::env::temp_dir().join(format!("ukis-validate-e2e-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for arm in ["base", "current"] {
        let dir = root.join(arm);
        copy_dir(&fixture.join(arm), &dir);
        std::fs::copy(fixture.join("contract.json"), dir.join("contract.json")).unwrap();
        let d = dir.to_str().unwrap();
        let o = bin(&["check-contract", d]);
        assert!(o.status.success(), "check-contract: {}", stdout(&o));
        let o = bin(&["import", "benchmark-configs", d, "--arm", arm]);
        assert!(
            o.status.success(),
            "import: {} {}",
            stdout(&o),
            String::from_utf8_lossy(&o.stderr)
        );
    }
    root
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn full_flow_supported_and_table_renders() {
    let root = prepared("flow");
    let (base, current) = (root.join("base"), root.join("current"));
    for dir in [&base, &current] {
        let o = bin(&["run", dir.to_str().unwrap()]);
        assert!(o.status.success(), "run: {}", stdout(&o));
        let results = json(&dir.join("results.json"));
        assert_eq!(results["status"], "VALID");
        // The real Benchmark_configs report() output agrees with the recomputation.
        assert_eq!(results["runner_summary"]["agrees"], true, "{results:#}");
    }
    let base_results = json(&base.join("results.json"));
    assert_eq!(base_results["metrics"]["accuracy"]["mean"], 47.5);
    assert_eq!(
        base_results["status_counts"],
        serde_json::json!({"error": 1, "ok": 38, "truncated": 1})
    );

    let cmp_path = root.join("comparison.json");
    let o = bin(&[
        "compare",
        base.to_str().unwrap(),
        current.to_str().unwrap(),
        "--out",
        cmp_path.to_str().unwrap(),
    ]);
    assert!(o.status.success());
    let cmp = json(&cmp_path);
    assert_eq!(cmp["verdict"], "SUPPORTED", "{cmp:#}");
    assert_eq!(cmp["delta"], 35.0);
    assert_eq!(cmp["n_pairs"], 40);
    assert_eq!(cmp["test"]["wins"], 14);
    assert_eq!(cmp["test"]["losses"], 0);
    assert_eq!(cmp["cap_binds"], true, "base truncated one row");

    let o = bin(&["table", cmp_path.to_str().unwrap()]);
    assert!(o.status.success(), "table: {}", stdout(&o));
    let table = stdout(&o);
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(
        lines[..3],
        [
            "| Benchmark | Base accuracy | Prior model accuracy | Current model accuracy | Current vs base | Mean thinking vs base | Median thinking vs base |",
            "|---|---:|---:|---:|---:|---:|---:|",
            "| Toy GPQA | 47.50% | - (1) | 82.50% | +35.00 pp | -19.33% | -19.71% |",
        ]
    );
    assert!(
        table.contains("(1) Toy GPQA / Prior model accuracy: no run given"),
        "{table}"
    );
    assert!(table.contains("- Toy GPQA: SUPPORTED;"), "{table}");
    assert!(
        !table.contains('\u{2014}') && !table.contains('\u{2013}'),
        "no em or en dashes"
    );
}

#[test]
fn invented_runner_summary_makes_run_invalid() {
    let root = prepared("invented");
    let current = root.join("current");
    let path = current.join("runner_summary.json");
    let mut summary = json(&path);
    summary["mean"] = serde_json::json!(90.0);
    std::fs::write(&path, summary.to_string()).unwrap();
    let o = bin(&["run", current.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stdout(&o).contains("runner summary disagrees: mean"),
        "{}",
        stdout(&o)
    );
}

#[test]
fn table_refuses_edited_results_and_comparisons() {
    let root = prepared("tamper");
    let (base, current) = (root.join("base"), root.join("current"));
    for dir in [&base, &current] {
        assert!(bin(&["run", dir.to_str().unwrap()]).status.success());
    }
    // Untouched results.json files are accepted...
    let results_path = current.join("results.json");
    let o = bin(&[
        "table",
        base.join("results.json").to_str().unwrap(),
        results_path.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", stdout(&o));
    // ...but a number typed into one is refused, not rendered.
    let mut results = json(&results_path);
    results["metrics"]["accuracy"]["mean"] = serde_json::json!(99.0);
    std::fs::write(&results_path, results.to_string()).unwrap();
    let o = bin(&[
        "table",
        base.to_str().unwrap(),
        results_path.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stdout(&o).contains("does not match a recomputation"),
        "{}",
        stdout(&o)
    );

    // Same for a comparison.json whose verdict was upgraded by hand.
    let cmp_path = root.join("comparison.json");
    assert!(
        bin(&[
            "compare",
            base.to_str().unwrap(),
            current.to_str().unwrap(),
            "--out",
            cmp_path.to_str().unwrap()
        ])
        .status
        .success()
    );
    let mut cmp = json(&cmp_path);
    cmp["delta"] = serde_json::json!(50.0);
    std::fs::write(&cmp_path, cmp.to_string()).unwrap();
    let o = bin(&["table", cmp_path.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
}
