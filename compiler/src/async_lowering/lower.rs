use std::collections::HashSet;

use crate::hir;
use crate::mir::{self, Callee, Terminator};

use super::{Plan, StateMachine, Suspension};

/// Lowers async call/task and channel waits. Remaining waiting primitives and native async functions
/// retain fiber execution until their poll variants land. Propagate that fallback through
/// awaited calls, including recursion, so a state machine never synchronously awaits a fiber.
pub fn lower(package: &hir::Package, program: &mir::Program) -> Plan {
    let mut eligible: HashSet<_> = program
        .bodies
        .iter()
        .filter(|body| {
            let function = package.function(body.function);
            function.is_async
                && !function.native
                && !body.blocks.iter().any(|block| match &block.terminator {
                    Terminator::Call { callee, .. } => {
                        super::suspension::needs_fiber(package, callee)
                    }
                    _ => false,
                })
        })
        .map(|body| body.function)
        .collect();
    loop {
        let removed: Vec<_> = program
            .bodies
            .iter()
            .filter(|body| {
                eligible.contains(&body.function)
                    && body.blocks.iter().any(|block| {
                        let Terminator::Call {
                            callee: Callee::Function(id),
                            ..
                        } = &block.terminator
                        else {
                            return false;
                        };
                        package.function(*id).is_async && !eligible.contains(id)
                    })
            })
            .map(|body| body.function)
            .collect();
        if removed.is_empty() {
            break;
        }
        for id in removed {
            eligible.remove(&id);
        }
    }
    let machines = program
        .bodies
        .iter()
        .filter(|body| eligible.contains(&body.function))
        .map(|body| {
            let suspensions = body
                .blocks
                .iter()
                .enumerate()
                .filter_map(|(index, block)| {
                    let Terminator::Call { callee, .. } = &block.terminator else {
                        return None;
                    };
                    let suspension = match callee {
                        Callee::Function(id) if eligible.contains(id) => Suspension::Call(*id),
                        Callee::TaskWait => Suspension::Task,
                        Callee::ChannelSend | Callee::ChannelReceive | Callee::Select { .. } => {
                            Suspension::Channel
                        }
                        _ => return None,
                    };
                    Some((mir::BlockId(index as u32), suspension))
                })
                .collect();
            (body.function, StateMachine { suspensions })
        })
        .collect();
    Plan { machines }
}
