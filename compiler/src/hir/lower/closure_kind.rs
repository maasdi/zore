use std::collections::HashSet;

use super::{Checker, subexpressions};
use crate::ast::ParamMode;
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, ExprKind, StmtKind};
use crate::resolve::{FunctionId, LocalId, LocalKind};

#[derive(Default)]
struct Escapes {
    locals: HashSet<LocalId>,
    /// Locals moved into a task as the callable of a `go`.
    spawned: HashSet<LocalId>,
    /// `(to, from)`: `to` was bound or assigned from the whole local `from`.
    aliases: Vec<(LocalId, LocalId)>,
}

impl Checker<'_> {
    /// Makes escaping closure literals owning and checks how call-once closures are used.
    pub(super) fn infer_closure_kinds(&mut self, body: &mut hir::Block) {
        let mut escapes = Escapes::default();
        self.block_escapes(body, &mut escapes);
        loop {
            let before = escapes.locals.len() + escapes.spawned.len();
            for &(to, from) in &escapes.aliases {
                if escapes.locals.contains(&to) {
                    escapes.locals.insert(from);
                }
                if escapes.spawned.contains(&to) {
                    escapes.spawned.insert(from);
                }
            }
            if escapes.locals.len() + escapes.spawned.len() == before {
                break;
            }
        }
        let mut once_locals = HashSet::new();
        self.bind_closures(body, &escapes.locals, &escapes.spawned, &mut once_locals);
        if !once_locals.is_empty() {
            self.block_once_uses(body, &once_locals);
        }
    }

    /// Whether the body moves a captured value out, which makes the closure call-once.
    pub(super) fn consumes_capture(&self, body: &hir::Block, captures: &[LocalId]) -> bool {
        let mut moved = HashSet::new();
        self.block_moves(body, &mut moved);
        captures.iter().any(|capture| moved.contains(capture))
    }

    fn param_mode(&self, callee: &ExprKind, index: usize) -> ParamMode {
        match callee {
            ExprKind::Call { function, .. } | ExprKind::CallGeneric { function, .. } => {
                let func = self.res.functions[function.0 as usize];
                func.receiver
                    .iter()
                    .chain(&func.params)
                    .nth(index)
                    .map_or(ParamMode::Borrow, |param| param.mode)
            }
            ExprKind::CallValue { callee, .. } => self
                .types
                .func_signature(callee.ty())
                .and_then(|signature| signature.params.get(index))
                .map_or(ParamMode::Borrow, |&(mode, _)| mode),
            ExprKind::InterfaceCall {
                receiver, method, ..
            } => self
                .types
                .interface_of(receiver.ty())
                .and_then(|id| self.types.interface_methods(id).get(*method))
                .and_then(|entry| entry.params.get(index))
                .map_or(ParamMode::Borrow, |&(mode, _)| mode),
            _ => ParamMode::Borrow,
        }
    }

    fn receiver_mode(&self, receiver: &hir::Expr, method: usize) -> ParamMode {
        self.types
            .interface_of(receiver.ty())
            .and_then(|id| self.types.interface_methods(id).get(method))
            .map_or(ParamMode::Borrow, |entry| entry.receiver)
    }

    fn is_call_once(&self, function: FunctionId) -> bool {
        let index = function.0 as usize - self.res.functions.len();
        match self.closures.get(index) {
            Some(closure) => closure.as_ref().is_some_and(|closure| closure.call_once),
            None => self
                .generated_functions
                .get(index - self.closures.len())
                .is_some_and(|generated| generated.call_once),
        }
    }

    fn block_escapes(&self, block: &mut hir::Block, escapes: &mut Escapes) {
        for stmt in &mut block.stmts {
            self.stmt_escapes(stmt, escapes);
        }
    }

    fn stmt_escapes(&self, stmt: &mut hir::Stmt, escapes: &mut Escapes) {
        match &mut stmt.kind {
            StmtKind::Let { targets, value } => {
                if let ([Some(to)], ExprKind::Local(from)) = (&targets[..], &value.kind) {
                    escapes.aliases.push((*to, *from));
                }
                self.expr_escapes(value, false, escapes);
            }
            StmtKind::Assign { targets, values } => {
                for (target, value) in targets.iter().zip(values.iter_mut()) {
                    let into_storage = target
                        .as_ref()
                        .is_some_and(|place| !place.projections.is_empty());
                    if let (Some(place), ExprKind::Local(from)) = (target, &value.kind)
                        && place.projections.is_empty()
                    {
                        escapes.aliases.push((place.root, *from));
                    }
                    self.expr_escapes(value, into_storage, escapes);
                }
                for value in values.iter_mut().skip(targets.len()) {
                    self.expr_escapes(value, false, escapes);
                }
            }
            StmtKind::MapAssign { key, value, .. } => {
                self.expr_escapes(key, false, escapes);
                self.expr_escapes(value, true, escapes);
            }
            StmtKind::CompoundAssign { value, .. }
            | StmtKind::Expr(value)
            | StmtKind::SetGlobal { value, .. } => self.expr_escapes(value, false, escapes),
            StmtKind::Return(values) => {
                for value in values {
                    self.expr_escapes(value, true, escapes);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr_escapes(condition, false, escapes);
                self.block_escapes(then_block, escapes);
                if let Some(else_block) = else_block {
                    self.block_escapes(else_block, escapes);
                }
            }
            StmtKind::Loop {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt_escapes(init, escapes);
                }
                if let Some(condition) = condition {
                    self.expr_escapes(condition, false, escapes);
                }
                if let Some(update) = update {
                    self.stmt_escapes(update, escapes);
                }
                self.block_escapes(body, escapes);
            }
            StmtKind::ForEach {
                collection, body, ..
            } => {
                self.expr_escapes(collection, false, escapes);
                self.block_escapes(body, escapes);
            }
            StmtKind::Select { arms, default } => {
                for arm in arms {
                    match &mut arm.comm {
                        hir::SelectComm::Receive { channel, .. } => {
                            self.expr_escapes(channel, false, escapes)
                        }
                        hir::SelectComm::Send { channel, value } => {
                            self.expr_escapes(channel, false, escapes);
                            self.expr_escapes(value, true, escapes);
                        }
                    }
                    self.block_escapes(&mut arm.body, escapes);
                }
                if let Some(default) = default {
                    self.block_escapes(default, escapes);
                }
            }
            StmtKind::Block(block) => self.block_escapes(block, escapes),
        }
    }

    /// A value in an escaping position outlives the expression that produced it.
    fn expr_escapes(&self, expr: &mut hir::Expr, escaping: bool, escapes: &mut Escapes) {
        if escaping {
            match &mut expr.kind {
                ExprKind::Local(local) => {
                    escapes.locals.insert(*local);
                }
                ExprKind::Closure { owning, .. } => *owning = true,
                _ => {}
            }
        }
        let modes: Vec<ParamMode> = match &expr.kind {
            ExprKind::Call { args, .. }
            | ExprKind::CallGeneric { args, .. }
            | ExprKind::CallValue { args, .. }
            | ExprKind::InterfaceCall { args, .. } => (0..args.len())
                .map(|index| self.param_mode(&expr.kind, index))
                .collect(),
            _ => Vec::new(),
        };
        match &mut expr.kind {
            ExprKind::StructLit { fields, .. } => {
                for (_, value) in fields {
                    self.expr_escapes(value, true, escapes);
                }
            }
            ExprKind::ArrayLit { elements, .. } => {
                for element in elements {
                    self.expr_escapes(element, true, escapes);
                }
            }
            ExprKind::MapLit { entries, .. } => {
                for (key, value) in entries {
                    self.expr_escapes(key, false, escapes);
                    self.expr_escapes(value, true, escapes);
                }
            }
            ExprKind::ArrayPush { array, value } => {
                self.expr_escapes(array, false, escapes);
                self.expr_escapes(value, true, escapes);
            }
            ExprKind::Call { args, .. } | ExprKind::CallGeneric { args, .. } => {
                for (arg, mode) in args.iter_mut().zip(modes) {
                    self.expr_escapes(arg, mode == ParamMode::Own, escapes);
                }
            }
            ExprKind::CallValue { callee, args, .. } => {
                self.expr_escapes(callee, false, escapes);
                for (arg, mode) in args.iter_mut().zip(modes) {
                    self.expr_escapes(arg, mode == ParamMode::Own, escapes);
                }
            }
            ExprKind::InterfaceCall { receiver, args, .. } => {
                self.expr_escapes(receiver, false, escapes);
                for (arg, mode) in args.iter_mut().zip(modes) {
                    self.expr_escapes(arg, mode == ParamMode::Own, escapes);
                }
            }
            ExprKind::InterfaceBox(source) => self.expr_escapes(source, true, escapes),
            ExprKind::Spawn { args, callable, .. } => {
                if *callable && let Some(ExprKind::Local(local)) = args.first().map(|arg| &arg.kind)
                {
                    escapes.spawned.insert(*local);
                }
                for arg in args {
                    self.expr_escapes(arg, true, escapes);
                }
            }
            _ => {
                for child in children_mut(expr) {
                    self.expr_escapes(child, false, escapes);
                }
            }
        }
    }

    fn bind_closures(
        &mut self,
        block: &mut hir::Block,
        escaping: &HashSet<LocalId>,
        spawned: &HashSet<LocalId>,
        once_locals: &mut HashSet<LocalId>,
    ) {
        for stmt in &mut block.stmts {
            let bound = match &stmt.kind {
                StmtKind::Let { targets, .. } => match &targets[..] {
                    [Some(local)] => Some((*local, self.binding_kind(*local) == LocalKind::Let)),
                    _ => None,
                },
                _ => None,
            };
            if let StmtKind::Let { value, .. } = &mut stmt.kind
                && let ExprKind::Closure {
                    function,
                    captures,
                    owning,
                } = &mut value.kind
                && let Some((local, is_let)) = bound
            {
                if escaping.contains(&local) {
                    *owning = true;
                }
                if spawned.contains(&local) {
                    self.check_spawned_captures(*function, captures, value.span);
                }
                if self.is_call_once(*function) && is_let {
                    *owning = true;
                    once_locals.insert(local);
                    continue;
                }
            }
            if let StmtKind::Assign { targets, values } = &mut stmt.kind {
                for (target, value) in targets.iter().zip(values.iter_mut()) {
                    if let (Some(place), ExprKind::Closure { owning, .. }) =
                        (target, &mut value.kind)
                        && place.projections.is_empty()
                        && escaping.contains(&place.root)
                    {
                        *owning = true;
                    }
                }
            }
            self.stmt_closures(stmt, escaping, spawned, once_locals);
        }
    }

    /// Reports call-once literals outside a `let` binding and recurses into nested blocks.
    fn stmt_closures(
        &mut self,
        stmt: &mut hir::Stmt,
        escaping: &HashSet<LocalId>,
        spawned: &HashSet<LocalId>,
        once_locals: &mut HashSet<LocalId>,
    ) {
        let mut misplaced = Vec::new();
        let mut spawned_literals = Vec::new();
        let mut spawn_callees = HashSet::new();
        let call_once: Vec<bool> = self
            .closures
            .iter()
            .map(|closure| closure.as_ref().is_some_and(|closure| closure.call_once))
            .chain(
                self.generated_functions
                    .iter()
                    .map(|generated| generated.call_once),
            )
            .collect();
        let declared = self.res.functions.len();
        for root in stmt_exprs(stmt) {
            visit_expr(root, &mut |expr| {
                if let ExprKind::Spawn {
                    args,
                    callable: true,
                    ..
                } = &expr.kind
                    && let Some(first) = args.first()
                    && let ExprKind::Closure {
                        function, captures, ..
                    } = &first.kind
                {
                    spawn_callees.insert(first.span);
                    spawned_literals.push((*function, captures.clone(), first.span));
                }
                if let ExprKind::Closure {
                    function, owning, ..
                } = &mut expr.kind
                    && call_once
                        .get(function.0 as usize - declared)
                        .copied()
                        .unwrap_or(false)
                    && !spawn_callees.contains(&expr.span)
                {
                    *owning = true;
                    misplaced.push(expr.span);
                }
            });
        }
        for (function, captures, span) in spawned_literals {
            self.check_spawned_captures(function, &captures, span);
        }
        for span in misplaced {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "a call-once function literal must initialize a `let` binding",
                    span,
                )
                .note("it consumes a captured value, so it can only be bound with `let name = func...` and called directly"),
            );
        }
        match &mut stmt.kind {
            StmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                self.bind_closures(then_block, escaping, spawned, once_locals);
                if let Some(else_block) = else_block {
                    self.bind_closures(else_block, escaping, spawned, once_locals);
                }
            }
            StmtKind::Loop {
                init, update, body, ..
            } => {
                if let Some(init) = init {
                    self.stmt_closures(init, escaping, spawned, once_locals);
                }
                if let Some(update) = update {
                    self.stmt_closures(update, escaping, spawned, once_locals);
                }
                self.bind_closures(body, escaping, spawned, once_locals);
            }
            StmtKind::ForEach { body, .. } => {
                self.bind_closures(body, escaping, spawned, once_locals)
            }
            StmtKind::Select { arms, default } => {
                for arm in arms {
                    self.bind_closures(&mut arm.body, escaping, spawned, once_locals);
                }
                if let Some(default) = default {
                    self.bind_closures(default, escaping, spawned, once_locals);
                }
            }
            StmtKind::Block(block) => self.bind_closures(block, escaping, spawned, once_locals),
            _ => {}
        }
    }

    fn block_once_uses(&mut self, block: &mut hir::Block, once_locals: &HashSet<LocalId>) {
        let mut misused = Vec::new();
        once_uses_in_block(block, once_locals, &mut misused);
        for (local, span) in misused {
            let name = self.res.locals[self.current][local.0 as usize].name.clone();
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("call-once closure `{name}` can only be called directly"),
                    span,
                )
                .note("it consumes a captured value, so it cannot be passed, returned, stored, rebound, or captured"),
            );
        }
    }

    fn block_moves(&self, block: &hir::Block, moved: &mut HashSet<LocalId>) {
        for stmt in &block.stmts {
            self.stmt_moves(stmt, moved);
        }
    }

    fn stmt_moves(&self, stmt: &hir::Stmt, moved: &mut HashSet<LocalId>) {
        match &stmt.kind {
            StmtKind::Let { value, .. }
            | StmtKind::CompoundAssign { value, .. }
            | StmtKind::Expr(value)
            | StmtKind::SetGlobal { value, .. } => self.value_moves(value, moved),
            StmtKind::Assign { targets, values } => {
                for target in targets.iter().flatten() {
                    for projection in &target.projections {
                        if let hir::Projection::Index(index) = projection {
                            self.value_moves(index, moved);
                        }
                    }
                }
                for value in values {
                    self.value_moves(value, moved);
                }
            }
            StmtKind::MapAssign { key, value, .. } => {
                self.value_moves(key, moved);
                self.value_moves(value, moved);
            }
            StmtKind::Return(values) => {
                for value in values {
                    self.value_moves(value, moved);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.value_moves(condition, moved);
                self.block_moves(then_block, moved);
                if let Some(else_block) = else_block {
                    self.block_moves(else_block, moved);
                }
            }
            StmtKind::Loop {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt_moves(init, moved);
                }
                if let Some(condition) = condition {
                    self.value_moves(condition, moved);
                }
                if let Some(update) = update {
                    self.stmt_moves(update, moved);
                }
                self.block_moves(body, moved);
            }
            StmtKind::ForEach {
                collection, body, ..
            } => {
                self.place_moves(collection, moved);
                self.block_moves(body, moved);
            }
            StmtKind::Select { arms, default } => {
                for arm in arms {
                    match &arm.comm {
                        hir::SelectComm::Receive { channel, .. } => {
                            self.place_moves(channel, moved)
                        }
                        hir::SelectComm::Send { channel, value } => {
                            self.place_moves(channel, moved);
                            self.value_moves(value, moved);
                        }
                    }
                    self.block_moves(&arm.body, moved);
                }
                if let Some(default) = default {
                    self.block_moves(default, moved);
                }
            }
            StmtKind::Block(block) => self.block_moves(block, moved),
        }
    }

    /// Mirrors MIR lowering: a whole non-Copy place in a value position is moved.
    fn value_moves(&self, expr: &hir::Expr, moved: &mut HashSet<LocalId>) {
        if expr.types.len() == 1
            && !self.type_is_copy(expr.ty())
            && let Some(root) = place_root(expr)
        {
            moved.insert(root);
            return;
        }
        match &expr.kind {
            ExprKind::Call { args, .. }
            | ExprKind::CallGeneric { args, .. }
            | ExprKind::CallValue { args, .. } => {
                if let ExprKind::CallValue { callee, .. } = &expr.kind {
                    self.place_moves(callee, moved);
                }
                for (index, arg) in args.iter().enumerate() {
                    if self.param_mode(&expr.kind, index) == ParamMode::Own {
                        self.value_moves(arg, moved);
                    } else {
                        self.place_moves(arg, moved);
                    }
                }
            }
            ExprKind::InterfaceCall {
                receiver,
                method,
                args,
            } => {
                if self.receiver_mode(receiver, *method) == ParamMode::Own {
                    self.value_moves(receiver, moved);
                } else {
                    self.place_moves(receiver, moved);
                }
                for (index, arg) in args.iter().enumerate() {
                    if self.param_mode(&expr.kind, index) == ParamMode::Own {
                        self.value_moves(arg, moved);
                    } else {
                        self.place_moves(arg, moved);
                    }
                }
            }
            ExprKind::InterfaceView { source, .. } => self.place_moves(source, moved),
            ExprKind::Closure {
                captures, owning, ..
            } => {
                if *owning {
                    for &(local, _) in captures {
                        if self.locals[local.0 as usize].is_some_and(|ty| !self.type_is_copy(ty)) {
                            moved.insert(local);
                        }
                    }
                }
            }
            ExprKind::Clone(inner) | ExprKind::Len(inner) | ExprKind::ArrayPop(inner) => {
                self.place_moves(inner, moved)
            }
            ExprKind::MutexWithLock { mutex, callback } => {
                self.place_moves(mutex, moved);
                self.place_moves(callback, moved);
            }
            ExprKind::MutexIsPoisoned(inner) => self.place_moves(inner, moved),
            ExprKind::MapLookup { map, key } | ExprKind::MapRemove { map, key } => {
                self.place_moves(map, moved);
                self.value_moves(key, moved);
            }
            ExprKind::ArrayPush { array, value } => {
                self.place_moves(array, moved);
                self.value_moves(value, moved);
            }
            ExprKind::Slice {
                base, low, high, ..
            } => {
                self.place_moves(base, moved);
                for bound in [low, high].into_iter().flatten() {
                    self.value_moves(bound, moved);
                }
            }
            ExprKind::Index { .. } | ExprKind::Field { .. } => self.place_moves(expr, moved),
            _ => {
                for child in subexpressions(expr) {
                    self.value_moves(child, moved);
                }
            }
        }
    }

    fn place_moves(&self, expr: &hir::Expr, moved: &mut HashSet<LocalId>) {
        match &expr.kind {
            ExprKind::Local(_) => {}
            ExprKind::Field { base, .. } => self.place_moves(base, moved),
            ExprKind::Index { base, index } => {
                self.place_moves(base, moved);
                self.value_moves(index, moved);
            }
            _ => self.value_moves(expr, moved),
        }
    }
}

