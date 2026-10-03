//! Interned semantic types.

pub mod bignum;
pub mod constant;
mod ty;
mod type_id;
mod type_store;

pub use ty::{FloatType, IntType, TypeKind};
pub use type_id::{StructId, TypeId};
pub use type_store::{TypeName, TypeStore};
