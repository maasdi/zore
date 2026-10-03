use super::expr::Expr;
use super::node::Name;
use crate::source::Span;

/// A type; map, channel, and function forms are not parsed yet.
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
    /// `[]element` or `mut []element`.
    Slice {
        element: Box<Type>,
        mutable: bool,
        span: Span,
    },
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Self::Named(name) => name.span,
            Self::Array { span, .. } | Self::Slice { span, .. } | Self::DynArray { span, .. } => {
                *span
            }
        }
    }
}
