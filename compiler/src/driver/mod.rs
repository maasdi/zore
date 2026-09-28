//! Command routing, source loading, and compiler pipeline orchestration.

pub mod build;
pub mod check;
mod command;
mod session;

pub use session::run;
