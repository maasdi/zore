//! Control-flow-graph MIR: how the program executes.

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
}

#[derive(Debug)]
pub struct LocalDecl {
    pub ty: TypeId,
    /// Source name for user bindings; `None` for temporaries.
    pub name: Option<String>,
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
    Const(Const, TypeId),
}

#[derive(Debug)]
pub enum Rvalue {
    Use(Operand),
    /// Checked arithmetic; `&&` and `||` are lowered to branches.
    Binary(BinaryOp, Operand, Operand),
    Unary(UnaryOp, Operand),
    /// Checked numeric conversion to the given type.
    Convert(Operand, TypeId),
    /// Fields in declaration order.
    Aggregate(StructId, Vec<Operand>),
}

#[derive(Debug)]
pub struct Statement {
    pub place: Place,
    pub rvalue: Rvalue,
    /// Location reported by runtime checks.
    pub span: Span,
}

#[derive(Debug)]
pub enum Callee {
    Function(FunctionId),
    Println,
}

#[derive(Debug)]
pub enum Terminator {
    Goto(BlockId),
    Branch {
        condition: Operand,
        then_block: BlockId,
        else_block: BlockId,
    },
    Call {
        callee: Callee,
        args: Vec<Operand>,
        /// One destination per result; `None` discards it.
        destinations: Vec<Option<Place>>,
        target: BlockId,
        span: Span,
    },
    Return,
    Unreachable,
}
