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
    let mut pending = budget_blocks.to_vec();
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
