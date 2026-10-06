use super::stmt::Block;
use crate::resolve::{LocalId, LocalKind};
use crate::source::Span;
use crate::types::TypeId;

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub span: Span,
    pub params: Vec<LocalId>,
    pub results: Vec<TypeId>,
    /// For a closure, its capture locals, in the order the creating
    /// expression lists them; each refers to its referent by reference.
    pub captures: Vec<LocalId>,
    /// Whether this is a closure body, called through a function value
    /// with its captured environment.
    pub is_closure: bool,
    pub locals: Vec<Local>,
    pub body: Block,
}

#[derive(Debug)]
pub struct Local {
    pub name: String,
    pub ty: TypeId,
    pub kind: LocalKind,
    pub span: Span,
}
