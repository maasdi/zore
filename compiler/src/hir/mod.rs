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
    pub entry: Option<FunctionId>,
}

impl Package {
    pub fn function(&self, id: FunctionId) -> &Function {
        &self.functions[id.0 as usize]
    }

    pub fn strukt(&self, id: StructId) -> &Struct {
        &self.structs[id.0 as usize]
    }

    pub fn is_copy(&self, ty: TypeId) -> bool {
        match self.types.kind(ty) {
            TypeKind::Bool
            | TypeKind::Int(_)
            | TypeKind::Float(_)
            | TypeKind::Rune
            | TypeKind::String
            | TypeKind::Error
            | TypeKind::Slice { .. } => true,
            // A closure may hold exclusive borrows, so it is never duplicated.
            TypeKind::Func(_) => false,
            TypeKind::Struct(id) => {
                let strukt = self.strukt(id);
                strukt.drop.is_none() && strukt.fields.iter().all(|f| self.is_copy(f.ty))
            }
            TypeKind::Array { element, .. } => self.is_copy(element),
            TypeKind::DynArray { .. } | TypeKind::Map { .. } => false,
        }
    }

    /// Slices and closures hold borrows; a slice's own elements are not part of the value.
    pub fn contains_view(&self, ty: TypeId) -> bool {
        self.contains(ty, &|kind| {
            matches!(kind, TypeKind::Slice { .. } | TypeKind::Func(_))
        })
    }

    pub fn contains_mut_view(&self, ty: TypeId) -> bool {
        self.contains(ty, &|kind| {
            matches!(
                kind,
                TypeKind::Slice { mutable: true, .. } | TypeKind::Func(_)
            )
        })
    }

    pub fn contains_array(&self, ty: TypeId) -> bool {
        self.contains(ty, &|kind| matches!(kind, TypeKind::Array { .. }))
    }

    fn contains(&self, ty: TypeId, matches: &dyn Fn(TypeKind) -> bool) -> bool {
        let kind = self.types.kind(ty);
        if matches(kind) {
            return true;
        }
        match kind {
            TypeKind::Struct(id) => self
                .strukt(id)
                .fields
                .iter()
                .any(|field| self.contains(field.ty, matches)),
            TypeKind::Array { element, .. } | TypeKind::DynArray { element } => {
                self.contains(element, matches)
            }
            TypeKind::Map { value, .. } => self.contains(value, matches),
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct Struct {
    pub name: String,
    pub span: Span,
    pub fields: Vec<Field>,
    /// Makes the struct Move.
    pub drop: Option<FunctionId>,
}

#[derive(Debug)]
pub struct Field {
    pub name: String,
    pub ty: TypeId,
    pub span: Span,
}
