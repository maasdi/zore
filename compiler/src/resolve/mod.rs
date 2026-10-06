mod ids;
mod resolver;
mod scope;
mod symbol;

pub use ids::{ConstId, FieldId, FunctionId, LocalId};
pub use resolver::{Resolution, resolve};
pub use symbol::{ClosureDecl, ConstDecl, LocalDecl, LocalKind, Res};
