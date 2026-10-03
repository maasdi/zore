use super::block::BlockId;
use super::operand::{Operand, Place};
use super::rvalue::Rvalue;
use crate::resolve::FunctionId;
use crate::source::Span;

#[derive(Debug)]
pub enum Callee {
    Function(FunctionId),
    Println,
    Drop,
}

#[derive(Debug)]
pub enum Terminator {
    Goto(BlockId),
    Branch {
        condition: Operand,
        then_block: BlockId,
        else_block: BlockId,
        span: Span,
    },
    Call {
        callee: Callee,
        args: Vec<Operand>,
        /// One destination per result; `None` discards it.
        destinations: Vec<Option<Place>>,
        target: BlockId,
        unwind: Option<BlockId>,
        span: Span,
    },
    Assert {
        place: Place,
        rvalue: Rvalue,
        target: BlockId,
        unwind: Option<BlockId>,
        span: Span,
    },
    Return,
    PanicReturn,
    Unreachable,
}
