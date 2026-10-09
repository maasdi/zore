use std::collections::HashSet;

use crate::hir;
use crate::mir::{self, Operand, Place, Projection, Rvalue, Statement, Terminator};

use super::LocalStorage;

pub(super) fn classify(
    body: &mir::Body,
    suspensions: &[(mir::BlockId, super::Suspension)],
    budget_blocks: &[mir::BlockId],
) -> Vec<LocalStorage> {
    let mut frame = vec![false; body.locals.len()];
    for &local in body
        .params
        .iter()
        .chain(&body.captures)
        .chain(&body.returns)
    {
        frame[local.0 as usize] = true;
    }
    for (index, local) in body.locals.iter().enumerate() {
        if local.by_reference {
            frame[index] = true;
        }
    }
    for block in &body.blocks {
        for statement in &block.statements {
            if let Statement::Assign { rvalue, .. } = statement {
                stable_rvalue(rvalue, &mut frame);
            }
        }
        if let Terminator::Call { args, .. } = &block.terminator {
            for argument in args {
                stable_operand(argument, &mut frame);
            }
        } else if let Terminator::Assert { rvalue, .. } = &block.terminator {
            stable_rvalue(rvalue, &mut frame);
        }
    }

    let mut reachable = vec![false; body.blocks.len()];
    let mut pending = Vec::new();
    for (block, _) in suspensions {
        if let Terminator::Call {
            target,
            unwind,
            destinations,
            ..
        } = &body.blocks[block.0 as usize].terminator
        {
            pending.push(*target);
            pending.extend(*unwind);
            for destination in destinations.iter().flatten() {
                visit_place(destination, &mut |local| frame[local] = true);
            }
        }
    }
    while let Some(block) = pending.pop() {
        let index = block.0 as usize;
        if reachable[index] {
            continue;
        }
        reachable[index] = true;
        let terminator = &body.blocks[index].terminator;
        pending.extend(terminator.successors());
        if let Terminator::Call {
            unwind: Some(unwind),
            ..
        }
        | Terminator::Assert {
            unwind: Some(unwind),
            ..
        } = terminator
        {
            pending.push(*unwind);
        }
    }
    for (index, block) in body.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for statement in &block.statements {
            visit_statement(statement, &mut |local| frame[local] = true);
        }
        visit_terminator(&block.terminator, &mut |local| frame[local] = true);
    }

    if !budget_blocks.is_empty() {
        let live_at_entry = live_at_entry(body);
        for block in budget_blocks {
            for (index, live) in live_at_entry[block.0 as usize].iter().enumerate() {
                frame[index] |= *live;
            }
        }
    }

    frame
        .into_iter()
        .map(|needed| {
            if needed {
                LocalStorage::Frame
            } else {
                LocalStorage::Poll
            }
        })
        .collect()
}