fn place_root(expr: &hir::Expr) -> Option<LocalId> {
    match &expr.kind {
        ExprKind::Local(local) => Some(*local),
        ExprKind::Field { base, .. } => place_root(base),
        _ => None,
    }
}

type Misuse = (LocalId, crate::source::Span);

fn once_uses_in_block(block: &mut hir::Block, once: &HashSet<LocalId>, misused: &mut Vec<Misuse>) {
    for stmt in &mut block.stmts {
        if let StmtKind::Let { targets, value } = &stmt.kind
            && let [Some(local)] = &targets[..]
            && once.contains(local)
            && matches!(value.kind, ExprKind::Closure { .. })
        {
            continue;
        }
        misused.extend(
            stmt_place_roots(stmt)
                .into_iter()
                .filter(|(root, _)| once.contains(root)),
        );
        for expr in stmt_exprs(stmt) {
            once_uses(expr, once, misused);
        }
        match &mut stmt.kind {
            StmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                once_uses_in_block(then_block, once, misused);
                if let Some(else_block) = else_block {
                    once_uses_in_block(else_block, once, misused);
                }
            }
            StmtKind::Loop {
                init, update, body, ..
            } => {
                for nested in [init, update].into_iter().flatten() {
                    for expr in stmt_exprs(nested) {
                        once_uses(expr, once, misused);
                    }
                }
                once_uses_in_block(body, once, misused);
            }
            StmtKind::ForEach { body, .. } | StmtKind::Block(body) => {
                once_uses_in_block(body, once, misused)
            }
            StmtKind::Select { arms, default } => {
                for arm in arms {
                    once_uses_in_block(&mut arm.body, once, misused);
                }
                if let Some(default) = default {
                    once_uses_in_block(default, once, misused);
                }
            }
            _ => {}
        }
    }
}

