use crate::hir;
use crate::mir::{
    BasicBlock, BlockId, Body, Callee, Local, Place, Program, Rvalue, Statement, Terminator,
    place_type,
};

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
        .filter(|(_, local)| !local.by_reference && package.needs_drop(local.ty))
        .map(|(index, _)| Local(index as u32))
        .collect();
    for index in 0..body.blocks.len() {
        let old = std::mem::take(&mut body.blocks[index].statements);
        let mut statements = Vec::new();
        for statement in old {
            match statement {
                Statement::Assign {
                    place,
                    rvalue,
                    span,
                } => {
                    let binds_reference =
                        matches!(rvalue, Rvalue::Ref(_) | Rvalue::MapValueRef(..));
                    if !binds_reference
                        && !package.is_copy(place_type(package, &body.locals, &place))
                    {
                        statements.push(Statement::Drop {
                            replacement: !place.projections.is_empty(),
                            place: place.clone(),
                            before_store: true,
                        });
                    }
                    statements.push(Statement::Assign {
                        place,
                        rvalue,
                        span,
                    });
                }
                Statement::EndScope(locals) => {
                    for local in locals.into_iter().rev() {
                        if owned.contains(&local) {
                            statements.push(Statement::Drop {
                                place: Place::local(local),
                                replacement: false,
                                before_store: false,
                            });
                        }
                    }
                }
                drop @ Statement::Drop { .. } => statements.push(drop),
                set @ Statement::SetGlobal { .. } => statements.push(set),
            }
        }
        let old_terminator =
            std::mem::replace(&mut body.blocks[index].terminator, Terminator::Unreachable);
        body.blocks[index].terminator = match old_terminator {
            Terminator::Call {
                callee: Callee::Drop,
                args,
                target,
                ..
            } => {
                if let [crate::mir::Operand::Move(place)] = &args[..] {
                    statements.push(Statement::Drop {
                        place: place.clone(),
                        replacement: false,
                        before_store: false,
                    });
                }
                Terminator::Goto(target)
            }
            Terminator::Call {
                callee,
                args,
                destinations,
                target,
                span,
                ..
            } => Terminator::Call {
                callee,
                args,
                destinations,
                target,
                unwind: Some(unwind),
                span,
            },
            Terminator::Assert {
                place,
                rvalue,
                target,
                span,
                ..
            } => Terminator::Assert {
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
                            before_store: false,
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
                before_store: false,
            })
            .collect(),
        terminator: Terminator::PanicReturn,
    });
    body.unwind = Some(unwind);
}
