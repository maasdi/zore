//! Typed, resolved HIR: what the program means.

mod expr;
mod function;
pub mod lower;
mod stmt;

pub use expr::{Const, Expr, ExprKind, Place, Projection};
pub use function::{Function, Local};
pub use stmt::{Block, Stmt, StmtKind};

use crate::resolve::FunctionId;
use crate::source::Span;
use crate::types::{StructId, TypeId, TypeKind, TypeStore};

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
            | TypeKind::String
            | TypeKind::Error => true,
            TypeKind::Struct(id) => {
                let strukt = self.strukt(id);
                strukt.drop.is_none() && strukt.fields.iter().all(|f| self.is_copy(f.ty))
            }
            TypeKind::Array { element, .. } => self.is_copy(element),
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
