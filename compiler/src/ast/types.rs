use super::expr::Expr;
use super::node::Name;
use crate::source::Span;

/// A type; slice and map forms are not parsed yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named(Name),
    /// `[element; size]`.
    Array {
        element: Box<Type>,
        size: Box<Expr>,
        span: Span,
    },
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Self::Named(name) => name.span,
            Self::Array { span, .. } => *span,
        }
    }
}
