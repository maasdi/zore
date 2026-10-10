use super::node::Name;
use super::stmt::{Binding, Block};
use super::types::Type;
use crate::source::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct File {
    /// `None` when the package clause is missing.
    pub package: Option<Name>,
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    pub path: String,
    pub path_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Func(FuncDecl),
    Struct(StructDecl),
    Named(NamedDecl),
    Binding(Binding),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncDecl {
    pub is_async: bool,
    pub receiver: Option<Param>,
    pub name: Name,
    pub params: Vec<Param>,
    pub results: Vec<Type>,
    /// Empty for a native function.
    pub body: Block,
    /// Declared without a body; its code is provided by the runtime.
    pub native: bool,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ParamMode {
    /// No modifier: a shared borrow.
    Borrow,
    Mut,
    Own,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: Name,
    pub mode: ParamMode,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructDecl {
    pub name: Name,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

/// `type Name Base`: a distinct type with the representation and operations of `Base`.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedDecl {
    pub name: Name,
    pub base: Type,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDecl {
    pub name: Name,
    pub ty: Type,
    pub span: Span,
}
