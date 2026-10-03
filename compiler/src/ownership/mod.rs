//! Ownership analysis: moves, partial moves, and borrow validity over MIR.

mod checker;
mod move_state;

pub use checker::check;
