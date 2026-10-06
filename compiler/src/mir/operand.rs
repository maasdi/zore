use super::body::{Local, LocalDecl};
use crate::hir::{self, Const};
use crate::resolve::FieldId;
use crate::types::{TypeId, TypeKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    Field(FieldId),
    /// Already bounds-checked.
    Index(Operand),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub local: Local,
    pub projections: Vec<Projection>,
}

impl Place {
    pub fn local(local: Local) -> Self {
        Self {
            local,
            projections: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Copy(Place),
    Move(Place),
    /// Only call arguments use references.
    Ref(Place),
    Const(Const, TypeId),
}

pub fn projection_type(package: &hir::Package, ty: TypeId, projection: &Projection) -> TypeId {
    match projection {
        Projection::Field(field) => {
            let id = package
                .types
                .struct_id(ty)
                .expect("field projection on a struct");
            package.strukt(id).fields[field.0 as usize].ty
        }
        Projection::Index(_) => match package.types.kind(ty) {
            TypeKind::Array { element, .. }
            | TypeKind::Slice { element, .. }
            | TypeKind::DynArray { element } => element,
            _ => unreachable!("index projection on a non-array, non-slice"),
        },
    }
}

pub fn place_type(package: &hir::Package, locals: &[LocalDecl], place: &Place) -> TypeId {
    place
        .projections
        .iter()
        .fold(locals[place.local.0 as usize].ty, |ty, projection| {
            projection_type(package, ty, projection)
        })
}