/// Marks direct calls of a call-once local as consuming it; any other use is a misuse.
fn once_uses(expr: &mut hir::Expr, once: &HashSet<LocalId>, misused: &mut Vec<Misuse>) {
    match &mut expr.kind {
        ExprKind::CallValue {
            callee,
            args,
            once: consumes,
        } if matches!(callee.kind, ExprKind::Local(local) if once.contains(&local)) => {
            *consumes = true;
            for arg in args {
                once_uses(arg, once, misused);
            }
            return;
        }
        ExprKind::Spawn {
            args,
            callable: true,
            ..
        } => {
            let consumed = matches!(
                args.first().map(|callee| &callee.kind),
                Some(ExprKind::Local(local)) if once.contains(local)
            );
            for arg in args.iter_mut().skip(usize::from(consumed)) {
                once_uses(arg, once, misused);
            }
            return;
        }
        ExprKind::Local(local) if once.contains(local) => misused.push((*local, expr.span)),
        ExprKind::Closure { captures, .. } => {
            for (local, _) in captures.iter() {
                if once.contains(local) {
                    misused.push((*local, expr.span));
                }
            }
        }
        _ => {}
    }
    for child in children_mut(expr) {
        once_uses(child, once, misused);
    }
}

fn stmt_place_roots(stmt: &hir::Stmt) -> Vec<(LocalId, crate::source::Span)> {
    match &stmt.kind {
        StmtKind::Assign { targets, .. } => targets
            .iter()
            .flatten()
            .map(|place| (place.root, place.span))
            .collect(),
        StmtKind::MapAssign { map: place, .. } | StmtKind::CompoundAssign { place, .. } => {
            vec![(place.root, place.span)]
        }
        _ => Vec::new(),
    }
}

