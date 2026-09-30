use crate::hir;
use crate::mir::{BasicBlock, BlockId, Body, Callee, Local, Place, Program, Statement, Terminator};
use crate::types::TypeId;

pub fn insert(package: &hir::Package, program: &mut Program) {
    for body in &mut program.bodies {
        insert_body(package, body);
    }
}

fn insert_body(package: &hir::Package, body: &mut Body) {
    let unwind = BlockId(body.blocks.len() as u32);
    let owned: Vec<Local> = body
        .locals
        .iter()
        .enumerate()
        .filter(|(_, local)| !local.by_reference && !package.is_copy(local.ty))
        .map(|(index, _)| Local(index as u32))
        .collect();
    for index in 0..body.blocks.len() {
        let old = std::mem::take(&mut body.blocks[index].statements);
        let mut statements = Vec::new();
        for statement in old {
            match statement {
                Statement::Assign { place, rvalue, span } => {
                    if !package.is_copy(place_type(package, body, &place)) {
                        statements.push(Statement::Drop {
                            replacement: !place.fields.is_empty(),
                            place: place.clone(),
                        });
                    }
                    statements.push(Statement::Assign { place, rvalue, span });
                }
                Statement::EndScope(locals) => {
                    for local in locals.into_iter().rev() {
                        if owned.contains(&local) {
                            statements.push(Statement::Drop {
                                place: Place::local(local),
                                replacement: false,
                            });
                        }
                    }
                }
                drop @ Statement::Drop { .. } => statements.push(drop),
            }
        }
        let old_terminator = std::mem::replace(&mut body.blocks[index].terminator, Terminator::Unreachable);
        body.blocks[index].terminator = match old_terminator {
            Terminator::Call {
                callee: Callee::Drop,
                args,
                target,
                ..
            } => {
                if let [crate::mir::Operand::Move(place)] = &args {
                    statements.push(Statement::Drop {
                        place: place.clone(),
                        replacement: false,
                    });
                }
                Terminator::Goto(target)
            }
            Terminator::Call { callee, args, destinations, target, span, .. } => {
                Terminator::Call {
                    callee,
                    args,
                    destinations,
                    target,
                    unwind: Some(unwind),
                    span,
                }
            }
            Terminator::Assert { place, rvalue, target, span, .. } => Terminator::Assert {
                place,
                rvalue,
                target,
                unwind: Some(unwind),
                span,
            },
            Terminator::Return => {
                for &local in owned.iter().rev() {
                    if !body.returns.contains(&local) {
                        statements.push(Statement::Drop {
                            place: Place::local(local),
                            replacement: false,
                        });
                    }
                }
                Terminator::Return
            }
            other => other,
        };
        body.blocks[index].statements = statements;
    }
    body.blocks.push(BasicBlock {
        statements: owned
            .into_iter()
            .rev()
            .map(|local| Statement::Drop {
                place: Place::local(local),
                replacement: false,
            })
            .collect(),
        terminator: Terminator::PanicReturn,
    });
    body.unwind = Some(unwind);
}

fn place_type(package: &hir::Package, body: &Body, place: &Place) -> TypeId {
    let mut ty = body.locals[place.local.0 as usize].ty;
    for field in &place.fields {
        let id = package.types.struct_id(ty).expect("field projection");
        ty = package.strukt(id).fields[field.0 as usize].ty;
    }
    ty
}
