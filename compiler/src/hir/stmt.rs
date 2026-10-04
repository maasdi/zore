use super::expr::{Expr, Place};
use crate::ast::BinaryOp;
use crate::resolve::LocalId;
use crate::source::Span;

#[derive(Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum StmtKind {
    /// One initializer; `None` targets discard their value.
    Let {
        targets: Vec<Option<LocalId>>,
        value: Expr,
    },
    /// Evaluates targets, then values, then stores left to right.
    Assign {
        targets: Vec<Option<Place>>,
        values: Vec<Expr>,
    },
    /// `map[key] = value`: evaluates the map place, key, then value, and only
    /// then inserts or replaces the entry (§13.3).
    MapAssign {
        map: Place,
        key: Expr,
        value: Expr,
    },
    /// `place op= value`, evaluating the place once.
    CompoundAssign {
        place: Place,
        op: BinaryOp,
        value: Expr,
    },
    Expr(Expr),
    Return(Vec<Expr>),
    Break,
    Continue,
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    /// Any loop form; absent parts are `None`.
    Loop {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Box<Stmt>>,
        body: Block,
    },
    Block(Block),
}
