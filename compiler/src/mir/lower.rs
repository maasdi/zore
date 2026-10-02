//! Lowering from typed HIR to MIR, making evaluation order explicit.

use crate::ast::{BinaryOp, ParamMode};
use crate::hir::{self, Const, ExprKind, FunctionId, LocalKind, StmtKind};
use crate::mir::{
    AggregateKind, BasicBlock, BlockId, Body, Callee, Local, LocalDecl, Operand, Place, Program,
    Projection, Rvalue, Statement, Terminator,
};
use crate::source::Span;
use crate::types::{TypeId, TypeKind, TypeStore};

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
    scope_depth: usize,
}

struct Builder {
    locals: Vec<LocalDecl>,
    blocks: Vec<PendingBlock>,
    current: BlockId,
    returns: Vec<Local>,
    loops: Vec<LoopTargets>,
    scopes: Vec<Vec<Local>>,
    temp_scopes: Vec<Vec<Local>>,
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
                by_reference: matches!(local.kind, LocalKind::Param(ParamMode::Mut))
                    || matches!(local.kind, LocalKind::Param(ParamMode::Borrow))
                        && !package.is_copy(local.ty),
            })
            .collect(),
        blocks: Vec::new(),
        current: BlockId(0),
        returns: Vec::new(),
        loops: Vec::new(),
        scopes: Vec::new(),
        temp_scopes: Vec::new(),
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
        unwind: None,
    }
}

impl Builder {
    fn temp(&mut self, ty: TypeId) -> Local {
        self.locals.push(LocalDecl {
            ty,
            name: None,
            by_reference: false,
        });
        let local = Local(self.locals.len() as u32 - 1);
        if let Some(temps) = self.temp_scopes.last_mut() {
            temps.push(local);
        }
        local
    }

    fn new_block(&mut self) -> BlockId {
        self.blocks.push(PendingBlock {
            statements: Vec::new(),
            terminator: None,
        });
        BlockId(self.blocks.len() as u32 - 1)
    }

    fn push(&mut self, place: Place, rvalue: Rvalue, span: Span) {
        if self.blocks[self.current.0 as usize].terminator.is_some() {
            return;
        }
        if matches!(
            rvalue,
            Rvalue::Binary(..) | Rvalue::Unary(..) | Rvalue::Convert(..)
        ) {
            let target = self.new_block();
            self.terminate(Terminator::Assert {
                place,
                rvalue,
                target,
                unwind: None,
                span,
            });
            self.current = target;
        } else {
            self.blocks[self.current.0 as usize]
                .statements
                .push(Statement::Assign {
                    place,
                    rvalue,
                    span,
                });
        }
    }

    fn end_scope(&mut self, locals: Vec<Local>) {
        if !locals.is_empty() && self.blocks[self.current.0 as usize].terminator.is_none() {
            self.blocks[self.current.0 as usize]
                .statements
                .push(Statement::EndScope(locals));
        }
    }

