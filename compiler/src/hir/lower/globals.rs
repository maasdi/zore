//! Checks package-level `let` values once every function is known, and builds their initializer.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, ExprKind, SelectComm, StmtKind};
use crate::resolve::{FunctionId, GlobalId};
use crate::types::{InterfaceId, TypeId};

/// `initializers[i]` computes global `i`.
pub(in crate::hir) fn check(
    package: &mut hir::Package,
    initializers: &[FunctionId],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for global in &package.globals {
        if !package.storable_globally(global.ty) {
            diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!(
                        "a package-level `let` cannot hold a `{}`",
                        package.types.display(global.ty)
                    ),
                    global.span,
                )
                .note("it must be a Copy value without slices, function values, tasks, channels, or mutexes"),
            );
        }
    }
    if package.globals.is_empty() {
        return diagnostics;
    }
    let references: Vec<References> = package
        .functions
        .iter()
        .map(|function| references(package, function))
        .collect();
    for (index, &function) in initializers.iter().enumerate() {
        let used = reached_globals(package, &references, function);
        let mut later: Vec<&GlobalId> = used.iter().filter(|g| g.0 as usize >= index).collect();
        later.sort_by_key(|g| g.0);
        if let Some(&&first) = later.first() {
            let global = &package.globals[index];
            let message = if first.0 as usize == index {
                format!("`{}` is used to compute itself", short(&global.name))
            } else {
                format!(
                    "`{}` is computed from `{}`, which is initialized after it",
                    short(&global.name),
                    short(&package.globals[first.0 as usize].name)
                )
            };
            diagnostics.push(
                Diagnostic::new(Severity::Error, message, global.span).note(
                    "an initializer can use, directly or through the functions it calls, only values initialized before it",
                ),
            );
        }
    }
    if !diagnostics.is_empty() {
        return diagnostics;
    }
    let stmts = package
        .globals
        .iter()
        .zip(initializers)
        .enumerate()
        .map(|(index, (global, &function))| hir::Stmt {
            kind: StmtKind::SetGlobal {
                global: GlobalId(index as u32),
                value: hir::Expr {
                    kind: ExprKind::Call {
                        function,
                        args: Vec::new(),
                    },
                    types: vec![global.ty],
                    span: global.span,
                },
            },
            span: global.span,
        })
        .collect();
    let span = package.globals[0].span;
    package.init = Some(FunctionId(package.functions.len() as u32));
    package.functions.push(hir::Function {
        name: "zore$init".into(),
        span,
        params: Vec::new(),
        results: Vec::new(),
        captures: Vec::new(),
        is_closure: false,
        is_async: false,
        native: false,
        call_once: false,
        type_params: Vec::new(),
        probe: false,
        locals: Vec::new(),
        body: hir::Block { stmts, span },
    });
    diagnostics
}

