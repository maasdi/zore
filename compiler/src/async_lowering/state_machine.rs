use std::collections::HashMap;

use crate::mir::BlockId;
use crate::resolve::FunctionId;

use super::Suspension;

#[derive(Debug, Default)]
pub struct Plan {
    pub machines: HashMap<FunctionId, StateMachine>,
}

/// All MIR locals and drop flags belong to the frame. States resume the poll part of a
/// call, skipping its argument evaluation and constructor so ownership transfers only once.
#[derive(Debug)]
pub struct StateMachine {
    pub suspensions: Vec<(BlockId, Suspension)>,
}
