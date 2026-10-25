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
    let returns = body
        .blocks
        .iter()
        .filter(|block| matches!(block.terminator, Terminator::Return))
        .count();
    let mut guards = Vec::new();
    for block in &body.blocks {
        for statement in &block.statements {
            if let Statement::EndMapEntry { guard, .. } = statement
                && guard.mutable
                && !guards
                    .iter()
                    .any(|previous: &crate::mir::MapEntryGuard| previous.entry == guard.entry)
            {
                guards.push(guard.clone());
            }
        }
    }
    guards.sort_by_key(|guard| guard.entry.0);
    let owned: Vec<Local> = body
        .locals
        .iter()
        .enumerate()
        .filter(|(_, local)| !local.by_reference && package.needs_drop(local.ty))
        .map(|(index, _)| Local(index as u32))
        .collect();
    let dropped_on_return: Vec<Local> = owned
        .iter()
        .rev()
        .filter(|local| !body.returns.contains(local))
        .copied()
        .collect();
    let original = body.blocks.len();
    let exit = (returns > 1).then(|| reserve(body, Terminator::Return));
    let unwind = reserve(body, Terminator::PanicReturn);
    for index in 0..original {
        let old = std::mem::take(&mut body.blocks[index].statements);
        let mut statements = Vec::new();
        for statement in old {
            match statement {
                Statement::Assign {
                    place,
                    rvalue,
                    span,
                } => {
                    let binds_reference = matches!(
                        rvalue,
                        Rvalue::Ref(_)
                            | Rvalue::RefMut(_)
                            | Rvalue::MapValueRef(..)
                            | Rvalue::MapEntryRef { .. }
                    );
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
                end @ (Statement::EndMapEntry { .. } | Statement::EndTaskScope { .. }) => {
                    statements.push(end)
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
            Terminator::Return => match exit {
                Some(exit) => Terminator::Goto(exit),
                None => {
                    statements.extend(drops(&dropped_on_return));
                    Terminator::Return
                }
            },
            other => other,
        };
        body.blocks[index].statements = statements;
    }
    if let Some(exit) = exit {
        body.blocks[exit.0 as usize].statements = drops(&dropped_on_return).collect();
    }
    let owned_in_reverse: Vec<Local> = owned.into_iter().rev().collect();
    body.blocks[unwind.0 as usize].statements = body
        .task_scopes
        .iter()
        .rev()
        .map(|scope| Statement::EndTaskScope {
            scope: scope.registry,
            unwind: true,
        })
        .chain(
            guards
                .into_iter()
                .rev()
                .map(|guard| Statement::EndMapEntry {
                    guard,
                    unwind: true,
                }),
        )
        .chain(drops(&owned_in_reverse))
        .collect();
    body.unwind = Some(unwind);
}

fn reserve(body: &mut Body, terminator: Terminator) -> BlockId {
    body.blocks.push(BasicBlock {
        statements: Vec::new(),
        terminator,
    });
    BlockId(body.blocks.len() as u32 - 1)
}

fn drops(locals: &[Local]) -> impl Iterator<Item = Statement> + '_ {
    locals.iter().map(|&local| Statement::Drop {
        place: Place::local(local),
        replacement: false,
        before_store: false,
    })
}
