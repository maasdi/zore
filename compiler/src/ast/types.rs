use super::decl::ParamMode;
use super::expr::Expr;
use super::node::Name;
use crate::source::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named(Name),
    /// `package.Name`
    Qualified {
        package: Name,
        name: Name,
        span: Span,
    },
    Array {
        element: Box<Type>,
        size: Box<Expr>,
        span: Span,
    },
    DynArray {
        element: Box<Type>,
        span: Span,
    },
    Map {
        key: Box<Type>,
        value: Box<Type>,
        span: Span,
    },
    Slice {
        element: Box<Type>,
        mutable: bool,
        span: Span,
    },
    Func {
        params: Vec<FuncTypeParam>,
        results: Vec<Type>,
        span: Span,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncTypeParam {
    pub mode: ParamMode,
    pub ty: Type,
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Self::Named(name) => name.span,
            Self::Qualified { span, .. }
            | Self::Array { span, .. }
            | Self::Slice { span, .. }
            | Self::DynArray { span, .. }
            | Self::Map { span, .. }
            | Self::Func { span, .. } => *span,
        }
    }
}
