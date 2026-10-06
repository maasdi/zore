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
    /// `None` targets discard their value.
    Let {
        targets: Vec<Option<LocalId>>,
        value: Expr,
    },
    /// Evaluates targets, then values, then stores left to right.
    Assign {
        targets: Vec<Option<Place>>,
        values: Vec<Expr>,
    },
    /// Evaluates the map place, key, then value before changing the entry.
    MapAssign {
        map: Place,
        key: Expr,
        value: Expr,
    },
    /// Evaluates the place once.
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
    /// Absent parts are `None`.
    Loop {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Box<Stmt>>,
        body: Block,
    },
    /// Visits each element or entry; `item` borrows it and `key` is its index or key.
    ForEach {
        key: Option<LocalId>,
        item: Option<LocalId>,
        collection: Expr,
        body: Block,
    },
    Block(Block),
}
