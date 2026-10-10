use std::collections::{HashMap, HashSet};

use super::closure_kind::visit_expr;
use super::{Checker, MutableUse, Value};
use crate::ast::{self, ParamMode};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, ExprKind, StmtKind};
use crate::resolve::{FunctionId, LocalKind, Res};
use crate::source::Span;
use crate::types::constant::Untyped;
use crate::types::{Constraint, TypeId, TypeKind, TypeStore};

impl Checker<'_> {
    /// Sets each type parameter's constraint once interface types exist.
    pub(super) fn type_param_constraints(&mut self) {
        let generic: Vec<(FunctionId, Vec<TypeId>)> = self
            .res
            .type_params
            .iter()
            .map(|(&id, params)| (id, params.clone()))
            .collect();
        for (id, params) in generic {
            let declaration = self.res.functions[id.0 as usize];
            for (param, &ty) in declaration.type_params.iter().zip(&params) {
                let name = match &param.constraint {
                    ast::Type::Named(name) | ast::Type::Qualified { name, .. } => name,
                    _ => continue,
                };
                let constraint = match self.res.uses.get(&name.span) {
                    Some(Res::Constraint(constraint)) => *constraint,
                    Some(Res::Interface(interface)) => match self.types.interface_of(*interface) {
                        Some(interface) => Constraint::Interface(interface),
                        None => continue,
                    },
                    _ => continue,
                };
                self.types.set_constraint(ty, constraint);
            }
        }
    }

    pub(super) fn is_generic(&self, id: FunctionId) -> bool {
        self.res.type_params.contains_key(&id)
    }

    pub(super) fn in_generic_body(&self) -> bool {
        self.current < self.res.functions.len() && self.is_generic(FunctionId(self.current as u32))
    }

    pub(super) fn reject_in_generic_body(&mut self, what: &str, span: Span) -> bool {
        if !self.in_generic_body() {
            return false;
        }
        self.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("{what} inside a generic function is not supported yet"),
                span,
            )
            .note("move this code into an ordinary function and call it"),
        );
        true
    }

    /// Binds the type parameters in `param` from the matching parts of `argument`.
    fn unify(&self, param: TypeId, argument: TypeId, bound: &mut HashMap<TypeId, TypeId>) {
        match (self.types.kind(param), self.types.kind(argument)) {
            (TypeKind::Param(_), _) => {
                bound.entry(param).or_insert(argument);
            }
            (
                TypeKind::Slice {
                    element: a,
                    mutable: m,
                },
                TypeKind::Slice {
                    element: b,
                    mutable: n,
                },
            ) if m == n => self.unify(a, b, bound),
            (
                TypeKind::Array {
                    element: a,
                    size: m,
                },
                TypeKind::Array {
                    element: b,
                    size: n,
                },
            ) if m == n => self.unify(a, b, bound),
            (TypeKind::DynArray { element: a }, TypeKind::DynArray { element: b })
            | (TypeKind::Channel { element: a }, TypeKind::Channel { element: b })
            | (TypeKind::Mutex { element: a }, TypeKind::Mutex { element: b }) => {
                self.unify(a, b, bound)
            }
            (TypeKind::Map { key: a, value: v }, TypeKind::Map { key: b, value: w }) => {
                self.unify(a, b, bound);
                self.unify(v, w, bound);
            }
            (TypeKind::Func(_), TypeKind::Func(_)) => {
                let (Some(want), Some(have)) = (
                    self.types.func_signature(param).cloned(),
                    self.types.func_signature(argument).cloned(),
                ) else {
                    return;
                };
                if want.params.len() != have.params.len()
                    || want.results.len() != have.results.len()
                {
                    return;
                }
                for (&(_, a), &(_, b)) in want.params.iter().zip(&have.params) {
                    self.unify(a, b, bound);
                }
                for (&a, &b) in want.results.iter().zip(&have.results) {
                    self.unify(a, b, bound);
                }
            }
            _ => {}
        }
    }

    fn constraint_name(&self, constraint: Constraint) -> String {
        match constraint {
            Constraint::Any => "any".into(),
            Constraint::Copyable => "copyable".into(),
            Constraint::Comparable => "comparable".into(),
            Constraint::Ordered => "ordered".into(),
            Constraint::Interface(id) => self.name(self.types.interface_type(id)),
        }
    }

    fn type_argument_allowed(&mut self, param: TypeId, argument: TypeId, span: Span) -> bool {
        let constraint = self.types.constraint(param).unwrap_or(Constraint::Any);
        if let Some(inner) = self.types.constraint(argument) {
            return self.type_param_argument_allowed(param, constraint, argument, inner, span);
        }
        let holds_function_or_mutable_view = self.type_contains(argument, &|kind| {
            matches!(
                kind,
                TypeKind::Func(_)
                    | TypeKind::Slice { mutable: true, .. }
                    | TypeKind::InterfaceView { .. }
            )
        });
        if holds_function_or_mutable_view {
            let message = format!(
                "`{}` cannot be the type argument for `{}`",
                self.name(argument),
                self.name(param)
            );
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, span).note(
                    "a type argument cannot hold a function value, a `mut []T` view, or a borrowed interface value",
                ),
            );
            return false;
        }
        let satisfied = match constraint {
            Constraint::Any => true,
            Constraint::Copyable => self.type_is_copy(argument),
            Constraint::Comparable => self.is_map_key(argument),
            Constraint::Ordered => matches!(
                self.types.kind(argument),
                TypeKind::Int(_) | TypeKind::Float(_) | TypeKind::Rune | TypeKind::String
            ),
            Constraint::Interface(interface) => {
                return self.require_satisfied(argument, interface, span);
            }
        };
        if !satisfied {
            let message = format!(
                "`{}` does not satisfy `{}`, the constraint of `{}`",
                self.name(argument),
                self.constraint_name(constraint),
                self.name(param)
            );
            let note = match constraint {
                Constraint::Copyable => {
                    "a copyable type is copied on assignment: numbers, `bool`, `rune`, `string`, `error`, and structs and fixed arrays of those"
                }
                Constraint::Comparable => {
                    "a comparable type is `bool`, an integer type, `rune`, `string`, or a named type built on one"
                }
                _ => {
                    "an ordered type is an integer or float type, `rune`, `string`, or a named type built on one"
                }
            };
            self.diagnostics
                .push(Diagnostic::new(Severity::Error, message, span).note(note));
        }
        satisfied
    }

    /// An enclosing function's type parameter passed on: its own constraint must promise as much.
    fn type_param_argument_allowed(
        &mut self,
        param: TypeId,
        constraint: Constraint,
        argument: TypeId,
        inner: Constraint,
        span: Span,
    ) -> bool {
        let satisfied = match (constraint, inner) {
            (Constraint::Any, _) => true,
            (
                Constraint::Copyable,
                Constraint::Copyable | Constraint::Comparable | Constraint::Ordered,
            ) => true,
            (Constraint::Comparable, Constraint::Comparable)
            | (Constraint::Ordered, Constraint::Ordered) => true,
            (Constraint::Interface(wanted), Constraint::Interface(have)) => {
                let have = self.types.interface_type(have);
                return self.require_satisfied(have, wanted, span);
            }
            _ => false,
        };
        if !satisfied {
            let message = format!(
                "`{}` does not satisfy `{}`, the constraint of `{}`",
                self.name(argument),
                self.constraint_name(constraint),
                self.name(param)
            );
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, span).note(format!(
                    "`{}` is only known to satisfy `{}`",
                    self.name(argument),
                    self.constraint_name(inner)
                )),
            );
        }
        satisfied
    }

    /// A call of a generic function: infers its type arguments from the arguments, then checks it like any call.
    pub(super) fn generic_call(
        &mut self,
        id: FunctionId,
        name: &str,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let Some(signature) = &self.signatures[id.0 as usize] else {
            self.report_arg_errors(args);
            return None;
        };
        let (params, results) = (signature.params.clone(), signature.results.clone());
        let type_params = self.res.type_params[&id].clone();
        if args.len() != params.len() {
            let message = format!(
                "`{name}` takes {} argument{} but {} {} given",
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len(),
                if args.len() == 1 { "was" } else { "were" },
            );
            self.error(message, span);
            self.report_arg_errors(args);
            return None;
        }
        let mut values = Vec::new();
        for (arg, &param) in args.iter().zip(&params) {
            let expected = (!self.types.mentions_param(param)).then_some(param);
            let value = match self.expr(arg, expected) {
                Some(Value::Typed(expr)) => self.single_value(expr).map(Value::Typed),
                other => other,
            };
            values.push(value);
        }
        let mut bound = HashMap::new();
        for (value, &param) in values.iter().zip(&params) {
            if let Some(Value::Typed(expr)) = value {
                self.unify(param, expr.ty(), &mut bound);
            }
        }
        for (value, &param) in values.iter().zip(&params) {
            if let (Some(Value::Untyped(untyped, _)), TypeKind::Param(_)) =
                (value, self.types.kind(param))
                && !bound.contains_key(&param)
            {
                let default = match untyped {
                    Untyped::Int(_) => TypeStore::INT,
                    Untyped::Float(_) => TypeStore::FLOAT64,
                };
                bound.insert(param, default);
            }
        }
        if values.iter().any(Option::is_none) {
            return None;
        }
        let mut ok = true;
        let mut type_args = Vec::new();
        for &param in &type_params {
            match bound.get(&param) {
                Some(&argument) => {
                    ok &= self.type_argument_allowed(param, argument, span);
                    type_args.push(argument);
                }
                None => {
                    let message = format!(
                        "cannot infer `{}` for this call of `{name}`",
                        self.name(param)
                    );
                    self.diagnostics.push(
                        Diagnostic::new(Severity::Error, message, span).note(
                            "type arguments come from the arguments; pass a value whose type decides it",
                        ),
                    );
                    ok = false;
                }
            }
        }
        if !ok {
            return None;
        }
        let mut checked = Vec::new();
        for (value, &param) in values.into_iter().zip(&params) {
            let param = self.types.substitute(param, &bound);
            match value.and_then(|value| self.coerce(value, param)) {
                Some(expr) => checked.push(expr),
                None => ok = false,
            }
        }
        if !ok {
            return None;
        }
        let modes: Vec<ParamMode> = self.res.locals[id.0 as usize]
            .iter()
            .take(params.len())
            .map(|local| match local.kind {
                LocalKind::Param(mode) => mode,
                _ => ParamMode::Borrow,
            })
            .collect();
        let accesses: Vec<_> = modes
            .iter()
            .zip(&params)
            .map(|(&mode, &param)| {
                let ty = self.types.substitute(param, &bound);
                self.argument_access(mode, ty)
            })
            .collect();
        if !self.check_argument_accesses(&accesses, &checked) {
            return None;
        }
        if self.is_async_function(id) && self.awaited_call != Some(span) {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("call to async function `{name}` is neither awaited nor spawned"),
                    span,
                )
                .note("write `await` before the call, or spawn it with `go`"),
            );
            return None;
        }
        let results = results
            .into_iter()
            .map(|result| self.types.substitute(result, &bound))
            .collect();
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::CallGeneric {
                function: id,
                type_args,
                args: checked,
            },
            types: results,
            span,
        }))
    }

    /// A method of a type parameter's interface constraint, called on the value inside it.
    pub(super) fn type_param_method_call(
        &mut self,
        receiver: hir::Expr,
        name: &ast::Name,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let ty = receiver.ty();
        let Some(Constraint::Interface(interface)) = self.types.constraint(ty) else {
            let message = format!(
                "type parameter `{}` has no methods; its constraint `{}` lists none",
                self.name(ty),
                self.constraint_name(self.types.constraint(ty).unwrap_or(Constraint::Any))
            );
            self.error(message, name.span);
            self.report_arg_errors(args);
            return None;
        };
        let Some(entry) = self
            .types
            .interface_methods(interface)
            .iter()
            .find(|entry| entry.name == name.text)
            .cloned()
        else {
            let message = format!(
                "type parameter `{}` has no method `{}`",
                self.name(ty),
                name.text
            );
            self.error(message, name.span);
            self.report_arg_errors(args);
            return None;
        };
        let span_of = receiver.span;
        let converted = match entry.receiver {
            ParamMode::Own => hir::Expr {
                kind: ExprKind::InterfaceBox(Box::new(receiver)),
                types: vec![self.types.interface_type(interface)],
                span: span_of,
            },
            mode => {
                let mutable = mode == ParamMode::Mut;
                if mutable && !self.mutable_place(&receiver, MutableUse::Argument) {
                    self.report_arg_errors(args);
                    return None;
                }
                hir::Expr {
                    kind: ExprKind::InterfaceView {
                        source: Box::new(receiver),
                        mutable,
                    },
                    types: vec![self.types.interface_view(interface, mutable)],
                    span: span_of,
                }
            }
        };
        self.interface_call(converted, name, args, span)
    }

    /// Replaces each generic call with a call of a copy made for its type arguments.
    pub(super) fn instantiate(&mut self, functions: &mut Vec<hir::Function>) {
        let mut copies: HashMap<(FunctionId, Vec<TypeId>, bool), FunctionId> = HashMap::new();
        let mut depths = vec![0; functions.len()];
        let mut runaway = HashSet::new();
        let mut pending: Vec<usize> = (0..functions.len())
            .filter(|&index| functions[index].type_params.is_empty())
            .collect();
        for index in 0..functions.len() {
            if functions[index].type_params.is_empty() {
                continue;
            }
            let placeholders: Vec<TypeId> = functions[index]
                .type_params
                .iter()
                .map(|&param| self.placeholder(param))
                .collect();
            let key = (FunctionId(index as u32), placeholders, true);
            let copy = self.copy(functions, key.0, &key.1, true);
            copies.insert(key, copy);
            depths.push(0);
            pending.push(copy.0 as usize);
        }
        while let Some(index) = pending.pop() {
            let probe = functions[index].probe;
            let depth = depths[index];
            let span = functions[index].span;
            let mut body = std::mem::replace(
                &mut functions[index].body,
                hir::Block {
                    stmts: Vec::new(),
                    span,
                },
            );
            each_expr(&mut body, &mut |expr| {
                let ExprKind::CallGeneric {
                    function,
                    type_args,
                    args,
                } = &mut expr.kind
                else {
                    return;
                };
                let key = (*function, type_args.clone(), probe);
                let copy = match copies.get(&key) {
                    Some(&copy) => copy,
                    None if depth >= MAX_COPY_DEPTH => {
                        if !runaway.insert(*function) {
                            return;
                        }
                        self.error(
                            format!(
                                "generic function `{}` keeps calling itself with new type arguments",
                                functions[function.0 as usize].name
                            ),
                            expr.span,
                        );
                        return;
                    }
                    None => {
                        if !probe {
                            self.satisfy_interface_constraints(
                                functions, &key.0, &key.1, expr.span,
                            );
                        }
                        let copy = self.copy(functions, key.0, &key.1, probe);
                        copies.insert(key, copy);
                        depths.push(depth + 1);
                        pending.push(copy.0 as usize);
                        copy
                    }
                };
                expr.kind = ExprKind::Call {
                    function: copy,
                    args: std::mem::take(args),
                };
            });
            functions[index].body = body;
        }
    }

    /// The type a probe uses for a type parameter: a copy type, or a type that moves.
    fn placeholder(&mut self, param: TypeId) -> TypeId {
        match self.types.constraint(param).unwrap_or(Constraint::Any) {
            Constraint::Copyable | Constraint::Comparable | Constraint::Ordered => TypeStore::INT64,
            Constraint::Any | Constraint::Interface(_) => self.types.dyn_array_type(TypeStore::INT),
        }
    }

    fn satisfy_interface_constraints(
        &mut self,
        functions: &[hir::Function],
        template: &FunctionId,
        type_args: &[TypeId],
        span: Span,
    ) {
        let params = functions[template.0 as usize].type_params.clone();
        for (param, &argument) in params.iter().zip(type_args) {
            if let Some(Constraint::Interface(interface)) = self.types.constraint(*param) {
                self.require_satisfied(argument, interface, span);
            }
        }
    }

    fn copy(
        &mut self,
        functions: &mut Vec<hir::Function>,
        template: FunctionId,
        type_args: &[TypeId],
        probe: bool,
    ) -> FunctionId {
        let mut copy = functions[template.0 as usize].clone();
        let bound: HashMap<TypeId, TypeId> = copy
            .type_params
            .iter()
            .copied()
            .zip(type_args.iter().copied())
            .collect();
        let names: Vec<String> = type_args.iter().map(|&ty| self.name(ty)).collect();
        copy.name = format!("{}<{}>", copy.name, names.join(", "));
        copy.type_params = Vec::new();
        copy.probe = probe;
        let types = &mut self.types;
        for local in &mut copy.locals {
            local.ty = types.substitute(local.ty, &bound);
        }
        for result in &mut copy.results {
            *result = types.substitute(*result, &bound);
        }
        each_place(&mut copy.body, &mut |place| {
            place.ty = types.substitute(place.ty, &bound);
        });
        each_expr(&mut copy.body, &mut |expr| {
            for ty in &mut expr.types {
                *ty = types.substitute(*ty, &bound);
            }
            match &mut expr.kind {
                ExprKind::ArrayLit { element, .. } | ExprKind::MakeChannel { element, .. } => {
                    *element = types.substitute(*element, &bound);
                }
                ExprKind::MapLit { key, value, .. } => {
                    *key = types.substitute(*key, &bound);
                    *value = types.substitute(*value, &bound);
                }
                ExprKind::Spawn { closure_ty, .. } => {
                    *closure_ty = types.substitute(*closure_ty, &bound);
                }
                ExprKind::CallGeneric { type_args, .. } => {
                    for ty in type_args {
                        *ty = types.substitute(*ty, &bound);
                    }
                }
                _ => {}
            }
        });
        functions.push(copy);
        FunctionId(functions.len() as u32 - 1)
    }
}

