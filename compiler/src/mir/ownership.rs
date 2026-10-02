use crate::diagnostic::{Diagnostic, Severity};
use crate::hir;
use crate::mir::{BasicBlock, BlockId, Body, Callee, Operand, Place, Program, Rvalue, Terminator};
use crate::source::Span;

/// The set of field paths moved out of a local and not yet reinitialized.
///
/// An empty path (`vec![]`) represents the whole local having been moved; by
/// construction it never coexists with any other entry (see `join`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct MovedSet {
    entries: Vec<(Vec<hir::FieldId>, Span)>,
}

impl MovedSet {
    /// A moved path that is `fields` itself or a container of it.
    fn moved_or_ancestor_moved(&self, fields: &[hir::FieldId]) -> Option<Span> {
        self.entries
            .iter()
            .find(|(path, _)| {
                path.len() <= fields.len() && path.as_slice() == &fields[..path.len()]
            })
            .map(|&(_, span)| span)
    }

    /// A moved path strictly nested inside `fields`.
    fn moved_descendant(&self, fields: &[hir::FieldId]) -> Option<Span> {
        self.entries
            .iter()
            .find(|(path, _)| path.len() > fields.len() && &path[..fields.len()] == fields)
            .map(|&(_, span)| span)
    }

    /// A moved path that is a proper container of `fields`, excluding `fields` itself.
    fn moved_strict_ancestor(&self, fields: &[hir::FieldId]) -> Option<Span> {
        self.entries
            .iter()
            .find(|(path, _)| path.len() < fields.len() && path.as_slice() == &fields[..path.len()])
            .map(|&(_, span)| span)
    }

    fn record_move(&mut self, fields: Vec<hir::FieldId>, span: Span) {
        self.entries.push((fields, span));
    }

