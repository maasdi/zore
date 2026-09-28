//! Syntax tree: what the programmer wrote, with unresolved names.

use crate::source::Span;
use crate::lexer::token::IntBase;

#[derive(Clone, Debug, PartialEq)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct File {
    /// `None` when the package clause is missing.
    pub package: Option<Name>,
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    /// Decoded import path.
    pub path: String,
    pub path_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Func(FuncDecl),
    Struct(StructDecl),
    Binding(Binding),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncDecl {
    pub is_async: bool,
    pub receiver: Option<Param>,
    pub name: Name,
    pub params: Vec<Param>,
    pub results: Vec<Type>,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDecl {
    pub name: Name,
    pub ty: Type,
    pub span: Span,
}

/// A named type; other type syntax is not parsed yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    pub name: Name,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Let,
    Var,
    Const,
}

/// A `let`, `var`, or `const` declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub kind: BindingKind,
    pub targets: Vec<BindingTarget>,
    /// Present only for single-target declarations.
    pub ty: Option<Type>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindingTarget {
    Name(Name),
    /// `_`, which introduces no name.
    Discard(Span),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Binding(Binding),
    Assign {
        targets: Vec<AssignTarget>,
        op: AssignOp,
        values: Vec<Expr>,
    },
    /// A call-based expression statement.
    Expr(Expr),
    Return(Vec<Expr>),
    Break,
    Continue,
    If(If),
    For(For),
    Block(Block),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AssignTarget {
    Place(Expr),
    Discard(Span),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignOp {
    Assign,
    /// A compound assignment such as `+=`.
    Compound(BinaryOp),
}

#[derive(Clone, Debug, PartialEq)]
pub struct If {
    pub condition: Expr,
    pub then_block: Block,
    pub else_branch: Option<Else>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Else {
    If(Box<If>),
    Block(Block),
}

#[derive(Clone, Debug, PartialEq)]
pub struct For {
    pub header: ForHeader,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ForHeader {
    Infinite,
    Condition(Expr),
    Counting {
        init: Box<Stmt>,
        condition: Expr,
        update: Box<Stmt>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Name(String),
    /// Integer literal; its value is decoded from the span during typing.
    Int(IntBase),
    /// Decimal float literal; its value is decoded from the span during typing.
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
    /// Postfix `?`.
    Try(Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: Name,
    },
    StructLit {
        ty: Name,
        fields: Vec<FieldInit>,
    },
    /// A literal the lexer already diagnosed.
    Malformed,
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
    /// Unary `^`: bitwise complement.
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
    /// Binding power: larger binds tighter.
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
