mod expr;
mod function;
pub mod lower;
mod stmt;

use std::collections::HashSet;

pub use expr::{Const, Expr, ExprKind, Place, Projection};
pub use function::{Function, Local};
pub use stmt::{Block, SelectArm, SelectComm, Stmt, StmtKind};

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
            TypeKind::Func(_) | TypeKind::Task(_) => false,
            TypeKind::Channel { .. } | TypeKind::Mutex { .. } => true,
            TypeKind::Struct(id) => {
                let strukt = self.strukt(id);
                strukt.drop.is_none() && strukt.fields.iter().all(|f| self.is_copy(f.ty))
            }
            TypeKind::Array { element, .. } => self.is_copy(element),
            TypeKind::DynArray { .. } | TypeKind::Map { .. } => false,
        }
    }

    /// Whether a value of `ty` owns a share of some runtime text, channel, or mutex.
    pub fn holds_shared(&self, ty: TypeId) -> bool {
        match self.types.kind(ty) {
            TypeKind::String
            | TypeKind::Error
            | TypeKind::Channel { .. }
            | TypeKind::Mutex { .. } => true,
            TypeKind::Struct(id) => self
                .strukt(id)
                .fields
                .iter()
                .any(|f| self.holds_shared(f.ty)),
            TypeKind::Array { element, .. } => self.holds_shared(element),
            _ => false,
        }
    }

    /// Copy values that hold text are still copied freely, but each copy shares the text.
    pub fn copies_shared(&self, ty: TypeId) -> bool {
        self.is_copy(ty) && self.holds_shared(ty)
    }

    /// Destroying a value of `ty` does something: it runs cleanup or gives up shared text.
    pub fn needs_drop(&self, ty: TypeId) -> bool {
        !self.is_copy(ty) || self.holds_shared(ty)
    }

    /// Slices and closures hold borrows; a slice's own elements are not part of the value.
    pub fn contains_view(&self, ty: TypeId) -> bool {
        self.contains(ty, &|kind| {
            matches!(kind, TypeKind::Slice { .. } | TypeKind::Func(_))
        })
    }

    pub fn contains_mut_slice_of_views(&self, ty: TypeId) -> bool {
        self.contains(ty, &|kind| match kind {
            TypeKind::Slice {
                element,
                mutable: true,
            } => self.contains_view(element),
            _ => false,
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

    /// Whether destroying a value of `ty` runs a custom `drop` that can read a borrow.
    pub fn drop_observes_view(&self, ty: TypeId) -> bool {
        self.contains(ty, &|kind| match kind {
            TypeKind::Struct(id) => {
                self.strukt(id).drop.is_some() && self.contains_view(self.types.struct_type(id))
            }
            _ => false,
        })
    }

    pub fn contains_array(&self, ty: TypeId) -> bool {
        self.contains(ty, &|kind| matches!(kind, TypeKind::Array { .. }))
    }

    fn contains(&self, ty: TypeId, matches: &dyn Fn(TypeKind) -> bool) -> bool {
        let mut pending = vec![ty];
        let mut seen = HashSet::new();
        while let Some(ty) = pending.pop() {
            if !seen.insert(ty) {
                continue;
            }
            let kind = self.types.kind(ty);
            if matches(kind) {
                return true;
            }
            match kind {
                TypeKind::Struct(id) => {
                    pending.extend(self.strukt(id).fields.iter().map(|field| field.ty));
                }
                TypeKind::Array { element, .. } | TypeKind::DynArray { element } => {
                    pending.push(element);
                }
                TypeKind::Map { value, .. } => pending.push(value),
                _ => {}
            }
        }
        false
    }
}

#[derive(Debug)]
pub struct Struct {
    pub name: String,
    pub span: Span,
    pub fields: Vec<Field>,
    /// Makes the struct Move.
    pub drop: Option<FunctionId>,
    pub clone: Option<FunctionId>,
}

#[derive(Debug)]
pub struct Field {
    pub name: String,
    pub ty: TypeId,
    pub span: Span,
}
