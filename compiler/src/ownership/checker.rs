use super::move_state::MovedSet;
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir;
use crate::mir::{
    BasicBlock, Body, Callee, Operand, Place, Program, Projection, Rvalue, Terminator,
    captured_operand, projection_type,
};
use crate::resolve::{FieldId, LocalKind};
use crate::source::Span;
use crate::types::{TypeId, TypeKind};

/// Array contents aren't partial-move-tracked past the array field.
fn leading_field_path(projections: &[Projection]) -> Vec<FieldId> {
    projections
        .iter()
        .take_while(|p| matches!(p, Projection::Field(_)))
        .map(|p| match p {
            Projection::Field(id) => *id,
            Projection::Index(_) => unreachable!("take_while already excluded this"),
        })
        .collect()
}

/// Only called with a field-only path.
fn has_custom_ancestor(package: &hir::Package, local_ty: TypeId, fields: &[FieldId]) -> bool {
    let mut ty = local_ty;
    for field in fields {
        let id = package.types.struct_id(ty).expect("field projection");
        if package.strukt(id).drop.is_some() {
            return true;
        }
        ty = package.strukt(id).fields[field.0 as usize].ty;
    }
    false
}

pub(super) fn describe_place(package: &hir::Package, body: &Body, place: &Place) -> String {
    let mut ty = body.locals[place.local.0 as usize].ty;
    let mut name = body.locals[place.local.0 as usize]
        .name
        .as_deref()
        .unwrap_or("temporary value")
        .to_string();
    for projection in &place.projections {
        match projection {
            Projection::Field(field) => {
                let id = package.types.struct_id(ty).expect("field projection");
                let f = &package.strukt(id).fields[field.0 as usize];
                name.push('.');
                name.push_str(&f.name);
                ty = f.ty;
            }
            Projection::Index(_) => {
                name.push_str("[_]");
                ty = projection_type(package, ty, projection);
            }
        }
    }
    name
}

pub fn check(package: &hir::Package, program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for body in &program.bodies {
        check_body(package, body, &mut diagnostics);
    }
    super::region::check(package, program, &mut diagnostics);
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
            for successor in block.terminator.successors() {
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
    let mut found = Vec::new();
    for (block, state) in body.blocks.iter().zip(incoming) {
        if let Some(mut state) = state {
            transfer(package, body, block, &mut state, &mut found);
        }
    }
    // A consuming call both uses and moves its callee, which can report one mistake twice.
    found.dedup_by(|later, earlier| {
        later.message() == earlier.message() && later.span() == earlier.span()
    });
    diagnostics.extend(found);
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
            if let Callee::Value(place) = callee {
                let callee = Operand::Copy(place.clone());
                check_operand(package, body, &callee, *span, state, diagnostics);
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
        Rvalue::Zero | Rvalue::Global(_) => {}
        Rvalue::Use(operand)
        | Rvalue::Spawn(operand)
        | Rvalue::Unary(_, operand)
        | Rvalue::Convert(operand, _)
        | Rvalue::Error(operand) => {
            check_operand(package, body, operand, span, state, diagnostics);
        }
        Rvalue::Binary(_, left, right) | Rvalue::BoundsCheck(left, right) => {
            check_operand(package, body, left, span, state, diagnostics);
            check_operand(package, body, right, span, state, diagnostics);
        }
        Rvalue::Aggregate(_, fields) => {
            for field in fields {
                check_operand(package, body, field, span, state, diagnostics);
            }
        }
        Rvalue::Length(place) | Rvalue::Ref(place) => {
            let base = Operand::Ref(place.clone());
            check_operand(package, body, &base, span, state, diagnostics);
        }
        Rvalue::MapKeyAt(place, position) | Rvalue::MapValueRef(place, position) => {
            let base = Operand::Ref(place.clone());
            check_operand(package, body, &base, span, state, diagnostics);
            check_operand(package, body, position, span, state, diagnostics);
        }
        Rvalue::StringSlice { source, low, high } => {
            check_operand(package, body, source, span, state, diagnostics);
            for bound in [low, high].into_iter().flatten() {
                check_operand(package, body, bound, span, state, diagnostics);
            }
        }
        Rvalue::StringChar(string, position) | Rvalue::StringAdvance(string, position) => {
            check_operand(package, body, string, span, state, diagnostics);
            check_operand(package, body, position, span, state, diagnostics);
        }
        Rvalue::Slice {
            place, low, high, ..
        } => {
            let base = Operand::Copy(place.clone());
            check_operand(package, body, &base, span, state, diagnostics);
            for bound in [low, high].into_iter().flatten() {
                check_operand(package, body, bound, span, state, diagnostics);
            }
        }
        Rvalue::Closure {
            captures, owning, ..
        } => {
            for (place, exclusive) in captures {
                let captured = if *owning {
                    captured_operand(package, &body.locals, place, *exclusive)
                } else {
                    Operand::Copy(place.clone())
                };
                check_operand(package, body, &captured, span, state, diagnostics);
            }
        }
    }
}

fn indexes_into(
    package: &hir::Package,
    body: &Body,
    place: &Place,
    indexed: fn(TypeKind) -> bool,
) -> bool {
    let mut ty = body.locals[place.local.0 as usize].ty;
    for projection in &place.projections {
        if matches!(projection, Projection::Index(_)) && indexed(package.types.kind(ty)) {
            return true;
        }
        ty = projection_type(package, ty, projection);
    }
    false
}

/// Two indices never prove disjointness.
fn projections_conservatively_equal(a: &Projection, b: &Projection) -> bool {
    match (a, b) {
        (Projection::Field(x), Projection::Field(y)) => x == y,
        (Projection::Index(_), Projection::Index(_)) => true,
        _ => false,
    }
}

fn places_overlap(left: &Place, right: &Place) -> bool {
    left.local == right.local
        && left
            .projections
            .iter()
            .zip(&right.projections)
            .all(|(a, b)| projections_conservatively_equal(a, b))
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
    let field_path = leading_field_path(&place.projections);
    if let Some(origin) = state[index].moved_or_ancestor_moved(&field_path) {
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
    if let Some(origin) = state[index].moved_descendant(&field_path) {
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
    let function = package.function(body.function);
    let is_capture = function
        .locals
        .get(index)
        .is_some_and(|local| matches!(local.kind, LocalKind::Capture(_)));
    let consumed_capture = is_capture && function.call_once;
    if is_capture && !consumed_capture {
        let name = describe_place(package, body, place);
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("moving captured value `{name}` out of a function literal is not supported yet"),
                span,
            )
            .note("a closure borrows what it captures; consuming a capture would make it callable only once"),
        );
        return;
    }
    if body.locals[index].by_reference && !consumed_capture {
        let name = describe_place(package, body, place);
        let is_item = package
            .function(body.function)
            .locals
            .get(index)
            .is_some_and(|local| local.kind == LocalKind::Item);
        let note = if is_item {
            "a loop item borrows the current element; it does not own it"
        } else {
            "a borrowed parameter does not own its argument"
        };
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("cannot move borrowed value `{name}`"),
                span,
            )
            .note(note),
        );
        return;
    }
    if indexes_into(package, body, place, |kind| {
        matches!(kind, TypeKind::Slice { .. })
    }) {
        let name = describe_place(package, body, place);
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("cannot move `{name}` out of a slice"),
                span,
            )
            .note("a slice borrows its elements; moving them out is not allowed"),
        );
        return;
    }
    if indexes_into(package, body, place, |kind| {
        matches!(kind, TypeKind::DynArray { .. })
    }) {
        let name = describe_place(package, body, place);
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("cannot move `{name}` out of a dynamic array"),
                span,
            )
            .note("`Array<T>` keeps ownership of its elements; borrow the element instead"),
        );
        return;
    }
    if place
        .projections
        .iter()
        .any(|p| matches!(p, Projection::Index(_)))
    {
        let name = describe_place(package, body, place);
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("moving `{name}` out through an array index is not supported yet"),
                span,
            )
            .note("fixed-array element extraction is planned for a later milestone"),
        );
        return;
    }
    if !field_path.is_empty() && has_custom_ancestor(package, body.locals[index].ty, &field_path) {
        let name = describe_place(package, body, place);
        diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("cannot move `{name}` out of a value with a custom `drop` method"),
                span,
            )
            .note("a destructor always requires a complete, unmoved receiver"),
        );
        return;
    }
    state[index].record_move(field_path, span);
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
    if place.projections.is_empty() {
        state[index].clear();
        return;
    }
    let field_path = leading_field_path(&place.projections);
    // Through an index, an exact match is a moved ancestor, not a reinitialization.
    let truncated = place.projections.len() > field_path.len();
    let ancestor = if truncated {
        state[index].moved_or_ancestor_moved(&field_path)
    } else {
        state[index].moved_strict_ancestor(&field_path)
    };
    if let Some(origin) = ancestor {
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
    if !truncated {
        state[index].reinitialize(&field_path);
    }
}

