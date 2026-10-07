pub mod bignum;
pub mod constant;
mod ty;
mod type_id;
mod type_store;

pub use ty::{FloatType, FuncSignature, IntType, TypeKind};
pub use type_id::{FuncTypeId, StructId, TaskTypeId, TypeId};
pub use type_store::{TypeName, TypeStore};
