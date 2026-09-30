//! Typed, resolved HIR: what the program means.

use crate::ast::{BinaryOp, ParamMode, UnaryOp};
use crate::source::Span;
use crate::types::{StructId, TypeId, TypeKind, TypeStore};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FunctionId(pub u32);

/// Index of a field within its struct's declaration order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FieldId(pub u32);

/// Index into the owning function's `locals`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LocalId(pub u32);

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub types: TypeStore,
    pub structs: Vec<Struct>,
    pub functions: Vec<Function>,
    /// The entry point, when this is an executable `main` package.
    pub entry: Option<FunctionId>,
}

impl Package {
    pub fn function(&self, id: FunctionId) -> &Function {
        &self.functions[id.0 as usize]
    }

    pub fn strukt(&self, id: StructId) -> &Struct {
        &self.structs[id.0 as usize]
    }

    /// Whether values of `ty` are Copy.
    pub fn is_copy(&self, ty: TypeId) -> bool {
        match self.types.kind(ty) {
            TypeKind::Bool
            | TypeKind::Int(_)
            | TypeKind::Float(_)
            | TypeKind::Rune
            | TypeKind::String => true,
            TypeKind::Struct(id) => {
                let strukt = self.strukt(id);
                strukt.drop.is_none() && strukt.fields.iter().all(|f| self.is_copy(f.ty))
            }
        }
    }
}

#[derive(Debug)]
pub struct Struct {
    pub name: String,
    pub span: Span,
    pub fields: Vec<Field>,
    /// The user-defined `drop` method, which makes the struct Move.
    pub drop: Option<FunctionId>,
}

#[derive(Debug)]
pub struct Field {
    pub name: String,
    pub ty: TypeId,
    pub span: Span,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalKind {
    Param(ParamMode),
    Let,
    Var,
}

#[derive(Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum StmtKind {
    /// One initializer; `None` targets discard their value.
    Let {
        targets: Vec<Option<LocalId>>,
        value: Expr,
    },
    /// Evaluates targets, then values, then stores left to right.
    Assign {
        targets: Vec<Option<Place>>,
        values: Vec<Expr>,
    },
    /// `place op= value`, evaluating the place once.
    CompoundAssign {
        place: Place,
        op: BinaryOp,
        value: Expr,
    },
    Expr(Expr),
    Return(Vec<Expr>),
    Break,
    Continue,
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    /// Any loop form; absent parts are `None`.
    Loop {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Box<Stmt>>,
        body: Block,
    },
    Block(Block),
}

/// A local with field projections.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub root: LocalId,
    pub fields: Vec<FieldId>,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug)]
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
}

#[derive(Debug)]
pub enum ExprKind {
    Const(Const),
    Local(LocalId),
    Field {
        base: Box<Expr>,
        field: FieldId,
    },
    Call {
        function: FunctionId,
        args: Vec<Expr>,
    },
    Println(Box<Expr>),
    Drop(Box<Expr>),
    /// Checked numeric conversion.
    Convert(Box<Expr>),
    /// Fields in written order, which is evaluation order.
    StructLit {
        strukt: StructId,
        fields: Vec<(FieldId, Expr)>,
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
