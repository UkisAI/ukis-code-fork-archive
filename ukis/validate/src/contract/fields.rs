//! Per-field parsers for the list-valued contract fields (metrics, arms, controls).
//! Each pushes refusals instead of failing fast, so one pass reports every problem.

use super::Arm;
use super::Direction;
use super::Metric;
use super::MetricKind;
use super::RawArm;
use super::RawControl;
use super::RawMetric;
use super::Role;
use super::text;

pub(super) fn parse_metrics(raw: Option<&[RawMetric]>, refusals: &mut Vec<String>) -> Vec<Metric> {
    let raw = raw.unwrap_or_default();
    if raw.is_empty() {
        refusals.push("missing metrics".to_string());
        return Vec::new();
    }
    let mut out: Vec<Metric> = Vec::new();
    for (i, m) in raw.iter().enumerate() {
        // Real names, defined on first use, with unit and good direction (experiment R0, rule 24).
        let label =
            text(&m.name).map_or_else(|| format!("metrics[{i}]"), |n| format!("metric {n}"));
        let direction = match m.direction.as_deref() {
            Some("higher_better") => Some(Direction::HigherBetter),
            Some("lower_better") => Some(Direction::LowerBetter),
            _ => None,
        };
        let kind = match m.kind.as_deref() {
            Some("accuracy") => Some(MetricKind::Accuracy),
            Some("scalar") => Some(MetricKind::Scalar),
            _ => None,
        };
        let mut ok = true;
        for (field, value) in [
            ("name", &m.name),
            ("unit", &m.unit),
            ("definition", &m.definition),
        ] {
            if text(value).is_none() {
                refusals.push(format!("{label}: missing {field}"));
                ok = false;
            }
        }
        if direction.is_none() {
            refusals.push(format!(
                "{label}: direction must be higher_better or lower_better"
            ));
            ok = false;
        }
        if kind.is_none() {
            refusals.push(format!("{label}: kind must be accuracy or scalar"));
            ok = false;
        }
        // Accuracy is reported in percent and lower is never better; a wrong declaration here
        // would silently flip every verdict, so refuse it instead of trusting it.
        if kind == Some(MetricKind::Accuracy) {
            if direction == Some(Direction::LowerBetter) {
                refusals.push(format!("{label}: accuracy must be higher_better"));
                ok = false;
            }
            if m.unit.as_deref() != Some("percent") {
                refusals.push(format!("{label}: accuracy unit must be \"percent\""));
                ok = false;
            }
        }
        if let (true, Some(direction), Some(kind)) = (ok, direction, kind) {
            out.push(Metric {
                name: text(&m.name).unwrap_or_default().to_string(),
                unit: text(&m.unit).unwrap_or_default().to_string(),
                direction,
                kind,
                primary: m.primary,
            });
        }
    }
    let mut names: Vec<&str> = out.iter().map(|m| m.name.as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        refusals.push("metric names must be unique".to_string());
    }
    if raw.iter().filter(|m| m.primary).count() != 1 {
        refusals
            .push("exactly one metric must be primary (the one the decision is about)".to_string());
    }
    // One `correct` field per row can only back one accuracy metric.
    if out
        .iter()
        .filter(|m| m.kind == MetricKind::Accuracy)
        .count()
        > 1
    {
        refusals.push(
            "at most one accuracy metric per contract (rows carry one `correct`)".to_string(),
        );
    }
    out
}

pub(super) fn parse_arms(raw: Option<&[RawArm]>, refusals: &mut Vec<String>) -> Vec<Arm> {
    let mut out: Vec<Arm> = Vec::new();
    for (i, a) in raw.unwrap_or_default().iter().enumerate() {
        let role = match a.role.as_deref() {
            Some("baseline") => Some(Role::Baseline),
            Some("prior") => Some(Role::Prior),
            Some("treatment") => Some(Role::Treatment),
            Some("control") => Some(Role::Control),
            _ => None,
        };
        match (text(&a.name), role) {
            (Some(name), Some(role)) => out.push(Arm {
                name: name.to_string(),
                role,
            }),
            _ => refusals.push(format!(
                "arms[{i}] needs a name and role baseline|prior|treatment|control"
            )),
        }
    }
    let mut names: Vec<&str> = out.iter().map(|a| a.name.as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        refusals.push("arm names must be unique".to_string());
    }
    // Without a baseline there is nothing to compare against, so no claim is possible (rule 12).
    if !out.iter().any(|a| a.role == Role::Baseline) {
        refusals.push("no baseline arm (every comparative question needs one)".to_string());
    }
    out
}

pub(super) fn parse_controls(raw: Option<&[RawControl]>, arms: &[Arm], refusals: &mut Vec<String>) {
    let raw = raw.unwrap_or_default();
    if raw.is_empty() {
        refusals
            .push("missing controls (say which arm rules out which rival explanation)".to_string());
    }
    for (i, c) in raw.iter().enumerate() {
        match text(&c.arm) {
            Some(arm) if arms.iter().any(|a| a.name == arm) => {}
            _ => refusals.push(format!("controls[{i}].arm must name a declared arm")),
        }
        if text(&c.rules_out).is_none() {
            refusals.push(format!("controls[{i}]: missing rules_out"));
        }
    }
}
