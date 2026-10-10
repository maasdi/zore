use super::closure_kind::stmt_exprs;
use super::{Checker, Value, typed};
use crate::ast::{self, ParamMode};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, ExprKind, StmtKind};
use crate::resolve::{FunctionId, LocalId, LocalKind};
use crate::source::Span;
use crate::types::{FuncSignature, TypeId, TypeKind};

const CALLEE_MESSAGE: &str = "`go` needs a call to a declared function or method, a function-typed local, or a closure literal";

impl Checker<'_> {
    pub(super) fn go_expr(&mut self, operand: &ast::Expr, span: Span) -> Option<Value> {
        let mut call = operand;
        while let ast::ExprKind::Paren(inner) = &call.kind {
            call = inner;
        }
        if !matches!(call.kind, ast::ExprKind::Call { .. }) {
            self.error("`go` needs a call", span);
            return None;
        }
        let previous = self.awaited_call.replace(call.span);
        let value = self.expr(operand, None);
        self.awaited_call = previous;
        let Value::Typed(expr) = value? else {
            self.error("`go` needs a call", span);
            return None;
        };
        let results = expr.types;
        match expr.kind {
            ExprKind::Call { function, args } => self.spawn_declared(function, args, results, span),
            ExprKind::CallValue { callee, args, .. } => {
                self.spawn_callable(*callee, args, results, span)
            }
            _ => {
                self.error(CALLEE_MESSAGE, span);
                None
            }
        }
    }

    fn spawn_declared(
        &mut self,
        function: FunctionId,
        args: Vec<hir::Expr>,
        results: Vec<TypeId>,
        span: Span,
    ) -> Option<Value> {
        let modes: Vec<ParamMode> = (0..args.len())
            .map(|index| {
                let LocalKind::Param(mode) = self.res.locals[function.0 as usize][index].kind
                else {
                    unreachable!("parameters come first among a function's locals")
                };
                mode
            })
            .collect();
        if !self.spawn_inputs_are_independent(&modes, &args) {
            return None;
        }
        let task = self.task_type(results.clone(), span)?;
        let closure_ty = self.types.func_type(FuncSignature {
            is_async: false,
            params: Vec::new(),
            results: results.clone(),
        });
        let thunk = self.spawn_thunk(function, &args, results, span);
        Some(Value::Typed(typed(
            ExprKind::Spawn {
                thunk,
                closure_ty,
                callable: false,
                args,
            },
            task,
            span,
        )))
    }

    fn spawn_callable(
        &mut self,
        callee: hir::Expr,
        args: Vec<hir::Expr>,
        results: Vec<TypeId>,
        span: Span,
    ) -> Option<Value> {
        if !matches!(callee.kind, ExprKind::Local(_) | ExprKind::Closure { .. }) {
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, CALLEE_MESSAGE, callee.span).note(
                    "a field, element, map value, or call result cannot be moved into a task",
                ),
            );
            return None;
        }
        let modes: Vec<ParamMode> = self
            .types
            .func_signature(callee.ty())?
            .params
            .iter()
            .map(|&(mode, _)| mode)
            .collect();
        if !self.spawn_inputs_are_independent(&modes, &args) {
            return None;
        }
        let task = self.task_type(results.clone(), span)?;
        let closure_ty = self.types.func_type(FuncSignature {
            is_async: false,
            params: Vec::new(),
            results: results.clone(),
        });
        let callee_ty = callee.ty();
        let is_async = self
            .types
            .func_signature(callee_ty)
            .is_some_and(|signature| signature.is_async);
        let thunk = self.spawn_callable_thunk(callee_ty, &args, results, is_async, span);
        let mut spawned = vec![callee];
        spawned.extend(args);
        Some(Value::Typed(typed(
            ExprKind::Spawn {
                thunk,
                closure_ty,
                callable: true,
                args: spawned,
            },
            task,
            span,
        )))
    }

    /// Every input must be valid for the task's whole life, whatever the spawner does next.
    fn spawn_inputs_are_independent(&mut self, modes: &[ParamMode], args: &[hir::Expr]) -> bool {
        let mut ok = true;
        for (&mode, arg) in modes.iter().zip(args) {
            let ty = arg.ty();
            let message = if mode == ParamMode::Mut {
                "a spawned call cannot take a `mut` parameter, since copying the argument would change what the caller sees"
            } else if self.type_contains(ty, &|kind| {
                matches!(
                    kind,
                    TypeKind::Slice { .. } | TypeKind::InterfaceView { .. }
                )
            }) {
                "a spawned call cannot take a view, since it may borrow storage the task does not own"
            } else if self.holds_func(ty) && mode != ParamMode::Own {
                "a spawned call can take a function value only for an `own` parameter, since the task cannot borrow the spawner's storage"
            } else if mode == ParamMode::Borrow && !self.type_is_copy(ty) {
                "a spawned call needs `own` to take a value that is moved, since the task cannot borrow the spawner's storage"
            } else {
                continue;
            };
            self.error(message, arg.span);
            ok = false;
        }
        ok
    }

    /// An owning closure over the arguments that makes the call; the runtime runs it once.
    fn spawn_thunk(
        &mut self,
        function: FunctionId,
        args: &[hir::Expr],
        results: Vec<TypeId>,
        span: Span,
    ) -> FunctionId {
        let captures: Vec<LocalId> = (0..args.len()).map(|i| LocalId(i as u32)).collect();
        let call = hir::Expr {
            kind: ExprKind::Call {
                function,
                args: captured_arguments(args.iter().map(hir::Expr::ty), 0, span),
            },
            types: results.clone(),
            span,
        };
        let types: Vec<TypeId> = args.iter().map(hir::Expr::ty).collect();
        let is_async = self.is_async_function(function);
        self.generated_thunk(&types, captures, call, results, is_async, span)
    }

    /// An owning closure over the callable and its arguments; the runtime runs it once.
    fn spawn_callable_thunk(
        &mut self,
        callee_ty: TypeId,
        args: &[hir::Expr],
        results: Vec<TypeId>,
        is_async: bool,
        span: Span,
    ) -> FunctionId {
        let mut types = vec![callee_ty];
        types.extend(args.iter().map(hir::Expr::ty));
        let captures: Vec<LocalId> = (0..types.len()).map(|i| LocalId(i as u32)).collect();
        let call = hir::Expr {
            kind: ExprKind::CallValue {
                callee: Box::new(typed(ExprKind::Local(LocalId(0)), callee_ty, span)),
                args: captured_arguments(args.iter().map(hir::Expr::ty), 1, span),
                once: false,
            },
            types: results.clone(),
            span,
        };
        self.generated_thunk(&types, captures, call, results, is_async, span)
    }

    fn generated_thunk(
        &mut self,
        capture_types: &[TypeId],
        captures: Vec<LocalId>,
        call: hir::Expr,
        results: Vec<TypeId>,
        is_async: bool,
        span: Span,
    ) -> FunctionId {
        let id = FunctionId(
            (self.res.functions.len() + self.closures.len() + self.generated_functions.len())
                as u32,
        );
        let locals = capture_types
            .iter()
            .enumerate()
            .map(|(index, &ty)| hir::Local {
                name: format!("arg{index}"),
                ty,
                kind: LocalKind::Capture(LocalId(index as u32)),
                span,
            })
            .collect();
        let statement = if results.is_empty() {
            StmtKind::Expr(call)
        } else {
            StmtKind::Return(vec![call])
        };
        let body = hir::Block {
            stmts: vec![hir::Stmt {
                kind: statement,
                span,
            }],
            span,
        };
        let call_once = self.consumes_capture(&body, &captures);
        let name = format!(
            "{}$go{}",
            self.function_name(FunctionId(self.current as u32)),
            self.generated_functions.len()
        );
        self.generated_functions.push(hir::Function {
            name,
            span,
            params: Vec::new(),
            results,
            captures,
            is_closure: true,
            is_async,
            native: false,
            call_once,
            locals,
            body,
        });
        id
    }

    /// A spawned closure owns its captures, so each must be something the task can own alone.
    pub(super) fn check_spawned_captures(
        &mut self,
        function: FunctionId,
        captures: &[(LocalId, bool)],
        span: Span,
    ) {
        let index = function.0 as usize - self.res.functions.len();
        let inner: Vec<LocalId> = self
            .closures
            .get(index)
            .and_then(Option::as_ref)
            .map(|closure| closure.captures.clone())
            .unwrap_or_default();
        for (position, &(outer, exclusive)) in captures.iter().enumerate() {
            let Some(ty) = self.locals[outer.0 as usize] else {
                continue;
            };
            let declaration = &self.res.locals[self.current][outer.0 as usize];
            let (name, declared_at) = (declaration.name.clone(), declaration.span);
            let (message, note, at) = if self.binding_kind(outer)
                == LocalKind::Param(ParamMode::Mut)
            {
                (
                    format!("a spawned closure cannot capture `mut` parameter `{name}`"),
                    "the task could outlive the exclusive access the caller granted",
                    span,
                )
            } else if self.type_contains(ty, &|kind| {
                matches!(
                    kind,
                    TypeKind::Slice { .. } | TypeKind::InterfaceView { .. }
                )
            }) {
                (
                    format!("a spawned closure cannot capture `{name}`, which holds a view"),
                    "a view borrows storage the task does not own",
                    span,
                )
            } else if exclusive && self.type_is_copy(ty) {
                let change = inner
                    .get(position)
                    .and_then(|&local| self.first_change(index, local))
                    .unwrap_or(span);
                (
                    format!(
                        "a spawned closure cannot change `{name}`: the task has its own copy, so the change would not be seen outside"
                    ),
                    "start from a local copy inside the closure, or share a change through a channel or a `mutex`",
                    change,
                )
            } else {
                continue;
            };
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, at)
                    .related(declared_at, "captured from here")
                    .note(note),
            );
        }
    }

    fn first_change(&mut self, closure_index: usize, local: LocalId) -> Option<Span> {
        let mut closure = self.closures.get_mut(closure_index)?.take()?;
        let found = self.change_in_block(&mut closure.body, local);
        self.closures[closure_index] = Some(closure);
        found
    }

    fn change_in_block(&self, block: &mut hir::Block, local: LocalId) -> Option<Span> {
        block
            .stmts
            .iter_mut()
            .find_map(|stmt| self.change_in_stmt(stmt, local))
    }

    fn change_in_stmt(&self, stmt: &mut hir::Stmt, local: LocalId) -> Option<Span> {
        match &stmt.kind {
            StmtKind::Assign { targets, .. } => {
                if let Some(place) = targets.iter().flatten().find(|place| place.root == local) {
                    return Some(place.span);
                }
            }
            StmtKind::CompoundAssign { place, .. } if place.root == local => {
                return Some(place.span);
            }
            _ => {}
        }
        for expr in stmt_exprs(stmt) {
            let mut places = Vec::new();
            self.mutated_places(expr, &mut places);
            if places.iter().any(|(root, _)| *root == local) {
                return Some(expr.span);
            }
        }
        match &mut stmt.kind {
            StmtKind::If {
                then_block,
                else_block,
                ..
            } => self.change_in_block(then_block, local).or_else(|| {
                else_block
                    .as_mut()
                    .and_then(|block| self.change_in_block(block, local))
            }),
            StmtKind::Loop {
                init, update, body, ..
            } => init
                .as_mut()
                .and_then(|stmt| self.change_in_stmt(stmt, local))
                .or_else(|| {
                    update
                        .as_mut()
                        .and_then(|stmt| self.change_in_stmt(stmt, local))
                })
                .or_else(|| self.change_in_block(body, local)),
            StmtKind::ForEach { body, .. } | StmtKind::Block(body) => {
                self.change_in_block(body, local)
            }
            StmtKind::Select { arms, default } => arms
                .iter_mut()
                .find_map(|arm| self.change_in_block(&mut arm.body, local))
                .or_else(|| {
                    default
                        .as_mut()
                        .and_then(|block| self.change_in_block(block, local))
                }),
            _ => None,
        }
    }
}

fn captured_arguments(
    types: impl Iterator<Item = TypeId>,
    first_capture: usize,
    span: Span,
) -> Vec<hir::Expr> {
    types
        .enumerate()
        .map(|(index, ty)| {
            typed(
                ExprKind::Local(LocalId((first_capture + index) as u32)),
                ty,
                span,
            )
        })
        .collect()
}