/// The expressions a statement evaluates directly, not those in nested blocks.
pub(super) fn stmt_exprs(stmt: &mut hir::Stmt) -> Vec<&mut hir::Expr> {
    let mut roots: Vec<&mut hir::Expr> = Vec::new();
    match &mut stmt.kind {
        StmtKind::Let { value, .. }
        | StmtKind::CompoundAssign { value, .. }
        | StmtKind::Expr(value)
        | StmtKind::SetGlobal { value, .. } => roots.push(value),
        StmtKind::Assign { targets, values } => {
            for target in targets.iter_mut().flatten() {
                for projection in &mut target.projections {
                    if let hir::Projection::Index(index) = projection {
                        roots.push(index);
                    }
                }
            }
            roots.extend(values.iter_mut());
        }
        StmtKind::MapAssign { key, value, .. } => {
            roots.push(key);
            roots.push(value);
        }
        StmtKind::Return(values) => roots.extend(values.iter_mut()),
        StmtKind::If { condition, .. } => roots.push(condition),
        StmtKind::Loop { condition, .. } => roots.extend(condition.as_mut()),
        StmtKind::ForEach { collection, .. } => roots.push(collection),
        StmtKind::Select { arms, .. } => {
            for arm in arms {
                match &mut arm.comm {
                    hir::SelectComm::Receive { channel, .. } => roots.push(channel),
                    hir::SelectComm::Send { channel, value } => {
                        roots.push(channel);
                        roots.push(value);
                    }
                }
            }
        }
        StmtKind::Break | StmtKind::Continue | StmtKind::Block(_) => {}
    }
    roots
}

