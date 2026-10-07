use super::block::BlockId;
use super::operand::{Operand, Place};
use super::rvalue::Rvalue;
use crate::resolve::FunctionId;
use crate::source::Span;
use crate::types::TypeId;

#[derive(Debug)]
pub enum Callee {
    Function(FunctionId),
    /// Uses the closure at the place exclusively.
    Value(Place),
    Println,
    Drop,
    Clone(TypeId),
    /// `[Ref(map), key, value]`; panics without consuming the value on a duplicate key.
    MapInsertNew,
    /// `[Ref(map), key, value]`.
    MapAssign,
    /// `[Ref(map), key]` to `[found, value]`; copies the value out.
    MapLookup,
    /// `[Ref(map), key]` to `[found, value]`; detaches the value.
    MapRemove,
    /// `[Ref(array), value]`.
    ArrayPush,
    /// `[Ref(array)]` to `[found, value]`; detaches the last element.
    ArrayPop,
    /// `[task]` to the task's results; raises the task's panic if it had one.
    TaskWait,
    /// `[capacity]` to a channel for values of the element type.
    ChannelMake(TypeId),
    /// `[Ref(channel), value]`; waits for room, and panics on a closed channel.
    ChannelSend,
    /// `[Ref(channel)]` to `[value, received]`; waits for a value.
    ChannelReceive,
    /// `[Ref(channel)]`; panics on a closed channel.
    ChannelClose,
}

impl Callee {
    /// The map is borrowed mutably unless the operation only looks up.
    pub fn map_access(&self) -> Option<crate::ast::ParamMode> {
        match self {
            Callee::MapLookup => Some(crate::ast::ParamMode::Borrow),
            Callee::MapInsertNew | Callee::MapAssign | Callee::MapRemove => {
                Some(crate::ast::ParamMode::Mut)
            }
            Callee::Function(_)
            | Callee::Value(_)
            | Callee::Println
            | Callee::Drop
            | Callee::Clone(_)
            | Callee::ArrayPush
            | Callee::ArrayPop
            | Callee::TaskWait
            | Callee::ChannelMake(_)
            | Callee::ChannelSend
            | Callee::ChannelReceive
            | Callee::ChannelClose => None,
        }
    }
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
        /// `None` discards the result.
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

impl Terminator {
    /// Excludes unwind edges.
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Terminator::Goto(target) => vec![*target],
            Terminator::Branch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            Terminator::Call { target, .. } | Terminator::Assert { target, .. } => vec![*target],
            Terminator::Return | Terminator::PanicReturn | Terminator::Unreachable => Vec::new(),
        }
    }
}
