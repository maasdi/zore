//! Lowering from typed HIR to MIR, making evaluation order explicit.

use crate::ast::BinaryOp;
use crate::hir::{self, Const, ExprKind, FunctionId, StmtKind};
use crate::mir::{
    BasicBlock, BlockId, Body, Callee, Local, LocalDecl, Operand, Place, Program, Rvalue,
    Statement, Terminator,
};
use crate::source::Span;
use crate::types::{TypeId, TypeStore};

pub fn lower(package: &hir::Package) -> Program {
    let bodies = package
        .functions
        .iter()
        .enumerate()
        .map(|(index, function)| lower_function(package, FunctionId(index as u32), function))
        .collect();
    Program {
        bodies,
        entry: package.entry,
    }
}

struct PendingBlock {
    statements: Vec<Statement>,
    terminator: Option<Terminator>,
}

#[derive(Clone, Copy)]
struct LoopTargets {
    continue_to: BlockId,
    break_to: BlockId,
}

struct Builder {
    locals: Vec<LocalDecl>,
    blocks: Vec<PendingBlock>,
    current: BlockId,
    returns: Vec<Local>,
    loops: Vec<LoopTargets>,
}

fn lower_function(package: &hir::Package, id: FunctionId, function: &hir::Function) -> Body {
    // HIR locals keep their indexes; temporaries follow.
    let mut builder = Builder {
        locals: function
            .locals
            .iter()
            .map(|local| LocalDecl {
                ty: local.ty,
                name: Some(local.name.clone()),
            })
            .collect(),
        blocks: Vec::new(),
        current: BlockId(0),
        returns: Vec::new(),
        loops: Vec::new(),
    };
    builder.returns = function
        .results
        .iter()
        .map(|&ty| builder.temp(ty))
        .collect();
    builder.current = builder.new_block();
    builder.block(package, &function.body);
    let fallthrough = if function.results.is_empty() {
        Terminator::Return
    } else {
        // The checker rejects bodies that fall through without returning.
        Terminator::Unreachable
    };
    builder.terminate(fallthrough);
    Body {
        function: id,
        name: function.name.clone(),
        params: function.params.iter().map(|p| Local(p.0)).collect(),
        returns: builder.returns,
        locals: builder.locals,
        blocks: builder
            .blocks
            .into_iter()
            .map(|block| BasicBlock {
                statements: block.statements,
                terminator: block.terminator.unwrap_or(Terminator::Unreachable),
            })
            .collect(),
    }
}

impl Builder {
    fn temp(&mut self, ty: TypeId) -> Local {
        self.locals.push(LocalDecl { ty, name: None });
        Local(self.locals.len() as u32 - 1)
    }

    fn new_block(&mut self) -> BlockId {
        self.blocks.push(PendingBlock {
            statements: Vec::new(),
            terminator: None,
        });
        BlockId(self.blocks.len() as u32 - 1)
    }

    fn push(&mut self, place: Place, rvalue: Rvalue, span: Span) {
        let block = &mut self.blocks[self.current.0 as usize];
        if block.terminator.is_none() {
            block.statements.push(Statement {
                place,
                rvalue,
                span,
            });
        }
    }

    fn terminate(&mut self, terminator: Terminator) {
        let block = &mut self.blocks[self.current.0 as usize];
        if block.terminator.is_none() {
            block.terminator = Some(terminator);
        }
    }

    fn diverge(&mut self, terminator: Terminator) {
        self.terminate(terminator);
        self.current = self.new_block();
    }

    fn goto_new(&mut self, target: BlockId) {
        self.terminate(Terminator::Goto(target));
        self.current = target;
    }

    fn assign_temp(&mut self, ty: TypeId, rvalue: Rvalue, span: Span) -> Operand {
        let temp = self.temp(ty);
        self.push(Place::local(temp), rvalue, span);
        Operand::Copy(Place::local(temp))
    }

    fn block(&mut self, package: &hir::Package, block: &hir::Block) {
        for stmt in &block.stmts {
            self.stmt(package, stmt);
        }
    }