const MAX_COPY_DEPTH: usize = 64;

fn each_expr(block: &mut hir::Block, visit: &mut dyn FnMut(&mut hir::Expr)) {
    for stmt in &mut block.stmts {
        each_stmt_expr(stmt, visit);
    }
}

fn each_stmt_expr(stmt: &mut hir::Stmt, visit: &mut dyn FnMut(&mut hir::Expr)) {
    let place_exprs = |place: &mut hir::Place, visit: &mut dyn FnMut(&mut hir::Expr)| {
        for projection in &mut place.projections {
            if let hir::Projection::Index(index) = projection {
                visit_expr(index, visit);
            }
        }
    };
    match &mut stmt.kind {
        StmtKind::Let { value, .. } | StmtKind::Expr(value) | StmtKind::SetGlobal { value, .. } => {
            visit_expr(value, visit)
        }
        StmtKind::Assign { targets, values } => {
            for target in targets.iter_mut().flatten() {
                place_exprs(target, visit);
            }
            for value in values {
                visit_expr(value, visit);
            }
        }
        StmtKind::MapAssign { map, key, value } => {
            place_exprs(map, visit);
            visit_expr(key, visit);
            visit_expr(value, visit);
        }
        StmtKind::CompoundAssign { place, value, .. } => {
            place_exprs(place, visit);
            visit_expr(value, visit);
        }
        StmtKind::Return(values) => {
            for value in values {
                visit_expr(value, visit);
            }
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            visit_expr(condition, visit);
            each_expr(then_block, visit);
            if let Some(block) = else_block {
                each_expr(block, visit);
            }
        }
        StmtKind::Loop {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                each_stmt_expr(init, visit);
            }
            if let Some(condition) = condition {
                visit_expr(condition, visit);
            }
            if let Some(update) = update {
                each_stmt_expr(update, visit);
            }
            each_expr(body, visit);
        }
        StmtKind::ForEach {
            collection, body, ..
        } => {
            visit_expr(collection, visit);
            each_expr(body, visit);
        }
        StmtKind::Select { arms, default } => {
            for arm in arms {
                match &mut arm.comm {
                    hir::SelectComm::Receive { channel, .. } => visit_expr(channel, visit),
                    hir::SelectComm::Send { channel, value } => {
                        visit_expr(channel, visit);
                        visit_expr(value, visit);
                    }
                }
                each_expr(&mut arm.body, visit);
            }
            if let Some(block) = default {
                each_expr(block, visit);
            }
        }
        StmtKind::Block(block) => each_expr(block, visit),
        StmtKind::Break | StmtKind::Continue => {}
    }
}

