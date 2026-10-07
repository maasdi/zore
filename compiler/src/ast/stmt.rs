use super::expr::{BinaryOp, Expr};
use super::node::Name;
use super::types::Type;
use crate::source::Span;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Let,
    Var,
    Const,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub kind: BindingKind,
    pub targets: Vec<BindingTarget>,
    /// Present only for single-target declarations.
    pub ty: Option<Type>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindingTarget {
    Name(Name),
    Discard(Span),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Binding(Binding),
    Assign {
        targets: Vec<AssignTarget>,
        op: AssignOp,
        values: Vec<Expr>,
    },
    /// A call-based expression statement.
    Expr(Expr),
    Return(Vec<Expr>),
    Break,
    Continue,
    If(If),
    For(For),
    Select(Select),
    Block(Block),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AssignTarget {
    Place(Expr),
    Discard(Span),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignOp {
    Assign,
    Compound(BinaryOp),
}

#[derive(Clone, Debug, PartialEq)]
pub struct If {
    pub condition: Expr,
    pub then_block: Block,
    pub else_branch: Option<Else>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Else {
    If(Box<If>),
    Block(Block),
}

#[derive(Clone, Debug, PartialEq)]
pub struct For {
    pub header: ForHeader,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ForHeader {
    Infinite,
    Condition(Expr),
    Counting {
        init: Box<Stmt>,
        condition: Expr,
        update: Box<Stmt>,
    },
    /// `for first in collection` or `for first, second in collection`.
    Each {
        first: BindingTarget,
        second: Option<BindingTarget>,
        collection: Expr,
    },
}

/// `select { arms }`; a `default` arm may appear anywhere among the cases.
#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub arms: Vec<SelectArm>,
    pub default: Option<Block>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectArm {
    pub comm: SelectComm,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectComm {
    /// `let targets = channel.receive()`.
    Bind(Binding),
    /// A send, or a receive whose results are discarded.
    Expr(Expr),
}
