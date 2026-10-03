use super::body::Local;
use super::operand::Place;
use super::rvalue::Rvalue;
use crate::source::Span;

#[derive(Debug)]
pub enum Statement {
    Assign {
        place: Place,
        rvalue: Rvalue,
        span: Span,
    },
    EndScope(Vec<Local>),
    Drop {
        place: Place,
        replacement: bool,
    },
}
