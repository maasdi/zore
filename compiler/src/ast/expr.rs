use super::decl::Param;
use super::node::Name;
use super::stmt::Block;
use super::types::Type;
use crate::lexer::IntBase;
use crate::source::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Name(String),
    /// Decoded from the span during typing.
    Int(IntBase),
    /// Decoded from the span during typing.
    Float,
    String(String),
    Rune(char),
    Bool(bool),
    Nil,
    Paren(Box<Expr>),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Await(Box<Expr>),
    Go(Box<Expr>),
    Try(Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: Name,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// Either bound may be omitted.
    Slice {
        base: Box<Expr>,
        low: Option<Box<Expr>>,
        high: Option<Box<Expr>>,
    },
    StructLit {
        /// Present for `package.Type{...}`.
        package: Option<Name>,
        ty: Name,
        /// `Stack<int>{...}`; empty for a type that is not generic.
        type_args: Vec<Type>,
        fields: Vec<FieldInit>,
    },
    ArrayLit {
        ty: Type,
        elements: Vec<Expr>,
    },
    /// Entries in written order.
    MapLit {
        ty: Type,
        entries: Vec<MapEntry>,
    },
    Closure(Box<Closure>),
    /// `channel<T>()` or `channel<T>(capacity)`.
    Channel {
        element: Type,
        capacity: Option<Box<Expr>>,
    },
    /// A literal the lexer already diagnosed.
    Malformed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Closure {
    pub params: Vec<Param>,
    pub results: Vec<Type>,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapEntry {
    pub key: Expr,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldInit {
    pub name: Name,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Plus,
    Neg,
    Not,
    Complement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    BitAnd,
    Add,
    Sub,
    BitOr,
    BitXor,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

impl BinaryOp {
    /// Larger binds tighter.
    pub fn precedence(self) -> u8 {
        match self {
            Self::Mul | Self::Div | Self::Rem | Self::Shl | Self::Shr | Self::BitAnd => 5,
            Self::Add | Self::Sub | Self::BitOr | Self::BitXor => 4,
            Self::Eq | Self::NotEq | Self::Lt | Self::LtEq | Self::Gt | Self::GtEq => 3,
            Self::And => 2,
            Self::Or => 1,
        }
    }

    pub fn is_comparison(self) -> bool {
        self.precedence() == 3
    }
}
