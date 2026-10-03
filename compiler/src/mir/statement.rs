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
        /// The next statement stores a new value into `place`; a panic from
        /// this drop is acted on only after that store, so the slot always
        /// holds exactly one live value.
        before_store: bool,
    },
}