#[cfg(test)]
mod tests {
    use super::super::move_state::MovedSet;
    use super::leading_field_path;
    use crate::mir::{Operand, Place, Projection};
    use crate::resolve::FieldId;
    use crate::source::SourceMap;

    #[test]
    fn leading_field_path_stops_at_the_first_index() {
        let zero = Operand::Const(crate::hir::Const::Int(0), crate::types::TypeStore::INT);
        assert_eq!(leading_field_path(&[]), Vec::new());
        assert_eq!(
            leading_field_path(&[Projection::Field(FieldId(0)), Projection::Field(FieldId(1))]),
            vec![FieldId(0), FieldId(1)]
        );
        assert_eq!(
            leading_field_path(&[
                Projection::Field(FieldId(0)),
                Projection::Index(zero.clone()),
                Projection::Field(FieldId(1)),
            ]),
            vec![FieldId(0)]
        );
        assert_eq!(leading_field_path(&[Projection::Index(zero)]), Vec::new());
    }

    #[test]
    fn whole_array_field_move_blocks_assignment_through_an_index() {
        // `let other = outer.arr` then `outer.arr[0] = v` must be rejected.
        let mut sources = SourceMap::new();
        let file = sources.add("test.ore", "0123456789".to_string()).unwrap();
        let span = sources.span(file, 0, 1).unwrap();

        let mut state = MovedSet::default();
        state.record_move(vec![FieldId(0)], span); // outer.arr moved whole

        let index_path = [Projection::Field(FieldId(0)), Projection::Index(zero())];
        let field_path = leading_field_path(&index_path);
        assert_eq!(field_path, vec![FieldId(0)]);
        let truncated = index_path.len() > field_path.len();
        assert!(truncated);
        assert!(state.moved_or_ancestor_moved(&field_path).is_some());
        assert!(state.moved_strict_ancestor(&field_path).is_none());
    }

    fn zero() -> Operand {
        Operand::Const(crate::hir::Const::Int(0), crate::types::TypeStore::INT)
    }

    #[test]
    fn places_overlap_treats_any_two_indices_as_overlapping() {
        let local = crate::mir::Local(0);
        let a = Place {
            local,
            projections: vec![Projection::Index(zero())],
        };
        let b = Place {
            local,
            projections: vec![Projection::Index(Operand::Const(
                crate::hir::Const::Int(1),
                crate::types::TypeStore::INT,
            ))],
        };
        assert!(super::places_overlap(&a, &b));
    }
}
