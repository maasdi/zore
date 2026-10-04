use crate::ast::{BinaryOp, UnaryOp};
use crate::resolve::{FieldId, FunctionId, LocalId};
use crate::source::Span;
use crate::types::{StructId, TypeId};

/// A step from a place into one of its parts.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    Field(FieldId),
    /// An array or slice element; the index is evaluated, not yet bounds-checked.
    Index(Box<Expr>),
}

/// A local with field and index projections.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub root: LocalId,
    pub projections: Vec<Projection>,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    /// One type per result; empty for calls without results.
    pub types: Vec<TypeId>,
    pub span: Span,
}

impl Expr {
    /// The type of a single-value expression.
    pub fn ty(&self) -> TypeId {
        debug_assert_eq!(self.types.len(), 1);
        self.types[0]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Bool(bool),
    Int(i128),
    /// Exactly representable in the expression's type; never negative zero.
    Float(f64),
    Rune(char),
    String(String),
    Nil,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Const(Const),
    Local(LocalId),
    Field {
        base: Box<Expr>,
        field: FieldId,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// `base[low:high]`, borrowing an exclusive view when `mutable`.
    Slice {
        base: Box<Expr>,
        low: Option<Box<Expr>>,
        high: Option<Box<Expr>>,
        mutable: bool,
    },
    Call {
        function: FunctionId,
        args: Vec<Expr>,
    },
    Println(Box<Expr>),
    Drop(Box<Expr>),
    /// Checked numeric conversion.
    Convert(Box<Expr>),
    Error(Box<Expr>),
    Try(Box<Expr>),
    /// Fields in written order, which is evaluation order.
    StructLit {
        strukt: StructId,
        fields: Vec<(FieldId, Expr)>,
    },
    /// Elements in evaluation order.
    ArrayLit {
        element: TypeId,
        elements: Vec<Expr>,
    },
    /// `map[K]V{...}`: entries in written order, each key before its value.
    MapLit {
        key: TypeId,
        value: TypeId,
        entries: Vec<(Expr, Expr)>,
    },
    /// `map[key]`: presence, then a copy of the value or its zero (§13.3).
    MapLookup {
        map: Box<Expr>,
        key: Box<Expr>,
    },
    /// `map.remove(key)`: presence, then the detached value or its zero.
    MapRemove {
        map: Box<Expr>,
        key: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}