    fn stmt(&mut self, package: &hir::Package, stmt: &hir::Stmt) {
        match &stmt.kind {
            StmtKind::Let { targets, value } => {
                let places: Vec<Option<Place>> = targets
                    .iter()
                    .map(|t| t.map(|id| Place::local(Local(id.0))))
                    .collect();
                self.store_results(package, &places, value, stmt.span);
            }
            StmtKind::Assign { targets, values } => {
                let places: Vec<Option<Place>> =
                    targets.iter().map(|t| t.as_ref().map(place)).collect();
                if let [value] = &values[..]
                    && places.len() > 1
                {
                    self.store_results(package, &places, value, stmt.span);
                    return;
                }
                // Every value is evaluated before the first store.
                let operands: Vec<(Operand, Span)> = values
                    .iter()
                    .map(|v| (self.evaluate_to_temporary(package, v), v.span))
                    .collect();
                for (target, (operand, span)) in places.into_iter().zip(operands) {
                    if let Some(target) = target {
                        self.push(target, Rvalue::Use(operand), span);
                    }
                }
            }
            StmtKind::CompoundAssign {
                place: target,
                op,
                value,
            } => {
                let target = place(target);
                let ty = self.place_type(package, &target);
                // The target is read before the right-hand side is evaluated.
                let current =
                    self.assign_temp(ty, Rvalue::Use(Operand::Copy(target.clone())), stmt.span);
                let rhs = self.operand(package, value);
                self.push(target, Rvalue::Binary(*op, current, rhs), stmt.span);
            }
            StmtKind::Expr(expr) => match &expr.kind {
                ExprKind::Call { .. } | ExprKind::Println(_) => {
                    let discard = vec![None; expr.types.len()];
                    self.call(package, expr, discard);
                }
                _ => {
                    self.operand(package, expr);
                }
            },
            StmtKind::Return(values) => {
                let returns: Vec<Option<Place>> = self
                    .returns
                    .iter()
                    .map(|&l| Some(Place::local(l)))
                    .collect();
                if let [value] = &values[..]
                    && returns.len() > 1
                {
                    self.store_results(package, &returns, value, stmt.span);
                } else {
                    let operands: Vec<Operand> =
                        values.iter().map(|v| self.operand(package, v)).collect();
                    for (target, operand) in returns.into_iter().zip(operands) {
                        self.push(
                            target.expect("result place"),
                            Rvalue::Use(operand),
                            stmt.span,
                        );
                    }
                }
                self.diverge(Terminator::Return);
            }
            StmtKind::Break => {
                let targets = *self.loops.last().expect("checked: inside a loop");
                self.diverge(Terminator::Goto(targets.break_to));
            }
            StmtKind::Continue => {
                let targets = *self.loops.last().expect("checked: inside a loop");
                self.diverge(Terminator::Goto(targets.continue_to));
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let condition = self.operand(package, condition);
                let then_id = self.new_block();
                let join = self.new_block();
                let else_id = if else_block.is_some() {
                    self.new_block()
                } else {
                    join
                };
                self.terminate(Terminator::Branch {
                    condition,
                    then_block: then_id,
                    else_block: else_id,
                });
                self.current = then_id;
                self.block(package, then_block);
                self.terminate(Terminator::Goto(join));
                if let Some(else_block) = else_block {
                    self.current = else_id;
                    self.block(package, else_block);
                    self.terminate(Terminator::Goto(join));
                }
                self.current = join;
            }
            StmtKind::Loop {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt(package, init);
                }
                let header = self.new_block();
                let body_id = self.new_block();
                let exit = self.new_block();
                let next = if update.is_some() {
                    self.new_block()
                } else {
                    header
                };
                self.goto_new(header);
                match condition {
                    Some(condition) => {
                        let condition = self.operand(package, condition);
                        self.terminate(Terminator::Branch {
                            condition,
                            then_block: body_id,
                            else_block: exit,
                        });
                    }
                    None => self.terminate(Terminator::Goto(body_id)),
                }
                self.current = body_id;
                self.loops.push(LoopTargets {
                    continue_to: next,
                    break_to: exit,
                });
                self.block(package, body);
                self.loops.pop();
                self.terminate(Terminator::Goto(next));
                if let Some(update) = update {
                    self.current = next;
                    self.stmt(package, update);
                    self.terminate(Terminator::Goto(header));
                }
                self.current = exit;
            }
            StmtKind::Block(block) => self.block(package, block),
        }
    }

    fn store_results(
        &mut self,
        package: &hir::Package,
        places: &[Option<Place>],
        value: &hir::Expr,
        span: Span,
    ) {
        if places.len() > 1 {
            self.call(package, value, places.to_vec());
            return;
        }
        let operand = self.operand(package, value);
        if let Some(Some(target)) = places.first() {
            self.push(target.clone(), Rvalue::Use(operand), span);
        }
    }

    fn place_type(&self, package: &hir::Package, place: &Place) -> TypeId {
        let mut ty = self.locals[place.local.0 as usize].ty;
        for field in &place.fields {
            let strukt = package
                .types
                .struct_id(ty)
                .expect("field projection on a struct");
            ty = package.strukt(strukt).fields[field.0 as usize].ty;
        }
        ty
    }

    fn evaluate_to_temporary(&mut self, package: &hir::Package, expr: &hir::Expr) -> Operand {
        let operand = self.operand(package, expr);
        match operand {
            Operand::Const(..) => operand,
            _ => self.assign_temp(expr.ty(), Rvalue::Use(operand), expr.span),
        }
    }

    fn call(&mut self, package: &hir::Package, expr: &hir::Expr, destinations: Vec<Option<Place>>) {
        let (callee, args) = match &expr.kind {
            ExprKind::Call { function, args } => (Callee::Function(*function), &args[..]),
            ExprKind::Println(arg) => (Callee::Println, std::slice::from_ref(&**arg)),
            _ => unreachable!("only calls produce multiple or no results"),
        };
        let args = args.iter().map(|a| self.evaluate_to_temporary(package, a)).collect();
        let target = self.new_block();
        self.terminate(Terminator::Call {
            callee,
            args,
            destinations,
            target,
            span: expr.span,
        });
        self.current = target;
    }

    fn operand(&mut self, package: &hir::Package, expr: &hir::Expr) -> Operand {
        let span = expr.span;
        match &expr.kind {
            ExprKind::Const(c) => Operand::Const(c.clone(), expr.ty()),
            ExprKind::Local(id) => Operand::Copy(Place::local(Local(id.0))),
            ExprKind::Field { base, field } => match self.operand(package, base) {
                Operand::Copy(mut base) => {
                    base.fields.push(*field);
                    Operand::Copy(base)
                }
                other => {
                    let temp = self.temp(base.ty());
                    self.push(Place::local(temp), Rvalue::Use(other), span);
                    Operand::Copy(Place {
                        local: temp,
                        fields: vec![*field],
                    })
                }
            },
            ExprKind::Call { .. } => {
                let temp = self.temp(expr.ty());
                self.call(package, expr, vec![Some(Place::local(temp))]);
                Operand::Copy(Place::local(temp))
            }
            ExprKind::Println(_) => unreachable!("println has no value"),
            ExprKind::Convert(inner) => {
                let inner = self.operand(package, inner);
                self.assign_temp(expr.ty(), Rvalue::Convert(inner, expr.ty()), span)
            }
            ExprKind::StructLit { strukt, fields } => {
                // Evaluate in written order, then assemble in declaration order.
                let mut values: Vec<(usize, Operand)> = fields
                    .iter()
                    .map(|(field, value)| (field.0 as usize, self.evaluate_to_temporary(package, value)))
                    .collect();
                values.sort_by_key(|(index, _)| *index);
                let operands = values.into_iter().map(|(_, operand)| operand).collect();
                self.assign_temp(expr.ty(), Rvalue::Aggregate(*strukt, operands), span)
            }
            ExprKind::Unary { op, operand } => {
                let operand = self.operand(package, operand);
                self.assign_temp(expr.ty(), Rvalue::Unary(*op, operand), span)
            }
            ExprKind::Binary { op, lhs, rhs } if matches!(op, BinaryOp::And | BinaryOp::Or) => {
                self.short_circuit(package, *op, lhs, rhs, span)
            }
            ExprKind::Binary { op, lhs, rhs } => {
                // The left operand is fixed before the right is evaluated.
                let lhs = self.evaluate_to_temporary(package, lhs);
                let rhs = self.operand(package, rhs);
                self.assign_temp(expr.ty(), Rvalue::Binary(*op, lhs, rhs), span)
            }
        }
    }

    fn short_circuit(
        &mut self,
        package: &hir::Package,
        op: BinaryOp,
        lhs: &hir::Expr,
        rhs: &hir::Expr,
        span: Span,
    ) -> Operand {
        let result = self.temp(TypeStore::BOOL);
        let condition = self.operand(package, lhs);
        let evaluate_rhs = self.new_block();
        let skip = self.new_block();
        let join = self.new_block();
        let (then_block, else_block) = if op == BinaryOp::And {
            (evaluate_rhs, skip)
        } else {
            (skip, evaluate_rhs)
        };
        self.terminate(Terminator::Branch {
            condition,
            then_block,
            else_block,
        });
        self.current = skip;
        let short = Const::Bool(op == BinaryOp::Or);
        self.push(
            Place::local(result),
            Rvalue::Use(Operand::Const(short, TypeStore::BOOL)),
            span,
        );
        self.terminate(Terminator::Goto(join));
        self.current = evaluate_rhs;
        let value = self.operand(package, rhs);
        self.push(Place::local(result), Rvalue::Use(value), span);
        self.goto_new(join);
        Operand::Copy(Place::local(result))
    }
}

fn place(place: &hir::Place) -> Place {
    Place {
        local: Local(place.root.0),
        fields: place.fields.clone(),
    }
}