pub(super) fn visit_expr(expr: &mut hir::Expr, visit: &mut dyn FnMut(&mut hir::Expr)) {
    visit(expr);
    for child in children_mut(expr) {
        visit_expr(child, visit);
    }
}

fn children_mut(expr: &mut hir::Expr) -> Vec<&mut hir::Expr> {
    match &mut expr.kind {
        ExprKind::Const(_)
        | ExprKind::Local(_)
        | ExprKind::Global(_)
        | ExprKind::Closure { .. } => Vec::new(),
        ExprKind::Field { base, .. } => vec![base],
        ExprKind::Index { base, index } => vec![base, index],
        ExprKind::Slice {
            base, low, high, ..
        } => std::iter::once(&mut **base)
            .chain(low.as_deref_mut())
            .chain(high.as_deref_mut())
            .collect(),
        ExprKind::Call { args, .. }
        | ExprKind::CallGeneric { args, .. }
        | ExprKind::Spawn { args, .. } => args.iter_mut().collect(),
        ExprKind::CallValue { callee, args, .. } => {
            std::iter::once(&mut **callee).chain(args).collect()
        }
        ExprKind::InterfaceCall { receiver, args, .. } => {
            std::iter::once(&mut **receiver).chain(args).collect()
        }
        ExprKind::InterfaceView { source: inner, .. } | ExprKind::InterfaceBox(inner) => {
            vec![inner]
        }
        ExprKind::StructLit { fields, .. } => fields.iter_mut().map(|(_, value)| value).collect(),
        ExprKind::ArrayLit { elements, .. } => elements.iter_mut().collect(),
        ExprKind::MapLit { entries, .. } => entries
            .iter_mut()
            .flat_map(|(key, value)| [key, value])
            .collect(),
        ExprKind::MapLookup { map, key } | ExprKind::MapRemove { map, key } => vec![map, key],
        ExprKind::ArrayPush { array, value } => vec![array, value],
        ExprKind::ChannelSend { channel, value } => vec![channel, value],
        ExprKind::MutexWithLock { mutex, callback } => vec![mutex, callback],
        ExprKind::MakeMutex(inner) | ExprKind::MutexIsPoisoned(inner) => vec![inner],
        ExprKind::MakeChannel { capacity, .. } => capacity.as_deref_mut().into_iter().collect(),
        ExprKind::ChannelReceive(inner) | ExprKind::ChannelClose(inner) => vec![inner],
        ExprKind::Len(inner)
        | ExprKind::ArrayPop(inner)
        | ExprKind::Println(inner)
        | ExprKind::Panic(inner)
        | ExprKind::Drop(inner)
        | ExprKind::Convert(inner)
        | ExprKind::Clone(inner)
        | ExprKind::Error(inner)
        | ExprKind::Try(inner)
        | ExprKind::TaskWait(inner)
        | ExprKind::Unary { operand: inner, .. } => vec![inner],
        ExprKind::Binary { lhs, rhs, .. } => vec![lhs, rhs],
    }
}
