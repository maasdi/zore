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
