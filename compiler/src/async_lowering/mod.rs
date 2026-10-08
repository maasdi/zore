//! Persistent-frame planning after drop insertion. The original MIR remains available for
//! fiber compatibility; code generation consumes the explicit suspension plan.

mod lower;
mod state_machine;
mod suspension;

pub use lower::lower;
pub use state_machine::{Plan, StateMachine};
pub use suspension::Suspension;
