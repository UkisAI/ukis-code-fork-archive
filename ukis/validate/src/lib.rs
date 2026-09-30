//! ukis-validate: the deterministic core of ukis-research. The model proposes, this code decides.
//!
//! Flow: `contract.json` (preregistered, locked by hash) + `items.jsonl` (one row per
//! arm x item x seed, written by the run or a thin adapter) -> `results.json` (recomputed)
//! -> `comparison.json` (paired test + verdict) -> canonical table (VALID data only).
//! Methodology: docs/ukis-research/study/E-ukis-bench-cli.md.

pub mod compare;
pub mod contract;
pub mod import_bc;
pub mod items;
pub mod results;
mod stats;
pub mod table;
#[cfg(test)]
mod test_support;
