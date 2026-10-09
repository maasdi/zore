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
    /// Bitset words and bits the slot reuse analysis read or wrote; deterministic, unlike its run time.
    pub frame_reuse_work: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalStorage {
    Frame,
    Poll,
}
