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
    /// Capture locals, in the order the closure expression lists them.
    pub captures: Vec<LocalId>,
    pub is_closure: bool,
    pub is_async: bool,
    /// Declared without a body; the runtime provides its code.
    pub native: bool,
    /// The body consumes a captured value, so the closure runs at most once.
    pub call_once: bool,
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
