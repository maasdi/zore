use super::block::{BasicBlock, BlockId};
use crate::resolve::FunctionId;
use crate::types::TypeId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Local(pub u32);

#[derive(Debug)]
pub struct Program {
    pub bodies: Vec<Body>,
    pub entry: Option<FunctionId>,
}

#[derive(Debug)]
pub struct Body {
    pub function: FunctionId,
    pub name: String,
    pub locals: Vec<LocalDecl>,
    pub params: Vec<Local>,
    /// Closure capture locals, in environment order; each holds a reference.
    pub captures: Vec<Local>,
    /// `Return` returns their values.
    pub returns: Vec<Local>,
    pub blocks: Vec<BasicBlock>,
    pub unwind: Option<BlockId>,
}

#[derive(Debug)]
pub struct LocalDecl {
    pub ty: TypeId,
    /// `None` for temporaries.
    pub name: Option<String>,
    /// The local's places designate the referenced value.
    pub by_reference: bool,
}
