use std::collections::HashMap;

use crate::mir::BlockId;
use crate::resolve::FunctionId;

use super::Suspension;

#[derive(Debug, Default)]
pub struct Plan {
    pub machines: HashMap<FunctionId, StateMachine>,
}

#[derive(Debug)]
pub struct StateMachine {
    pub suspensions: Vec<(BlockId, Suspension)>,
    pub budget_blocks: Vec<BlockId>,
    pub storage: Vec<LocalStorage>,
    pub frame_reuse: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalStorage {
    Frame,
    Poll,
}