    fn end_exited_scopes(&mut self, depth: usize) {
        let scopes: Vec<Vec<Local>> = self.scopes[depth..].iter().rev().cloned().collect();
        for scope in scopes {
            self.end_scope(scope);
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

    fn assign_temp(
        &mut self,
        package: &hir::Package,
        ty: TypeId,
        rvalue: Rvalue,
        span: Span,
    ) -> Operand {
        let temp = self.temp(ty);
        self.push(Place::local(temp), rvalue, span);
        value_operand(package, Place::local(temp), ty)
    }

    fn block(&mut self, package: &hir::Package, block: &hir::Block) {
        self.scopes.push(Vec::new());
        for stmt in &block.stmts {
            self.stmt(package, stmt);
        }
        let locals = self.scopes.pop().expect("block scope");
        self.end_scope(locals);
    }

    fn stmt(&mut self, package: &hir::Package, stmt: &hir::Stmt) {
        let has_temp_scope = matches!(
            &stmt.kind,
            StmtKind::Let { .. }
                | StmtKind::Assign { .. }
                | StmtKind::CompoundAssign { .. }
                | StmtKind::Expr(_)
        );
        if has_temp_scope {
            self.temp_scopes.push(Vec::new());
        }
        match &stmt.kind {
            StmtKind::Let { targets, value } => {
                for target in targets.iter().flatten() {
                    self.scopes
                        .last_mut()
                        .expect("binding scope")
                        .push(Local(target.0));
                }
                let places: Vec<Option<Place>> = targets
                    .iter()
                    .map(|t| t.map(|id| Place::local(Local(id.0))))
                    .collect();
                self.store_results(package, &places, value, stmt.span);
            }
            StmtKind::Assign { targets, values } => {
                let places: Vec<Option<Place>> = targets
                    .iter()
                    .map(|t| t.as_ref().map(|p| self.place(package, p)))
                    .collect();
                if let [value] = &values[..]
                    && places.len() > 1
                {
                    self.store_results(package, &places, value, stmt.span);
                    let temps = self.temp_scopes.pop().expect("statement temps");
                    self.end_scope(temps);
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
                let target = self.place(package, target);
                let ty = self.place_type(package, &target);
                // The target is read before the right-hand side is evaluated.
                let current = self.assign_temp(
                    package,
                    ty,
                    Rvalue::Use(Operand::Copy(target.clone())),
                    stmt.span,
                );
                let rhs = self.operand(package, value);
                self.push(target, Rvalue::Binary(*op, current, rhs), stmt.span);
            }
            StmtKind::Expr(expr) => match &expr.kind {
                ExprKind::Call { .. } | ExprKind::Println(_) | ExprKind::Drop(_) => {
                    let discard = vec![None; expr.types.len()];
                    self.call(package, expr, discard);
                }
                ExprKind::Try(_) => {
                    self.try_values(package, expr);
                }
                _ => {
                    let operand = self.operand(package, expr);
                    if let Operand::Move(_) = operand {
                        self.assign_temp(package, expr.ty(), Rvalue::Use(operand), expr.span);
                    }
                }
            },
            StmtKind::Return(values) => {
                let returns: Vec<Option<Place>> = self
                    .returns
                    .iter()
                    .map(|&l| Some(Place::local(l)))
                    .collect();
                if let [value] = &values[..]
                    && matches!(value.kind, ExprKind::Try(_))
                    && value.types.len() + 1 == returns.len()
                {
                    self.store_results(package, &returns[..returns.len() - 1], value, stmt.span);
                    self.push(
                        returns
                            .last()
                            .cloned()
                            .flatten()
                            .expect("error return place"),
                        Rvalue::Use(Operand::Const(Const::Nil, TypeStore::ERROR)),
                        stmt.span,
                    );
                } else if let [value] = &values[..]
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
                self.end_exited_scopes(targets.scope_depth);
                self.diverge(Terminator::Goto(targets.break_to));
            }
            StmtKind::Continue => {
                let targets = *self.loops.last().expect("checked: inside a loop");
                self.end_exited_scopes(targets.scope_depth);
                self.diverge(Terminator::Goto(targets.continue_to));
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.temp_scopes.push(Vec::new());
                let condition = self.operand(package, condition);
                let condition_temps = self.temp_scopes.pop().expect("condition temps");
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
                    span: stmt.span,
                });
                self.current = then_id;
                self.end_scope(condition_temps.clone());
                self.block(package, then_block);
                self.terminate(Terminator::Goto(join));
                if let Some(else_block) = else_block {
                    self.current = else_id;
                    self.end_scope(condition_temps);
                    self.block(package, else_block);
                    self.terminate(Terminator::Goto(join));
                } else {
                    self.current = join;
                    self.end_scope(condition_temps);
                }
                self.current = join;
            }
            StmtKind::Loop {
                init,
                condition,
                update,
                body,
            } => {
                self.scopes.push(Vec::new());
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
                        self.temp_scopes.push(Vec::new());
                        let condition = self.operand(package, condition);
                        let condition_temps = self.temp_scopes.pop().expect("condition temps");
                        self.terminate(Terminator::Branch {
                            condition,
                            then_block: body_id,
                            else_block: exit,
                            span: stmt.span,
                        });
                        self.current = body_id;
                        self.end_scope(condition_temps.clone());
                        self.current = exit;
                        self.end_scope(condition_temps);
                        self.current = header;
                    }
                    None => self.terminate(Terminator::Goto(body_id)),
                }
                self.current = body_id;
                self.loops.push(LoopTargets {
                    continue_to: next,
                    break_to: exit,
                    scope_depth: self.scopes.len(),
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
                let locals = self.scopes.pop().expect("loop scope");
                self.end_scope(locals);
            }
            StmtKind::Block(block) => self.block(package, block),
        }
        if has_temp_scope {
            let temps = self.temp_scopes.pop().expect("statement temps");
            self.end_scope(temps);
        }
    }

    fn store_results(
        &mut self,
        package: &hir::Package,
        places: &[Option<Place>],
        value: &hir::Expr,
        span: Span,
    ) {
        if matches!(value.kind, ExprKind::Try(_)) {
            let operands = self.try_values(package, value);
            for (target, operand) in places.iter().zip(operands) {
                if let Some(target) = target {
                    self.push(target.clone(), Rvalue::Use(operand), span);
                }
            }
            return;
        }
        if places.len() > 1 {
            let results: Vec<Local> = value.types.iter().map(|&ty| self.temp(ty)).collect();
            self.call(
                package,
                value,
                results
                    .iter()
                    .map(|&local| Some(Place::local(local)))
                    .collect(),
            );
            for (target, &result) in places.iter().zip(&results) {
                if let Some(target) = target {
                    let ty = self.locals[result.0 as usize].ty;
                    self.push(
                        target.clone(),
                        Rvalue::Use(value_operand(package, Place::local(result), ty)),
                        span,
                    );
                }
            }
            return;
        }
        let operand = self.operand(package, value);
        if let Some(Some(target)) = places.first() {
            self.push(target.clone(), Rvalue::Use(operand), span);
        } else if matches!(operand, Operand::Copy(_) | Operand::Move(_)) {
            self.assign_temp(package, value.ty(), Rvalue::Use(operand), span);
        }
    }

    fn try_values(&mut self, package: &hir::Package, expr: &hir::Expr) -> Vec<Operand> {
        let ExprKind::Try(inner) = &expr.kind else {
            unreachable!("checked propagation expression")
        };
        let result_locals: Vec<Local> = inner.types.iter().map(|&ty| self.temp(ty)).collect();
        if matches!(inner.kind, ExprKind::Call { .. }) {
            self.call(
                package,
                inner,
                result_locals
                    .iter()
                    .map(|&local| Some(Place::local(local)))
                    .collect(),
            );
        } else {
            let operand = self.operand(package, inner);
            self.push(
                Place::local(result_locals[0]),
                Rvalue::Use(operand),
                inner.span,
            );
        }
        let error_local = *result_locals.last().expect("trailing error result");
        let condition = self.assign_temp(
            package,
            TypeStore::BOOL,
            Rvalue::Binary(
                BinaryOp::NotEq,
                Operand::Copy(Place::local(error_local)),
                Operand::Const(Const::Nil, TypeStore::ERROR),
            ),
            expr.span,
        );
        let failure = self.new_block();
        let success = self.new_block();
        self.terminate(Terminator::Branch {
            condition,
            then_block: failure,
            else_block: success,
            span: expr.span,
        });
        self.current = failure;
        let non_error_count = self.returns.len() - 1;
        let zero_returns = self.returns[..non_error_count].to_vec();
        for result in zero_returns {
            self.push(Place::local(result), Rvalue::Zero, expr.span);
        }
        self.push(
            Place::local(self.returns[non_error_count]),
            Rvalue::Use(Operand::Copy(Place::local(error_local))),
            expr.span,
        );
        self.terminate(Terminator::Return);
        self.current = success;
        result_locals[..result_locals.len() - 1]
            .iter()
            .map(|&local| {
                let ty = self.locals[local.0 as usize].ty;
                value_operand(package, Place::local(local), ty)
            })
            .collect()
    }

    fn place_type(&self, package: &hir::Package, place: &Place) -> TypeId {
        let mut ty = self.locals[place.local.0 as usize].ty;
        for projection in &place.projections {
            ty = match projection {
                Projection::Field(field) => {
                    let strukt = package
                        .types
                        .struct_id(ty)
                        .expect("field projection on a struct");
                    package.strukt(strukt).fields[field.0 as usize].ty
                }
                Projection::Index(_) => match package.types.kind(ty) {
                    TypeKind::Array { element, .. } => element,
                    _ => unreachable!("index projection on a non-array"),
                },
            };
        }
        ty
    }

    fn evaluate_to_temporary(&mut self, package: &hir::Package, expr: &hir::Expr) -> Operand {
        let operand = self.operand(package, expr);
        match operand {
            Operand::Const(..) => operand,
            _ => self.assign_temp(package, expr.ty(), Rvalue::Use(operand), expr.span),
        }
    }

    fn call(
        &mut self,
        package: &hir::Package,
        expr: &hir::Expr,
        mut destinations: Vec<Option<Place>>,
    ) {
        let (callee, args) = match &expr.kind {
            ExprKind::Call { function, args } => (Callee::Function(*function), &args[..]),
            ExprKind::Println(arg) => (Callee::Println, std::slice::from_ref(&**arg)),
            ExprKind::Drop(arg) => (Callee::Drop, std::slice::from_ref(&**arg)),
            _ => unreachable!("only calls produce multiple or no results"),
        };
        let mut operands = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            let (by_reference, by_ownership) = match &callee {
                Callee::Function(id) => {
                    let function = package.function(*id);
                    let mode = function.locals[function.params[index].0 as usize].kind;
                    (
                        mode == LocalKind::Param(ParamMode::Mut)
                            || mode == LocalKind::Param(ParamMode::Borrow)
                                && !package.is_copy(arg.ty()),
                        mode == LocalKind::Param(ParamMode::Own),
                    )
                }
                Callee::Println => (false, false),
                Callee::Drop => (false, true),
            };
            operands.push(if by_reference {
                match self.argument_place_opt(package, arg) {
                    Some(place) => Operand::Ref(place),
                    None => {
                        let operand = self.operand(package, arg);
                        let temp = self.temp(arg.ty());
                        self.push(Place::local(temp), Rvalue::Use(operand), arg.span);
                        Operand::Ref(Place::local(temp))
                    }
                }
            } else if by_ownership {
                self.operand(package, arg)
            } else {
                self.evaluate_to_temporary(package, arg)
            });
        }
        let args = operands;
        for (index, destination) in destinations.iter_mut().enumerate() {
            if destination.is_none() && !package.is_copy(expr.types[index]) {
                *destination = Some(Place::local(self.temp(expr.types[index])));
            }
        }
        let target = self.new_block();
        self.terminate(Terminator::Call {
            callee,
            args,
            destinations,
            target,
            unwind: None,
            span: expr.span,
        });
        self.current = target;
    }