fn live_at_entry(body: &mir::Body) -> Vec<Vec<bool>> {
    let block_count = body.blocks.len();
    let local_count = body.locals.len();
    let mut used_before_definition = vec![vec![false; local_count]; block_count];
    let mut defined = vec![vec![false; local_count]; block_count];
    for (index, block) in body.blocks.iter().enumerate() {
        for statement in &block.statements {
            match statement {
                Statement::Assign { place, rvalue, .. } => {
                    visit_rvalue(rvalue, &mut |local| {
                        if !defined[index][local] {
                            used_before_definition[index][local] = true;
                        }
                    });
                    if place.projections.is_empty() {
                        defined[index][place.local.0 as usize] = true;
                    } else {
                        visit_place(place, &mut |local| {
                            if !defined[index][local] {
                                used_before_definition[index][local] = true;
                            }
                        });
                    }
                }
                Statement::Drop { place, .. } => visit_place(place, &mut |local| {
                    if !defined[index][local] {
                        used_before_definition[index][local] = true;
                    }
                }),
                Statement::EndScope(locals) => {
                    for local in locals {
                        let local = local.0 as usize;
                        if !defined[index][local] {
                            used_before_definition[index][local] = true;
                        }
                    }
                }
            }
        }
        visit_terminator(&block.terminator, &mut |local| {
            if !defined[index][local] {
                used_before_definition[index][local] = true;
            }
        });
    }
    let mut live = vec![vec![false; local_count]; block_count];
    loop {
        let mut changed = false;
        for index in (0..block_count).rev() {
            let mut next = used_before_definition[index].clone();
            let terminator = &body.blocks[index].terminator;
            let mut successors = terminator.successors();
            match terminator {
                Terminator::Call {
                    unwind: Some(unwind),
                    ..
                }
                | Terminator::Assert {
                    unwind: Some(unwind),
                    ..
                } => successors.push(*unwind),
                _ => {}
            }
            for successor in successors {
                for (local, needed) in live[successor.0 as usize].iter().enumerate() {
                    if !defined[index][local] {
                        next[local] |= *needed;
                    }
                }
            }
            if next != live[index] {
                live[index] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    live
}

pub(super) fn reuse(
    body: &mir::Body,
    package: &hir::Package,
    storage: &[LocalStorage],
) -> Vec<usize> {
    let count = body.locals.len();
    let mut eligible: Vec<bool> = body
        .locals
        .iter()
        .enumerate()
        .map(|(index, local)| {
            storage[index] == LocalStorage::Frame
                && !local.by_reference
                && !package.needs_drop(local.ty)
                && !package.contains_view(local.ty)
        })
        .collect();
    for local in body
        .params
        .iter()
        .chain(&body.captures)
        .chain(&body.returns)
    {
        eligible[local.0 as usize] = false;
    }
    let mut stable = vec![false; count];
    for block in &body.blocks {
        for statement in &block.statements {
            if let Statement::Assign { rvalue, .. } = statement {
                stable_rvalue(rvalue, &mut stable);
            }
        }
        match &block.terminator {
            Terminator::Call { args, .. } => {
                for arg in args {
                    stable_operand(arg, &mut stable);
                }
            }
            Terminator::Assert { rvalue, .. } => stable_rvalue(rvalue, &mut stable),
            _ => {}
        }
    }
    for (index, is_stable) in stable.into_iter().enumerate() {
        eligible[index] &= !is_stable;
    }

    let mut entry = vec![vec![false; count]; body.blocks.len()];
    loop {
        let mut changed = false;
        for (index, block) in body.blocks.iter().enumerate().rev() {
            let mut live = successor_liveness(body, &entry, &block.terminator);
            transfer_terminator(&block.terminator, &mut live);
            for statement in block.statements.iter().rev() {
                transfer_statement(statement, &mut live);
            }
            if live != entry[index] {
                entry[index] = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut interference = vec![HashSet::new(); count];
    for block in &body.blocks {
        let mut live = successor_liveness(body, &entry, &block.terminator);
        mark_overlap(&live, &eligible, &mut interference);
        let mut touched = live.clone();
        visit_terminator(&block.terminator, &mut |local| touched[local] = true);
        mark_overlap(&touched, &eligible, &mut interference);
        transfer_terminator(&block.terminator, &mut live);
        mark_overlap(&live, &eligible, &mut interference);
        for statement in block.statements.iter().rev() {
            let mut touched = live.clone();
            visit_statement(statement, &mut |local| touched[local] = true);
            mark_overlap(&touched, &eligible, &mut interference);
            transfer_statement(statement, &mut live);
            mark_overlap(&live, &eligible, &mut interference);
        }
    }

    let mut reuse: Vec<usize> = (0..count).collect();
    for index in 0..count {
        if !eligible[index] {
            continue;
        }
        for owner in 0..index {
            if !eligible[owner]
                || reuse[owner] != owner
                || body.locals[owner].ty != body.locals[index].ty
            {
                continue;
            }
            if (0..index)
                .any(|member| reuse[member] == owner && interference[index].contains(&member))
            {
                continue;
            }
            reuse[index] = owner;
            break;
        }
    }
    reuse
}

fn successor_liveness(body: &mir::Body, entry: &[Vec<bool>], terminator: &Terminator) -> Vec<bool> {
    let mut live = vec![false; body.locals.len()];
    let mut successors = terminator.successors();
    match terminator {
        Terminator::Call {
            unwind: Some(unwind),
            ..
        }
        | Terminator::Assert {
            unwind: Some(unwind),
            ..
        } => successors.push(*unwind),
        _ => {}
    }
    for successor in successors {
        for (index, used) in entry[successor.0 as usize].iter().enumerate() {
            live[index] |= used;
        }
    }
    live
}

fn transfer_terminator(terminator: &Terminator, live: &mut [bool]) {
    match terminator {
        Terminator::Call {
            callee,
            args,
            destinations,
            ..
        } => {
            for place in destinations.iter().flatten() {
                visit_place(place, &mut |local| live[local] = true);
            }
            if let mir::Callee::Value(place) = callee {
                visit_place(place, &mut |local| live[local] = true);
            }
            for arg in args {
                visit_operand(arg, &mut |local| live[local] = true);
            }
        }
        _ => visit_terminator(terminator, &mut |local| live[local] = true),
    }
}

fn transfer_statement(statement: &Statement, live: &mut [bool]) {
    if let Statement::Assign { place, rvalue, .. } = statement {
        if place.projections.is_empty() {
            live[place.local.0 as usize] = false;
        } else {
            visit_place(place, &mut |local| live[local] = true);
        }
        visit_rvalue(rvalue, &mut |local| live[local] = true);
    } else {
        visit_statement(statement, &mut |local| live[local] = true);
    }
}

fn mark_overlap(live: &[bool], eligible: &[bool], interference: &mut [HashSet<usize>]) {
    let active: Vec<_> = live
        .iter()
        .enumerate()
        .filter_map(|(index, used)| (*used && eligible[index]).then_some(index))
        .collect();
    for (position, &left) in active.iter().enumerate() {
        for &right in &active[position + 1..] {
            interference[left].insert(right);
            interference[right].insert(left);
        }
    }
}

fn stable_rvalue(rvalue: &Rvalue, stable: &mut [bool]) {
    match rvalue {
        Rvalue::Ref(place) | Rvalue::Slice { place, .. } | Rvalue::MapValueRef(place, _) => {
            visit_place(place, &mut |local| stable[local] = true);
        }
        Rvalue::Closure { captures, .. } => {
            for (place, _) in captures {
                visit_place(place, &mut |local| stable[local] = true);
            }
        }
        _ => {}
    }
}

fn stable_operand(operand: &Operand, stable: &mut [bool]) {
    if let Operand::Ref(place) = operand {
        visit_place(place, &mut |local| stable[local] = true);
    }
}

fn visit_place(place: &Place, visit: &mut impl FnMut(usize)) {
    visit(place.local.0 as usize);
    for projection in &place.projections {
        if let Projection::Index(index) = projection {
            visit_operand(index, visit);
        }
    }
}

fn visit_operand(operand: &Operand, visit: &mut impl FnMut(usize)) {
    if let Operand::Copy(place) | Operand::Move(place) | Operand::Ref(place) = operand {
        visit_place(place, visit);
    }
}

fn visit_rvalue(rvalue: &Rvalue, visit: &mut impl FnMut(usize)) {
    match rvalue {
        Rvalue::Use(value)
        | Rvalue::Unary(_, value)
        | Rvalue::Convert(value, _)
        | Rvalue::Error(value)
        | Rvalue::Spawn(value) => visit_operand(value, visit),
        Rvalue::Binary(_, left, right)
        | Rvalue::BoundsCheck(left, right)
        | Rvalue::StringChar(left, right)
        | Rvalue::StringAdvance(left, right) => {
            visit_operand(left, visit);
            visit_operand(right, visit);
        }
        Rvalue::Length(place) | Rvalue::Ref(place) => visit_place(place, visit),
        Rvalue::MapKeyAt(place, index) | Rvalue::MapValueRef(place, index) => {
            visit_place(place, visit);
            visit_operand(index, visit);
        }
        Rvalue::StringSlice { source, low, high } => {
            visit_operand(source, visit);
            for bound in low.iter().chain(high) {
                visit_operand(bound, visit);
            }
        }
        Rvalue::Slice {
            place, low, high, ..
        } => {
            visit_place(place, visit);
            for bound in low.iter().chain(high) {
                visit_operand(bound, visit);
            }
        }
        Rvalue::Aggregate(_, values) => {
            for value in values {
                visit_operand(value, visit);
            }
        }
        Rvalue::Closure { captures, .. } => {
            for (place, _) in captures {
                visit_place(place, visit);
            }
        }
        Rvalue::Zero => {}
    }
}

fn visit_statement(statement: &Statement, visit: &mut impl FnMut(usize)) {
    match statement {
        Statement::Assign { place, rvalue, .. } => {
            visit_place(place, visit);
            visit_rvalue(rvalue, visit);
        }
        Statement::Drop { place, .. } => visit_place(place, visit),
        Statement::EndScope(locals) => {
            for local in locals {
                visit(local.0 as usize);
            }
        }
    }
}

fn visit_terminator(terminator: &Terminator, visit: &mut impl FnMut(usize)) {
    match terminator {
        Terminator::Branch { condition, .. } => visit_operand(condition, visit),
        Terminator::Call {
            callee,
            args,
            destinations,
            ..
        } => {
            if let mir::Callee::Value(place) = callee {
                visit_place(place, visit);
            }
            for argument in args {
                visit_operand(argument, visit);
            }
            for destination in destinations.iter().flatten() {
                visit_place(destination, visit);
            }
        }
        Terminator::Assert { place, rvalue, .. } => {
            visit_place(place, visit);
            visit_rvalue(rvalue, visit);
        }
        Terminator::Goto(_)
        | Terminator::Return
        | Terminator::PanicReturn
        | Terminator::Unreachable => {}
    }
}
