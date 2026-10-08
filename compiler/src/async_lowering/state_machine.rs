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
}