    /// Restores `fields` and everything moved out from beneath it.
    fn reinitialize(&mut self, fields: &[hir::FieldId]) {
        self.entries
            .retain(|(path, _)| !(path.len() >= fields.len() && &path[..fields.len()] == fields));
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    /// Conservative union: a path moved on either side counts as moved after the join.
    fn join(&self, other: &Self) -> Self {
        let mut entries: Vec<(Vec<hir::FieldId>, Span)> =
            self.entries.iter().chain(&other.entries).cloned().collect();
        entries.sort_by_key(|(path, span)| (path.len(), path.clone(), span.start(), span.end()));
        let mut reduced: Vec<(Vec<hir::FieldId>, Span)> = Vec::new();
        for (path, span) in entries {
            let covered = reduced.iter().any(|(kept, _)| {
                kept.len() <= path.len() && kept.as_slice() == &path[..kept.len()]
            });
            if !covered {
                reduced.push((path, span));
            }
        }
        Self { entries: reduced }
    }
}

/// Whether any struct containing `place`'s path (never the designated field's own type)
/// defines a custom `drop`, which forbids moving that field out on its own.
fn has_custom_ancestor(package: &hir::Package, body: &Body, place: &Place) -> bool {
    let mut ty = body.locals[place.local.0 as usize].ty;
    for field in &place.fields {
        let id = package.types.struct_id(ty).expect("field projection");
        if package.strukt(id).drop.is_some() {
            return true;
        }
        ty = package.strukt(id).fields[field.0 as usize].ty;
    }
    false
}

/// Renders `place` as a dotted source-like name for diagnostics.
fn describe_place(package: &hir::Package, body: &Body, place: &Place) -> String {
    let mut ty = body.locals[place.local.0 as usize].ty;
    let mut name = body.locals[place.local.0 as usize]
        .name
        .as_deref()
        .unwrap_or("temporary value")
        .to_string();
    for field in &place.fields {
        let id = package.types.struct_id(ty).expect("field projection");
        let f = &package.strukt(id).fields[field.0 as usize];
        name.push('.');
        name.push_str(&f.name);
        ty = f.ty;
    }
    name
}

pub fn check(package: &hir::Package, program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for body in &program.bodies {
        check_body(package, body, &mut diagnostics);
    }
    diagnostics
}

fn check_body(package: &hir::Package, body: &Body, diagnostics: &mut Vec<Diagnostic>) {
    if body.blocks.is_empty() {
        return;
    }
    let initial = vec![MovedSet::default(); body.locals.len()];
    let mut incoming = vec![None; body.blocks.len()];
    incoming[0] = Some(initial);
    let mut changed = true;
    while changed {
        changed = false;
        for (index, block) in body.blocks.iter().enumerate() {
            let Some(mut state) = incoming[index].clone() else {
                continue;
            };
            transfer(package, body, block, &mut state, &mut Vec::new());
            for successor in successors(&block.terminator) {
                let slot = &mut incoming[successor.0 as usize];
                let joined = match slot {
                    Some(previous) => previous
                        .iter()
                        .zip(&state)
                        .map(|(a, b)| a.join(b))
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
            transfer(package, body, block, &mut state, diagnostics);
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
    package: &hir::Package,
    body: &Body,
    block: &BasicBlock,
    state: &mut [MovedSet],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in &block.statements {
        if let crate::mir::Statement::Assign {
            place,
            rvalue,
            span,
        } = statement
        {
            check_rvalue(package, body, rvalue, *span, state, diagnostics);
            assign(package, body, place, *span, state, diagnostics);
        }
    }
    match &block.terminator {
        Terminator::Assert {
            place,
            rvalue,
            span,
            ..
        } => {
            check_rvalue(package, body, rvalue, *span, state, diagnostics);
            assign(package, body, place, *span, state, diagnostics);
        }
        Terminator::Branch {
            condition, span, ..
        } => {
            check_operand(package, body, condition, *span, state, diagnostics);
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
                        let name = describe_place(package, body, moved);
                        diagnostics.push(Diagnostic::new(
                            Severity::Error,
                            format!("cannot move `{name}` while it is borrowed by this call"),
                            *span,
                        ));
                    }
                }
            }
            for arg in args {
                check_operand(package, body, arg, *span, state, diagnostics);
            }
            if !matches!(callee, Callee::Drop) {
                for place in destinations.iter().flatten() {
                    assign(package, body, place, *span, state, diagnostics);
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
    package: &hir::Package,
    body: &Body,
    rvalue: &Rvalue,
    span: Span,
    state: &mut [MovedSet],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match rvalue {
        Rvalue::Zero => {}
        Rvalue::Use(operand)
        | Rvalue::Unary(_, operand)
        | Rvalue::Convert(operand, _)
        | Rvalue::Error(operand) => {
            check_operand(package, body, operand, span, state, diagnostics);
        }
        Rvalue::Binary(_, left, right) => {
            check_operand(package, body, left, span, state, diagnostics);
            check_operand(package, body, right, span, state, diagnostics);
        }
        Rvalue::Aggregate(_, fields) => {
            for field in fields {
                check_operand(package, body, field, span, state, diagnostics);
            }
        }
    }
}

fn places_overlap(left: &Place, right: &Place) -> bool {
    left.local == right.local && left.fields.iter().zip(&right.fields).all(|(a, b)| a == b)
}

fn check_operand(
    package: &hir::Package,
    body: &Body,
    operand: &Operand,
    span: Span,
    state: &mut [MovedSet],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (place, moving) = match operand {
        Operand::Copy(place) | Operand::Ref(place) => (place, false),
        Operand::Move(place) => (place, true),
        Operand::Const(..) => return,
    };
    let index = place.local.0 as usize;
    if let Some(origin) = state[index].moved_or_ancestor_moved(&place.fields) {
        let name = describe_place(package, body, place);
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
    if let Some(origin) = state[index].moved_descendant(&place.fields) {
        let name = describe_place(package, body, place);
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("cannot use `{name}` as a whole value while a field is moved out"),
                span,
            )
            .related(origin, "field moved here"),
        );
        return;
    }
    if !moving {
        return;
    }
    if body.locals[index].by_reference {
        let name = describe_place(package, body, place);
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
    if !place.fields.is_empty() && has_custom_ancestor(package, body, place) {
        let name = describe_place(package, body, place);
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("cannot move `{name}` out of a value with a custom `drop` method"),
                span,
            )
            .note("a destructor always requires a complete, unmoved receiver (§31.2)"),
        );
        return;
    }
    state[index].record_move(place.fields.clone(), span);
}

fn assign(
    package: &hir::Package,
    body: &Body,
    place: &Place,
    span: Span,
    state: &mut [MovedSet],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let index = place.local.0 as usize;
    if place.fields.is_empty() {
        state[index].clear();
        return;
    }
    if let Some(origin) = state[index].moved_strict_ancestor(&place.fields) {
        let name = describe_place(package, body, place);
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("cannot assign through moved value `{name}`"),
                span,
            )
            .related(origin, "value moved here"),
        );
        return;
    }
    state[index].reinitialize(&place.fields);
}

#[cfg(test)]
mod tests {
    use super::MovedSet;
    use crate::hir::FieldId;
    use crate::source::{SourceMap, Span};

    fn two_spans() -> (Span, Span) {
        let mut sources = SourceMap::new();
        let file = sources.add("test.ore", "0123456789".to_string()).unwrap();
        (
            sources.span(file, 0, 1).unwrap(),
            sources.span(file, 2, 3).unwrap(),
        )
    }

    #[test]
    fn whole_local_move_absorbs_any_other_entry_on_join() {
        let (whole_span, field_span) = two_spans();
        let mut whole = MovedSet::default();
        whole.record_move(Vec::new(), whole_span);

        let mut field = MovedSet::default();
        field.record_move(vec![FieldId(0)], field_span);

        let joined = whole.join(&field);
        assert_eq!(joined.entries, vec![(Vec::new(), whole_span)]);

        let joined = field.join(&whole);
        assert_eq!(joined.entries, vec![(Vec::new(), whole_span)]);
    }
}
