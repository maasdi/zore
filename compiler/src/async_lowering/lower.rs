use std::collections::{BTreeSet, HashSet};

use crate::hir;
use crate::mir::{self, Callee, Terminator};

use super::{Plan, StateMachine, Suspension};

pub fn lower(package: &hir::Package, program: &mir::Program) -> Plan {
    let mut waiting_library = HashSet::new();
    loop {
        let before = waiting_library.len();
        for body in &program.bodies {
            let function = package.function(body.function);
            if function.native || function.is_closure || !function.name.starts_with("zore/") {
                continue;
            }
            if body.blocks.iter().any(|block| match &block.terminator {
                Terminator::Call { callee, .. } => match callee {
                    Callee::Function(id) => {
                        waiting_library.contains(id)
                            || super::native_wait(&package.function(*id).name).is_some()
                            || package.function(*id).name == "zore/time.Sleep"
                    }
                    Callee::ChannelSend
                    | Callee::ChannelReceive
                    | Callee::Select { .. }
                    | Callee::MutexWithLock
                    | Callee::TaskWait => true,
                    _ => false,
                },
                _ => false,
            }) {
                waiting_library.insert(body.function);
            }
        }
        if waiting_library.len() == before {
            break;
        }
    }
    let mut eligible: HashSet<_> = program
        .bodies
        .iter()
        .filter(|body| {
            let function = package.function(body.function);
            !function.native && (function.is_async || waiting_library.contains(&body.function))
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
            let suspensions: Vec<_> = body
                .blocks
                .iter()
                .enumerate()
                .filter_map(|(index, block)| {
                    let Terminator::Call { callee, .. } = &block.terminator else {
                        return None;
                    };
                    let suspension = match callee {
                        Callee::Function(id) if eligible.contains(id) => Suspension::Call(*id),
                        Callee::Value(place)
                            if package
                                .types
                                .func_signature(mir::place_type(package, &body.locals, place))
                                .is_some_and(|signature| signature.is_async) =>
                        {
                            Suspension::Value
                        }
                        Callee::Function(id)
                            if package.function(*id).native
                                && package.function(*id).name == "zore/time.Sleep" =>
                        {
                            Suspension::Sleep
                        }
                        Callee::Function(id)
                            if package.function(*id).native
                                && super::native_wait(&package.function(*id).name).is_some() =>
                        {
                            Suspension::Io(*id)
                        }
                        Callee::MutexWithLock => Suspension::Mutex,
                        Callee::TaskWait => Suspension::Task,
                        Callee::ChannelSend | Callee::ChannelReceive | Callee::Select { .. } => {
                            Suspension::Channel
                        }
                        _ => return None,
                    };
                    Some((mir::BlockId(index as u32), suspension))
                })
                .collect();
            let budget_blocks = budget_blocks(body);
            let storage = super::storage::classify(body, &suspensions, &budget_blocks);
            let (frame_reuse, frame_reuse_work) = super::storage::reuse(body, package, &storage);
            (
                body.function,
                StateMachine {
                    suspensions,
                    budget_blocks,
                    storage,
                    frame_reuse,
                    frame_reuse_work,
                },
            )
        })
        .collect();
    Plan { machines }
}

fn budget_blocks(body: &mir::Body) -> Vec<mir::BlockId> {
    let count = body.blocks.len();
    let mut predecessors = vec![Vec::new(); count];
    for (index, block) in body.blocks.iter().enumerate() {
        for target in block.terminator.successors() {
            predecessors[target.0 as usize].push(index);
        }
    }
    let mut reachable = vec![false; count];
    let mut pending = vec![0];
    while let Some(index) = pending.pop() {
        if reachable[index] {
            continue;
        }
        reachable[index] = true;
        pending.extend(
            body.blocks[index]
                .terminator
                .successors()
                .into_iter()
                .map(|target| target.0 as usize),
        );
    }
    let mut dominators = vec![reachable.clone(); count];
    dominators[0].fill(false);
    dominators[0][0] = true;
    loop {
        let mut changed = false;
        for index in 1..count {
            if !reachable[index] {
                continue;
            }
            let mut next = reachable.clone();
            for &predecessor in &predecessors[index] {
                if reachable[predecessor] {
                    for (candidate, dominated) in next.iter_mut().enumerate() {
                        *dominated &= dominators[predecessor][candidate];
                    }
                }
            }
            next[index] = true;
            if next != dominators[index] {
                dominators[index] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut headers = BTreeSet::new();
    for (index, block) in body.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for target in block.terminator.successors() {
            if dominators[index][target.0 as usize] {
                headers.insert(target);
            }
        }
    }
    headers.into_iter().collect()
}
