use super::decl::ParamMode;
use super::expr::Expr;
use super::node::Name;
use crate::source::Span;

/// A type; channel forms are not parsed yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named(Name),
    /// `[element; size]`.
    Array {
        element: Box<Type>,
        size: Box<Expr>,
        span: Span,
    },
    /// The owned dynamic array `Array<element>`.
    DynArray {
        element: Box<Type>,
        span: Span,
    },
    /// The owned map `map[key]value`.
    Map {
        key: Box<Type>,
        value: Box<Type>,
        span: Span,
    },
    /// `[]element` or `mut []element`.
    Slice {
        element: Box<Type>,
        mutable: bool,
        span: Span,
    },
    /// The function type `func(T, mut U) R` (§16.2).
    Func {
        params: Vec<FuncTypeParam>,
        results: Vec<Type>,
        span: Span,
    },
}

/// One unnamed parameter of a function type.
#[derive(Clone, Debug, PartialEq)]
pub struct FuncTypeParam {
    pub mode: ParamMode,
    pub ty: Type,
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Self::Named(name) => name.span,
            Self::Array { span, .. }
            | Self::Slice { span, .. }
            | Self::DynArray { span, .. }
            | Self::Map { span, .. }
            | Self::Func { span, .. } => *span,
        }
    }
}
