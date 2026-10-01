use crate::diagnostic::{Diagnostic, Severity};
use crate::mir::{BasicBlock, BlockId, Body, Callee, Operand, Place, Program, Rvalue, Terminator};
use crate::source::Span;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValueState {
    Available,
    Moved(Span),
    PartiallyMoved(Span),
}

impl ValueState {
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Available, Self::Available) => Self::Available,
            (Self::PartiallyMoved(span), _) | (_, Self::PartiallyMoved(span)) => {
                Self::PartiallyMoved(span)
            }
            (Self::Moved(span), _) | (_, Self::Moved(span)) => Self::Moved(span),
        }
    }

    fn move_site(self) -> Option<Span> {
        match self {
            Self::Available => None,
            Self::Moved(span) | Self::PartiallyMoved(span) => Some(span),
        }
    }
}

pub fn check(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for body in &program.bodies {
        check_body(body, &mut diagnostics);
    }
    diagnostics
}

fn check_body(body: &Body, diagnostics: &mut Vec<Diagnostic>) {
    if body.blocks.is_empty() {
        return;
    }
    let initial = vec![ValueState::Available; body.locals.len()];
    let mut incoming = vec![None; body.blocks.len()];
    incoming[0] = Some(initial);
    let mut changed = true;
    while changed {
        changed = false;
        for (index, block) in body.blocks.iter().enumerate() {
            let Some(mut state) = incoming[index].clone() else {
                continue;
            };
            transfer(body, block, &mut state, &mut Vec::new());
            for successor in successors(&block.terminator) {
                let slot = &mut incoming[successor.0 as usize];
                let joined = match slot {
                    Some(previous) => previous
                        .iter()
                        .zip(&state)
                        .map(|(&a, &b)| a.join(b))
                        .collect(),
                    None => state.clone(),
                };
                if slot.as_ref() != Some(&joined) {
                    *slot = Some(joined);
                    changed = true;
                }
            }
        }
    }
    for (block, state) in body.blocks.iter().zip(incoming) {
        if let Some(mut state) = state {
            transfer(body, block, &mut state, diagnostics);
        }
    }
}

fn successors(terminator: &Terminator) -> Vec<BlockId> {
    match terminator {
        Terminator::Goto(target) => vec![*target],
        Terminator::Branch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        Terminator::Call { target, .. } | Terminator::Assert { target, .. } => vec![*target],
        Terminator::Return | Terminator::PanicReturn | Terminator::Unreachable => Vec::new(),
    }
}

fn transfer(
    body: &Body,
    block: &BasicBlock,
    state: &mut [ValueState],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in &block.statements {
        if let crate::mir::Statement::Assign {
            place,
            rvalue,
            span,
        } = statement
        {
            check_rvalue(body, rvalue, *span, state, diagnostics);
            assign(body, place, *span, state, diagnostics);
        }
    }
    match &block.terminator {
        Terminator::Assert {
            place,
            rvalue,
            span,
            ..
        } => {
            check_rvalue(body, rvalue, *span, state, diagnostics);
            assign(body, place, *span, state, diagnostics);
        }
        Terminator::Branch {
            condition, span, ..
        } => {
            check_operand(body, condition, *span, state, diagnostics);
        }
        Terminator::Call {
            callee,
            args,
            destinations,
            span,
            ..
        } => {
            for (index, arg) in args.iter().enumerate() {
                let Operand::Move(moved) = arg else {
                    continue;
                };
                for (other_index, other) in args.iter().enumerate() {
                    if index == other_index {
                        continue;
                    }
                    let Operand::Ref(borrowed) = other else {
                        continue;
                    };
                    if places_overlap(moved, borrowed) {
                        let name = body.locals[moved.local.0 as usize]
                            .name
                            .as_deref()
                            .unwrap_or("temporary value");
                        diagnostics.push(Diagnostic::new(
                            Severity::Error,
                            format!("cannot move `{name}` while it is borrowed by this call"),
                            *span,
                        ));
                    }
                }
            }
            for arg in args {
                check_operand(body, arg, *span, state, diagnostics);
            }
            if !matches!(callee, Callee::Drop) {
                for place in destinations.iter().flatten() {
                    assign(body, place, *span, state, diagnostics);
                }
            }
        }
        Terminator::Goto(_)
        | Terminator::Return
        | Terminator::PanicReturn
        | Terminator::Unreachable => {}
    }
}

fn check_rvalue(
    body: &Body,
    rvalue: &Rvalue,
    span: Span,
    state: &mut [ValueState],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match rvalue {
        Rvalue::Zero => {}
        Rvalue::Use(operand)
        | Rvalue::Unary(_, operand)
        | Rvalue::Convert(operand, _)
        | Rvalue::Error(operand) => {
            check_operand(body, operand, span, state, diagnostics);
        }
        Rvalue::Binary(_, left, right) => {
            check_operand(body, left, span, state, diagnostics);
            check_operand(body, right, span, state, diagnostics);
        }
        Rvalue::Aggregate(_, fields) => {
            for field in fields {
                check_operand(body, field, span, state, diagnostics);
            }
        }
    }
}

fn places_overlap(left: &Place, right: &Place) -> bool {
    left.local == right.local && left.fields.iter().zip(&right.fields).all(|(a, b)| a == b)
}

fn check_operand(
    body: &Body,
    operand: &Operand,
    span: Span,
    state: &mut [ValueState],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (place, moving) = match operand {
        Operand::Copy(place) | Operand::Ref(place) => (place, false),
        Operand::Move(place) => (place, true),
        Operand::Const(..) => return,
    };
    let index = place.local.0 as usize;
    let name = body.locals[index]
        .name
        .as_deref()
        .unwrap_or("temporary value");
    if let Some(origin) = state[index].move_site() {
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("use of moved value `{name}`"),
                span,
            )
            .related(origin, "value moved here"),
        );
        return;
    }
    if moving && body.locals[index].by_reference {
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("cannot move borrowed value `{name}`"),
                span,
            )
            .note("a borrowed parameter does not own its argument"),
        );
        return;
    }
    if moving && !place.fields.is_empty() {
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("partial move of `{name}` is not supported yet"),
                span,
            )
            .note("moving a whole value is supported; field moves need drop tracking"),
        );
        state[index] = ValueState::PartiallyMoved(span);
        return;
    }
    if moving {
        state[index] = ValueState::Moved(span);
    }
}

fn assign(
    body: &Body,
    place: &Place,
    span: Span,
    state: &mut [ValueState],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let index = place.local.0 as usize;
    if !place.fields.is_empty() {
        if let Some(origin) = state[index].move_site() {
            let name = body.locals[index]
                .name
                .as_deref()
                .unwrap_or("temporary value");
            diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("cannot assign through moved value `{name}`"),
                    span,
                )
                .related(origin, "value moved here"),
            );
        }
        return;
    }
    state[index] = ValueState::Available;
}