    fn operand(&mut self, package: &hir::Package, expr: &hir::Expr) -> Operand {
        let span = expr.span;
        match &expr.kind {
            ExprKind::Const(c) => Operand::Const(c.clone(), expr.ty()),
            ExprKind::Local(id) => value_operand(package, Place::local(Local(id.0)), expr.ty()),
            ExprKind::Field { base, field } => match self.operand(package, base) {
                Operand::Copy(mut base) | Operand::Move(mut base) => {
                    base.projections.push(Projection::Field(*field));
                    value_operand(package, base, expr.ty())
                }
                other => {
                    let temp = self.temp(base.ty());
                    self.push(Place::local(temp), Rvalue::Use(other), span);
                    value_operand(
                        package,
                        Place {
                            local: temp,
                            projections: vec![Projection::Field(*field)],
                        },
                        expr.ty(),
                    )
                }
            },
            ExprKind::Index { base, index } => {
                // §12.6: evaluate the base, then the index, once each.
                let base_operand = self.operand(package, base);
                let index_operand = self.operand(package, index);
                match base_operand {
                    Operand::Copy(mut place) | Operand::Move(mut place) => {
                        place.projections.push(Projection::Index(index_operand));
                        value_operand(package, place, expr.ty())
                    }
                    other => {
                        let temp = self.temp(base.ty());
                        self.push(Place::local(temp), Rvalue::Use(other), span);
                        value_operand(
                            package,
                            Place {
                                local: temp,
                                projections: vec![Projection::Index(index_operand)],
                            },
                            expr.ty(),
                        )
                    }
                }
            }
            ExprKind::Call { .. } => {
                let temp = self.temp(expr.ty());
                self.call(package, expr, vec![Some(Place::local(temp))]);
                value_operand(package, Place::local(temp), expr.ty())
            }
            ExprKind::Try(_) => {
                let values = self.try_values(package, expr);
                debug_assert_eq!(values.len(), 1);
                values.into_iter().next().expect("single propagated result")
            }
            ExprKind::Println(_) => unreachable!("println has no value"),
            ExprKind::Drop(_) => unreachable!("drop has no value"),
            ExprKind::Convert(inner) => {
                let inner = self.operand(package, inner);
                self.assign_temp(package, expr.ty(), Rvalue::Convert(inner, expr.ty()), span)
            }
            ExprKind::Error(inner) => {
                let inner = self.operand(package, inner);
                self.assign_temp(package, expr.ty(), Rvalue::Error(inner), span)
            }
            ExprKind::StructLit { strukt, fields } => {
                // Evaluate in written order, then assemble in declaration order.
                let mut values: Vec<(usize, Operand)> = fields
                    .iter()
                    .map(|(field, value)| {
                        (field.0 as usize, self.evaluate_to_temporary(package, value))
                    })
                    .collect();
                values.sort_by_key(|(index, _)| *index);
                let operands = values.into_iter().map(|(_, operand)| operand).collect();
                self.assign_temp(
                    package,
                    expr.ty(),
                    Rvalue::Aggregate(AggregateKind::Struct(*strukt), operands),
                    span,
                )
            }
            ExprKind::ArrayLit { element, elements } => {
                let operands = elements
                    .iter()
                    .map(|value| self.evaluate_to_temporary(package, value))
                    .collect();
                self.assign_temp(
                    package,
                    expr.ty(),
                    Rvalue::Aggregate(AggregateKind::Array(*element), operands),
                    span,
                )
            }
            ExprKind::Unary { op, operand } => {
                let operand = self.operand(package, operand);
                self.assign_temp(package, expr.ty(), Rvalue::Unary(*op, operand), span)
            }
            ExprKind::Binary { op, lhs, rhs } if matches!(op, BinaryOp::And | BinaryOp::Or) => {
                self.short_circuit(package, *op, lhs, rhs, span)
            }
            ExprKind::Binary { op, lhs, rhs } => {
                // The left operand is fixed before the right is evaluated.
                let lhs = self.evaluate_to_temporary(package, lhs);
                let rhs = self.operand(package, rhs);
                self.assign_temp(package, expr.ty(), Rvalue::Binary(*op, lhs, rhs), span)
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
            span,
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

    /// The place an argument names, if it is a plain place rather than a
    /// temporary value; evaluates any index subexpressions it contains.
    fn argument_place_opt(&mut self, package: &hir::Package, expr: &hir::Expr) -> Option<Place> {
        match &expr.kind {
            ExprKind::Local(id) => Some(Place::local(Local(id.0))),
            ExprKind::Field { base, field } => {
                let mut place = self.argument_place_opt(package, base)?;
                place.projections.push(Projection::Field(*field));
                Some(place)
            }
            ExprKind::Index { base, index } => {
                let mut place = self.argument_place_opt(package, base)?;
                let index_operand = self.operand(package, index);
                place.projections.push(Projection::Index(index_operand));
                Some(place)
            }
            _ => None,
        }
    }

    fn place(&mut self, package: &hir::Package, place: &hir::Place) -> Place {
        let mut projections = Vec::with_capacity(place.projections.len());
        for projection in &place.projections {
            projections.push(match projection {
                hir::Projection::Field(id) => Projection::Field(*id),
                hir::Projection::Index(index) => Projection::Index(self.operand(package, index)),
            });
        }
        Place {
            local: Local(place.root.0),
            projections,
        }
    }
}

fn value_operand(package: &hir::Package, place: Place, ty: TypeId) -> Operand {
    if package.is_copy(ty) {
        Operand::Copy(place)
    } else {
        Operand::Move(place)
    }
}
