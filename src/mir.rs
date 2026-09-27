//! Control-flow-graph MIR: how the program executes (spec §25.2, §33).
//!
//! Each function body has locals, basic blocks of statements, and one
//! terminator per block. Operands distinguish `Copy`, `Move`, and constants
//! (§33.2); control flow such as short-circuit logic, loops, and calls is
//! explicit. Every type accepted so far is Copy, so lowering emits only `Copy`
//! and constant operands until ownership analysis exists.

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
    /// Index into `bodies` of the §3.19 entry point.
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
    /// Not produced yet: every accepted type is Copy.
    Move(Place),
    Const(Const, TypeId),
}

#[derive(Debug)]
pub enum Rvalue {
    Use(Operand),
    /// Checked where §6.6 requires (integer overflow, division by zero,
    /// shift counts); `&&`/`||` never appear here.
    Binary(BinaryOp, Operand, Operand),
    Unary(UnaryOp, Operand),
    /// Checked numeric conversion to the given type.
    Convert(Operand, TypeId),
    /// Fields in declaration order; evaluation order was fixed by earlier
    /// statements (§8.4).
    Aggregate(StructId, Vec<Operand>),
}

#[derive(Debug)]
pub struct Statement {
    pub place: Place,
    pub rvalue: Rvalue,
    /// Source location, reported by runtime checks.
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
