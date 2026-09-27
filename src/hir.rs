//! Typed, resolved program meaning (spec §25.2, §27–30).
//!
//! Names are replaced by stable IDs, every expression carries a type, constant
//! expressions are folded to values, and assignment targets are places with
//! field projections. Explicit Copy/Move operations belong to MIR (§29.2).

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
    /// The §3.19 entry point, when this is a valid `main` package.
    pub entry: Option<FunctionId>,
}

impl Package {
    pub fn function(&self, id: FunctionId) -> &Function {
        &self.functions[id.0 as usize]
    }

    pub fn strukt(&self, id: StructId) -> &Struct {
        &self.structs[id.0 as usize]
    }

    /// Copy/Move classification derived from fields (§8.3, §10.2). Every type
    /// the checker currently accepts is Copy.
    pub fn is_copy(&self, ty: TypeId) -> bool {
        match self.types.kind(ty) {
            TypeKind::Bool | TypeKind::Int(_) | TypeKind::Rune | TypeKind::String => true,
            TypeKind::Struct(id) => self.strukt(id).fields.iter().all(|f| self.is_copy(f.ty)),
        }
    }
}

#[derive(Debug)]
pub struct Struct {
    pub name: String,
    pub span: Span,
    pub fields: Vec<Field>,
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
    /// One initializer producing one value per target; `None` discards.
    Let {
        targets: Vec<Option<LocalId>>,
        value: Expr,
    },
    /// Targets are evaluated, then values, then stored left to right (§5.6).
    Assign {
        targets: Vec<Option<Place>>,
        values: Vec<Expr>,
    },
    /// `place op= value`: the place is evaluated once (§5.6).
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
    /// All three loop forms; absent parts are `None` (§5.10).
    Loop {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Box<Stmt>>,
        body: Block,
    },
    Block(Block),
}

/// A local with field projections (§30).
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
    /// Result types: one for ordinary values, several for a multiple-result
    /// call, none for a call without results.
    pub types: Vec<TypeId>,
    pub span: Span,
}

impl Expr {
    /// The single result type; valid only for single-value expressions.
    pub fn ty(&self) -> TypeId {
        debug_assert_eq!(self.types.len(), 1);
        self.types[0]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Bool(bool),
    Int(i128),
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
    /// Checked integer conversion (§6.6).
    Convert(Box<Expr>),
    /// Fields in written order, which is also evaluation order (§8.4).
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