fn each_place(block: &mut hir::Block, visit: &mut dyn FnMut(&mut hir::Place)) {
    for stmt in &mut block.stmts {
        each_stmt_place(stmt, visit);
    }
}

fn each_stmt_place(stmt: &mut hir::Stmt, visit: &mut dyn FnMut(&mut hir::Place)) {
    match &mut stmt.kind {
        StmtKind::Assign { targets, .. } => targets.iter_mut().flatten().for_each(&mut *visit),
        StmtKind::MapAssign { map: place, .. } | StmtKind::CompoundAssign { place, .. } => {
            visit(place)
        }
        StmtKind::If {
            then_block,
            else_block,
            ..
        } => {
            each_place(then_block, visit);
            if let Some(block) = else_block {
                each_place(block, visit);
            }
        }
        StmtKind::Loop {
            init, update, body, ..
        } => {
            if let Some(init) = init {
                each_stmt_place(init, visit);
            }
            if let Some(update) = update {
                each_stmt_place(update, visit);
            }
            each_place(body, visit);
        }
        StmtKind::ForEach { body, .. } | StmtKind::Block(body) => each_place(body, visit),
        StmtKind::Select { arms, default } => {
            for arm in arms {
                each_place(&mut arm.body, visit);
            }
            if let Some(block) = default {
                each_place(block, visit);
            }
        }
        _ => {}
    }
}