fn short(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

#[derive(Default)]
struct References {
    globals: HashSet<GlobalId>,
    functions: HashSet<FunctionId>,
    interfaces: HashSet<(TypeId, usize)>,
    conversions: HashSet<(TypeId, TypeId)>,
    types: HashSet<TypeId>,
}

fn reached_globals(
    package: &hir::Package,
    references: &[References],
    start: FunctionId,
) -> HashSet<GlobalId> {
    let mut seen = HashSet::new();
    let mut globals = HashSet::new();
    let mut calls: HashMap<InterfaceId, HashSet<usize>> = HashMap::new();
    let mut sources: HashMap<InterfaceId, HashSet<TypeId>> = HashMap::new();
    let mut pending_functions = vec![start];
    let mut pending_calls = Vec::new();
    let mut pending_conversions = Vec::new();
    loop {
        if let Some(function) = pending_functions.pop() {
            if !seen.insert(function) {
                continue;
            }
            let found = &references[function.0 as usize];
            globals.extend(found.globals.iter().copied());
            pending_functions.extend(found.functions.iter().copied());
            for &(ty, method) in &found.interfaces {
                let interface = package.types.interface_of(ty).expect("interface receiver");
                pending_calls.push((interface, method));
            }
            for &(source, target) in &found.conversions {
                let source = match package.types.interface_of(source) {
                    Some(interface) => package.types.interface_type(interface),
                    None => source,
                };
                let target = package
                    .types
                    .interface_of(target)
                    .expect("interface conversion");
                pending_conversions.push((source, target));
            }
        } else if let Some((interface, method)) = pending_calls.pop() {
            if !calls.entry(interface).or_default().insert(method) {
                continue;
            }
            for &source in sources.get(&interface).into_iter().flatten() {
                dispatch(
                    package,
                    (source, interface, method),
                    &mut pending_functions,
                    &mut pending_calls,
                );
            }
        } else if let Some((source, target)) = pending_conversions.pop() {
            if !sources.entry(target).or_default().insert(source) {
                continue;
            }
            for &method in calls.get(&target).into_iter().flatten() {
                dispatch(
                    package,
                    (source, target, method),
                    &mut pending_functions,
                    &mut pending_calls,
                );
            }
        } else {
            return globals;
        }
    }
}

fn dispatch(
    package: &hir::Package,
    (source, target, method): (TypeId, InterfaceId, usize),
    functions: &mut Vec<FunctionId>,
    calls: &mut Vec<(InterfaceId, usize)>,
) {
    let Some(entries) = package.implementations.get(&(source, target)) else {
        return;
    };
    match entries[method] {
        hir::Implementation::Method(function) => functions.push(function),
        hir::Implementation::Entry(index) => {
            let inner = package
                .types
                .interface_of(source)
                .expect("interface adapter");
            calls.push((inner, index));
        }
    }
}

fn references(package: &hir::Package, function: &hir::Function) -> References {
    let mut found = References::default();
    found
        .types
        .extend(function.locals.iter().map(|local| local.ty));
    block(&function.body, &mut found);
    for &ty in &found.types {
        found.functions.extend(package.drops_within(ty));
    }
    found
}

fn block(block: &hir::Block, found: &mut References) {
    for stmt in &block.stmts {
        statement(stmt, found);
    }
}

fn place(place: &hir::Place, found: &mut References) {
    for projection in &place.projections {
        if let hir::Projection::Index(index) = projection {
            expr(index, found);
        }
    }
}

fn statement(stmt: &hir::Stmt, found: &mut References) {
    match &stmt.kind {
        StmtKind::Let { value, .. } | StmtKind::Expr(value) | StmtKind::SetGlobal { value, .. } => {
            expr(value, found)
        }
        StmtKind::Assign {
            targets, values, ..
        } => {
            for target in targets.iter().flatten() {
                place(target, found);
            }
            for value in values {
                expr(value, found);
            }
        }
        StmtKind::MapAssign { map, key, value } => {
            place(map, found);
            expr(key, found);
            expr(value, found);
        }
        StmtKind::CompoundAssign {
            place: target,
            value,
            ..
        } => {
            place(target, found);
            expr(value, found);
        }
        StmtKind::Return(values) => {
            for value in values {
                expr(value, found);
            }
        }
        StmtKind::Break | StmtKind::Continue => {}
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            expr(condition, found);
            block(then_block, found);
            if let Some(else_block) = else_block {
                block(else_block, found);
            }
        }
        StmtKind::Loop {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                statement(init, found);
            }
            if let Some(condition) = condition {
                expr(condition, found);
            }
            if let Some(update) = update {
                statement(update, found);
            }
            block(body, found);
        }
        StmtKind::ForEach {
            collection, body, ..
        } => {
            expr(collection, found);
            block(body, found);
        }
        StmtKind::Select { arms, default } => {
            for arm in arms {
                match &arm.comm {
                    SelectComm::Receive { channel, .. } => expr(channel, found),
                    SelectComm::Send { channel, value } => {
                        expr(channel, found);
                        expr(value, found);
                    }
                }
                block(&arm.body, found);
            }
            if let Some(default) = default {
                block(default, found);
            }
        }
        StmtKind::Block(inner) => block(inner, found),
    }
}

fn expr(expr_: &hir::Expr, found: &mut References) {
    found.types.extend(expr_.types.iter().copied());
    match &expr_.kind {
        ExprKind::InterfaceBox(source) | ExprKind::InterfaceView { source, .. } => {
            found.conversions.insert((source.ty(), expr_.ty()));
        }
        ExprKind::InterfaceCall {
            receiver, method, ..
        } => {
            found.interfaces.insert((receiver.ty(), *method));
        }
        ExprKind::Global(global) => {
            found.globals.insert(*global);
        }
        ExprKind::Call { function, .. } | ExprKind::Closure { function, .. } => {
            found.functions.insert(*function);
        }
        ExprKind::Spawn { thunk, .. } => {
            found.functions.insert(*thunk);
        }
        _ => {}
    }
    for child in super::subexpressions(expr_) {
        expr(child, found);
    }
}
