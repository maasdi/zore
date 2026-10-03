//! Human-readable diagnostics tied to validated source spans.

#[allow(clippy::module_inception)]
mod diagnostic;
mod label;
mod renderer;

pub use diagnostic::{Diagnostic, Severity};
pub use label::Label;
pub use renderer::RenderError;
