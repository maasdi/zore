//! Ownership analysis: moves, partial moves, and borrow validity over MIR.

mod borrow;
mod checker;
mod move_state;
mod region;

pub use checker::check;
