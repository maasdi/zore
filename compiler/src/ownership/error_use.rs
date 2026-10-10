use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::hir;
use crate::mir::{BasicBlock, Body, Operand, Place, Program, Rvalue, Statement, Terminator};
use crate::resolve::LocalKind;
use crate::source::Span;
use crate::types::TypeStore;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UseState {
    Inactive,
    Unused,
    Used,
}

impl UseState {
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unused, _) | (_, Self::Unused) => Self::Unused,
            (Self::Used, _) | (_, Self::Used) => Self::Used,
            _ => Self::Inactive,
        }
    }
}

pub fn check(package: &hir::Package, program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for body in &program.bodies {
        check_body(package.function(body.function), body, &mut diagnostics);
    }
    diagnostics
}

fn check_body(function: &hir::Function, body: &Body, diagnostics: &mut Vec<Diagnostic>) {
    if body.blocks.is_empty() {
        return;
    }
    let mut initial = vec![UseState::Inactive; function.locals.len()];
    for param in &body.params {
        let index = param.0 as usize;
        if function.locals[index].ty == TypeStore::ERROR {
            initial[index] = UseState::Unused;
        }
    }
    let mut incoming = vec![None; body.blocks.len()];
    incoming[0] = Some(initial);
    let mut changed = true;
    while changed {
        changed = false;
        for (index, block) in body.blocks.iter().enumerate() {
            let Some(mut state) = incoming[index].clone() else {
                continue;
            };
            transfer(
                function,
                block,
                &mut state,
                &mut Vec::new(),
                &mut HashSet::new(),
            );
            for successor in block.terminator.successors() {
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
    let mut reported = HashSet::new();
    for (block, state) in body.blocks.iter().zip(incoming) {
        if let Some(mut state) = state {
            transfer(function, block, &mut state, diagnostics, &mut reported);
        }
    }
}

fn transfer(
    function: &hir::Function,
    block: &BasicBlock,
    state: &mut [UseState],
    diagnostics: &mut Vec<Diagnostic>,
    reported: &mut HashSet<(Span, usize, bool)>,
) {
    for statement in &block.statements {
        match statement {
            Statement::Assign {
                place,
                rvalue,
                span,
            } => {
                read_rvalue(rvalue, state);
                write(function, place, *span, state, diagnostics, reported);
            }
            Statement::EndScope(locals) => {
                for local in locals {
                    let index = local.0 as usize;
                    if index < state.len() {
                        report_unused(function, index, state[index], diagnostics, reported);
                        state[index] = UseState::Inactive;
                    }
                }
            }
            Statement::Drop { .. } => {}
            Statement::SetGlobal { value, .. } => read_operand(value, state),
        }
    }
    match &block.terminator {
        Terminator::Branch { condition, .. } => read_operand(condition, state),
        Terminator::Call {
            args,
            destinations,
            span,
            ..
        } => {
            for arg in args {
                read_operand(arg, state);
            }
            for place in destinations.iter().flatten() {
                write(function, place, *span, state, diagnostics, reported);
            }
        }
        Terminator::Assert {
            place,
            rvalue,
            span,
            ..
        } => {
            read_rvalue(rvalue, state);
            write(function, place, *span, state, diagnostics, reported);
        }
        Terminator::Return => {
            for (index, &value) in state.iter().enumerate() {
                report_unused(function, index, value, diagnostics, reported);
            }
        }
        Terminator::Goto(_) | Terminator::PanicReturn | Terminator::Unreachable => {}
    }
}

fn read_rvalue(rvalue: &Rvalue, state: &mut [UseState]) {
    match rvalue {
        Rvalue::Zero | Rvalue::Global(_) => {}
        Rvalue::Use(value)
        | Rvalue::Spawn(value)
        | Rvalue::Unary(_, value)
        | Rvalue::Convert(value, _)
        | Rvalue::Error(value) => read_operand(value, state),
        Rvalue::Binary(_, left, right) | Rvalue::BoundsCheck(left, right) => {
            read_operand(left, state);
            read_operand(right, state);
        }
        Rvalue::Aggregate(_, values) => {
            for value in values {
                read_operand(value, state);
            }
        }
        Rvalue::Length(_) | Rvalue::Ref(_) => {}
        Rvalue::MapKeyAt(_, position) | Rvalue::MapValueRef(_, position) => {
            read_operand(position, state)
        }
        Rvalue::Slice { low, high, .. } => {
            for bound in [low, high].into_iter().flatten() {
                read_operand(bound, state);
            }
        }
        Rvalue::StringSlice { source, low, high } => {
            read_operand(source, state);
            for bound in [low, high].into_iter().flatten() {
                read_operand(bound, state);
            }
        }
        Rvalue::StringChar(string, position) | Rvalue::StringAdvance(string, position) => {
            read_operand(string, state);
            read_operand(position, state);
        }
        // A closure may read what it captures whenever it is called.
        Rvalue::Closure { captures, .. } => {
            for (place, _) in captures {
                read_operand(&Operand::Copy(place.clone()), state);
            }
        }
    }
}

fn read_operand(operand: &Operand, state: &mut [UseState]) {
    let place = match operand {
        Operand::Copy(place) | Operand::Move(place) | Operand::Ref(place) => place,
        Operand::Const(..) => return,
    };
    let index = place.local.0 as usize;
    if place.projections.is_empty() && state.get(index) == Some(&UseState::Unused) {
        state[index] = UseState::Used;
    }
}

fn write(
    function: &hir::Function,
    place: &Place,
    span: Span,
    state: &mut [UseState],
    diagnostics: &mut Vec<Diagnostic>,
    reported: &mut HashSet<(Span, usize, bool)>,
) {
    let index = place.local.0 as usize;
    // A closure's write through a capture is for its enclosing function to use.
    if !place.projections.is_empty()
        || index >= state.len()
        || function.locals[index].ty != TypeStore::ERROR
        || matches!(
            function.locals[index].kind,
            LocalKind::Capture(_) | LocalKind::Item
        )
    {
        return;
    }
    if state[index] == UseState::Unused && reported.insert((span, index, true)) {
        let local = &function.locals[index];
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!(
                    "error value in `{}` may be overwritten before use",
                    local.name
                ),
                span,
            )
            .related(local.span, "error binding declared here")
            .note("read the value or explicitly discard it with `_ = name`"),
        );
    }
    state[index] = UseState::Unused;
}

fn report_unused(
    function: &hir::Function,
    index: usize,
    state: UseState,
    diagnostics: &mut Vec<Diagnostic>,
    reported: &mut HashSet<(Span, usize, bool)>,
) {
    if state != UseState::Unused {
        return;
    }
    let local = &function.locals[index];
    if reported.insert((local.span, index, false)) {
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!(
                    "error value in `{}` may be unused before scope exit",
                    local.name
                ),
                local.span,
            )
            .note("read the value or explicitly discard it with `_ = name`"),
        );
    }
}
