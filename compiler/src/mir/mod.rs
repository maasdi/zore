//! Control-flow-graph MIR: how the program executes.

pub mod drop;
pub mod error_use;
pub mod lower;
pub mod ownership;

use crate::ast::{BinaryOp, UnaryOp};
use crate::hir::{Const, FieldId, FunctionId};
use crate::source::Span;
use crate::types::{StructId, TypeId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Local(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlockId(pub u32);

#[derive(Debug)]
pub struct Program {
    pub bodies: Vec<Body>,
    /// The entry point.
    pub entry: Option<FunctionId>,
}

#[derive(Debug)]
pub struct Body {
    pub function: FunctionId,
    pub name: String,
    pub locals: Vec<LocalDecl>,
    /// Parameter locals, in order.
    pub params: Vec<Local>,
    /// Result locals, in order; `Return` returns their values.
    pub returns: Vec<Local>,
    pub blocks: Vec<BasicBlock>,
    pub unwind: Option<BlockId>,
}

#[derive(Debug)]
pub struct LocalDecl {
    pub ty: TypeId,
    /// Source name for user bindings; `None` for temporaries.
    pub name: Option<String>,
    /// The local holds a reference to a value of `ty`, which is what its places designate.
    pub by_reference: bool,
}

#[derive(Debug)]
pub struct BasicBlock {
    pub statements: Vec<Statement>,
    pub terminator: Terminator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub local: Local,
    pub fields: Vec<FieldId>,
}

impl Place {
    pub fn local(local: Local) -> Self {
        Self {
            local,
            fields: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Copy(Place),
    Move(Place),
    /// A reference to the place; only call arguments use it.
    Ref(Place),
    Const(Const, TypeId),
}

#[derive(Debug)]
pub enum Rvalue {
    Use(Operand),
    Zero,
    /// Checked arithmetic; `&&` and `||` are lowered to branches.
    Binary(BinaryOp, Operand, Operand),
    Unary(UnaryOp, Operand),
    /// Checked numeric conversion to the given type.
    Convert(Operand, TypeId),
    Error(Operand),
    /// Fields in declaration order.
    Aggregate(StructId, Vec<Operand>),
}

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
