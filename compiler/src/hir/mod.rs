mod expr;
mod function;
pub mod lower;
mod stmt;

use std::collections::{HashMap, HashSet};

pub use expr::{Const, Expr, ExprKind, Place, Projection};
pub use function::{Function, Local};
pub use stmt::{Block, SelectArm, SelectComm, Stmt, StmtKind};

use crate::resolve::FunctionId;
use crate::source::Span;
use crate::types::{Constraint, InterfaceId, StructId, TypeId, TypeKind, TypeStore};

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub types: TypeStore,
    pub structs: Vec<Struct>,
    pub functions: Vec<Function>,
    pub entry: Option<FunctionId>,
    /// In initialization order.
    pub globals: Vec<Global>,
    /// Sets every global before the entry point runs.
    pub init: Option<FunctionId>,
    /// For each type converted to an interface, what serves each entry, in entry order.
    pub implementations: HashMap<(TypeId, InterfaceId), Vec<Implementation>>,
}

/// What serves one interface entry for a type converted to the interface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Implementation {
    Method(FunctionId),
    /// An entry of the source interface, for one interface converted to another.
    Entry(usize),
}

#[derive(Debug)]
pub struct Global {
    pub name: String,
    pub ty: TypeId,
    pub span: Span,
}

impl Package {
    pub fn function(&self, id: FunctionId) -> &Function {
        &self.functions[id.0 as usize]
    }

    /// A borrowed interface value is already a view, so it is passed as it is.
    pub fn passes_by_reference(&self, mode: crate::ast::ParamMode, ty: TypeId) -> bool {
        if matches!(self.types.kind(ty), TypeKind::InterfaceView { .. }) {
            return false;
        }
        match mode {
            crate::ast::ParamMode::Mut => true,
            crate::ast::ParamMode::Borrow => !self.is_copy(ty) || self.contains_array(ty),
            crate::ast::ParamMode::Own => false,
        }
    }

    pub fn interface_entry(
        &self,
        receiver: TypeId,
        method: usize,
    ) -> &crate::types::InterfaceMethod {
        let interface = self
            .types
            .interface_of(receiver)
            .expect("an interface receiver");
        &self.types.interface_methods(interface)[method]
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
            | TypeKind::Slice { .. }
            | TypeKind::InterfaceView { .. } => true,
            // A closure may hold exclusive borrows, so it is never duplicated.
            TypeKind::Func(_) | TypeKind::Task(_) | TypeKind::Interface(_) => false,
            TypeKind::Channel { .. } | TypeKind::Mutex { .. } => true,
            TypeKind::Struct(id) => {
                let strukt = self.strukt(id);
                strukt.drop.is_none() && strukt.fields.iter().all(|f| self.is_copy(f.ty))
            }
            TypeKind::Array { element, .. } => self.is_copy(element),
            TypeKind::DynArray { .. } | TypeKind::Map { .. } => false,
            TypeKind::Param(_) => matches!(
                self.types.constraint(ty),
                Some(Constraint::Copyable | Constraint::Comparable | Constraint::Ordered)
            ),
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

    /// Slices, closures, and borrowed interface values hold borrows; a slice's own elements are not part of the value.
    pub fn contains_view(&self, ty: TypeId) -> bool {
        self.contains(ty, &|kind| {
            matches!(
                kind,
                TypeKind::Slice { .. } | TypeKind::Func(_) | TypeKind::InterfaceView { .. }
            )
        })
    }

    /// Package-level `let` values hold no views, tasks, channels, or mutexes.
    pub fn storable_globally(&self, ty: TypeId) -> bool {
        self.is_copy(ty)
            && !self.contains(ty, &|kind| {
                matches!(
                    kind,
                    TypeKind::Slice { .. }
                        | TypeKind::Func(_)
                        | TypeKind::Task(_)
                        | TypeKind::Channel { .. }
                        | TypeKind::Mutex { .. }
                )
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
                TypeKind::Slice { mutable: true, .. }
                    | TypeKind::Func(_)
                    | TypeKind::InterfaceView { mutable: true, .. }
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
