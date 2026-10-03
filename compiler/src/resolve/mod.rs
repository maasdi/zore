//! Name resolution: assigns stable IDs and records what each name refers to.

mod ids;
mod resolver;
mod scope;
mod symbol;

pub use ids::{ConstId, FieldId, FunctionId, LocalId};
pub use resolver::{Resolution, resolve};
pub use symbol::{ConstDecl, LocalDecl, LocalKind, Res};
