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
    /// `[Ref(map), key, value]`: adds a literal entry, panicking without
    /// consuming the value if the key is already present.
    MapInsertNew,
    /// `[Ref(map), key, value]`: replaces or inserts an entry (§13.3).
    MapAssign,
    /// `[Ref(map), key]` to `[found, value]`: copies the value out.
    MapLookup,
    /// `[Ref(map), key]` to `[found, value]`: detaches and returns the value.
    MapRemove,
}

impl Callee {
    /// Whether this is a compiler-provided map operation, whose first
    /// argument is the map borrowed mutably unless it only looks up.
    pub fn map_access(&self) -> Option<crate::ast::ParamMode> {
        match self {
            Callee::MapLookup => Some(crate::ast::ParamMode::Borrow),
            Callee::MapInsertNew | Callee::MapAssign | Callee::MapRemove => {
                Some(crate::ast::ParamMode::Mut)
            }
            Callee::Function(_) | Callee::Println | Callee::Drop => None,
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

impl Terminator {
    /// The blocks control may continue to on the normal (non-unwind) path.
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
