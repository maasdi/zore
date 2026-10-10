use super::body::Local;
use super::operand::{Operand, Place};
use super::rvalue::Rvalue;
use crate::resolve::GlobalId;
use crate::source::Span;

#[derive(Debug)]
pub enum Statement {
    Assign {
        place: Place,
        rvalue: Rvalue,
        span: Span,
    },
    EndScope(Vec<Local>),
    /// Moves the value into a package-level `let`, which keeps it for the rest of the program.
    SetGlobal {
        global: GlobalId,
        value: Operand,
        span: Span,
    },
    Drop {
        place: Place,
        replacement: bool,
        /// A panic from this drop is acted on after the following store.
        before_store: bool,
    },
}
