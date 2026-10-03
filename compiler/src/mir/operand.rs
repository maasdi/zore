use super::body::Local;
use crate::hir::Const;
use crate::resolve::FieldId;
use crate::types::TypeId;

/// A step from a place into one of its parts.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    Field(FieldId),
    /// A fixed-array element; the index is already bounds-checked.
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
    /// A reference to the place; only call arguments use it.
    Ref(Place),
    Const(Const, TypeId),
}
