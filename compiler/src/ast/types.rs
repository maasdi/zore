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
        is_async: bool,
        params: Vec<FuncTypeParam>,
        results: Vec<Type>,
        span: Span,
    },
    Channel {
        element: Box<Type>,
        span: Span,
    },
    Mutex {
        element: Box<Type>,
        span: Span,
    },
    /// A generic struct type with its type arguments, `Stack<int>` or `pkg.Stack<int>`.
    Instance {
        base: Box<Type>,
        args: Vec<Type>,
        span: Span,
    },
    /// `Task<R1, ..., Rn>`, or plain `Task` with no results.
    Task {
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
            | Self::Func { span, .. }
            | Self::Task { span, .. }
            | Self::Instance { span, .. }
            | Self::Channel { span, .. }
            | Self::Mutex { span, .. } => *span,
        }
    }
}
