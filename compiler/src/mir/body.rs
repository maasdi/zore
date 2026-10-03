use super::block::{BasicBlock, BlockId};
use crate::resolve::FunctionId;
use crate::types::TypeId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Local(pub u32);

#[derive(Debug)]
pub struct Program {
    pub bodies: Vec<Body>,
    /// The entry point.
    pub entry: Option<FunctionId>,
}

#[derive(Debug)]
pub struct Body {
    pub function: FunctionId,
    pub name: String,
    pub locals: Vec<LocalDecl>,
    /// Parameter locals, in order.
    pub params: Vec<Local>,
    /// Result locals, in order; `Return` returns their values.
    pub returns: Vec<Local>,
    pub blocks: Vec<BasicBlock>,
    pub unwind: Option<BlockId>,
}

#[derive(Debug)]
pub struct LocalDecl {
    pub ty: TypeId,
    /// Source name for user bindings; `None` for temporaries.
    pub name: Option<String>,
    /// The local holds a reference to a value of `ty`, which is what its places designate.
    pub by_reference: bool,
}
