//! Ukis research extension.
//!
//! Spike scope: only the final-answer claim gate. When the agent ends a turn with a
//! numeric performance claim that carries no `[run:<id>]` evidence tag, the gate injects
//! a correction into the still-running turn so the model must answer again.

mod claim_gate;
mod extension;

pub use claim_gate::MAX_GATE_CORRECTIONS_PER_TURN;
pub use extension::install;
