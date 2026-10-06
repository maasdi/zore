use std::collections::{HashMap, HashSet};

use crate::ast::{
    self, AssignOp, AssignTarget, BinaryOp, BindingKind, BindingTarget, ForHeader, UnaryOp,
};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, Const, ExprKind};
use crate::resolve::{ConstId, FieldId, FunctionId, LocalId, LocalKind, Res, Resolution};
use crate::source::Span;
use crate::types::bignum::BigInt;
use crate::types::constant::{self, ConstError, Folded, Unrepresentable, Untyped};
use crate::types::{FuncSignature, IntType, StructId, TypeId, TypeKind, TypeStore};

/// HIR is returned only when there are no diagnostics.
pub fn check(
    file: &ast::File,
    mut resolution: Resolution<'_>,
    text: &str,
) -> (Option<hir::Package>, Vec<Diagnostic>) {
    let diagnostics = std::mem::take(&mut resolution.diagnostics);
    let types = std::mem::take(&mut resolution.types);
    let mut checker = Checker {
        consts: resolution
            .consts
            .iter()
            .map(|_| ConstState::Pending)
            .collect(),
        res: resolution,
        text,
        types,
        fields: Vec::new(),
        signatures: Vec::new(),
        diagnostics,
        current: 0,
        locals: Vec::new(),
        results: Vec::new(),
        loop_depth: 0,
        closures: Vec::new(),
        exclusive_captures: HashSet::new(),
        resolving_fields: false,
    };
    checker.closures = checker.res.closures.iter().map(|_| None).collect();
    checker.signatures_and_fields();
    let mut functions = Vec::new();
    for index in 0..checker.res.functions.len() {
        functions.push(checker.function(FunctionId(index as u32)));
    }
    for index in 0..checker.res.consts.len() {
        checker.eval_const(ConstId(index as u32));
    }
    let entry = checker.check_entry_point(file);
    if checker.diagnostics.is_empty()
        && let Some(index) = checker.closures.iter().position(Option::is_none)
    {
        let span = checker.res.closures[index].span;
        checker.error(
            "internal error: this function literal was not checked",
            span,
        );
    }
    if !checker.diagnostics.is_empty() {
        return (None, checker.diagnostics);
    }
    let structs = checker
        .res
        .structs
        .iter()
        .zip(&checker.fields)
        .enumerate()
        .map(|(index, (decl, fields))| hir::Struct {
            name: decl.name.text.clone(),
            span: decl.name.span,
            drop: checker
                .res
                .methods
                .get(&(StructId(index as u32), "drop".to_string()))
                .copied(),
            clone: checker
                .res
                .methods
                .get(&(StructId(index as u32), "clone".to_string()))
                .copied(),
            fields: fields
                .iter()
                .map(|(name, ty, span)| hir::Field {
                    name: name.clone(),
                    ty: ty.expect("no diagnostics means every field type resolved"),
                    span: *span,
                })
                .collect(),
        })
        .collect();
    let closures = std::mem::take(&mut checker.closures);
    let package = hir::Package {
        name: checker.res.package.clone(),
        types: std::mem::take(&mut checker.types),
        structs,
        functions: functions
            .into_iter()
            .chain(closures)
            .map(|f| f.expect("every function and closure was checked"))
            .collect(),
        entry,
    };
    (Some(package), checker.diagnostics)
}

#[derive(Clone)]
enum ConstValue {
    Untyped(Untyped),
    Typed(TypeId, Const),
}

enum ConstState {
    Pending,
    InProgress,
    Done(Option<ConstValue>),
}

enum Value {
    Untyped(Untyped, Span),
    Typed(hir::Expr),
}

impl Value {
    fn span(&self) -> Span {
        match self {
            Self::Untyped(_, span) => *span,
            Self::Typed(expr) => expr.span,
        }
    }
}

struct Signature {
    params: Vec<TypeId>,
    results: Vec<TypeId>,
}

struct Checker<'a> {
    res: Resolution<'a>,
    text: &'a str,
    types: TypeStore,
    fields: Vec<Vec<(String, Option<TypeId>, Span)>>,
    signatures: Vec<Option<Signature>>,
    consts: Vec<ConstState>,
    diagnostics: Vec<Diagnostic>,
    current: usize,
    locals: Vec<Option<TypeId>>,
    results: Vec<TypeId>,
    loop_depth: usize,
    /// Indexed like `res.closures`.
    closures: Vec<Option<hir::Function>>,
    exclusive_captures: HashSet<(usize, LocalId)>,
    resolving_fields: bool,
}

struct BodyState {
    current: usize,
    locals: Vec<Option<TypeId>>,
    results: Vec<TypeId>,
    loop_depth: usize,
}

fn typed(kind: ExprKind, ty: TypeId, span: Span) -> hir::Expr {
    hir::Expr {
        kind,
        types: vec![ty],
        span,
    }
}

fn constant(expr: &hir::Expr) -> Option<&Const> {
    match &expr.kind {
        ExprKind::Const(value) => Some(value),
        _ => None,
    }
}

fn is_nil(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Nil => true,
        ast::ExprKind::Paren(inner) => is_nil(inner),
        _ => false,
    }
}

fn is_try(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Try(_) => true,
        ast::ExprKind::Paren(inner) => is_try(inner),
        _ => false,
    }
}

fn op_str(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::BitAnd => "&",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::LtEq => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::GtEq => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

impl<'a> Checker<'a> {
    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::new(Severity::Error, message, span));
    }

    fn unsupported(&mut self, what: &str, span: Span, note: &str) {
        self.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("{what} not supported by the checker yet"),
                span,
            )
            .note(note.to_owned()),
        );
    }

    fn name(&self, ty: TypeId) -> String {
        self.types.display(ty).to_string()
    }

    fn mismatch(&mut self, expected: TypeId, found: TypeId, span: Span) {
        let message = format!(
            "mismatched types: expected `{}`, found `{}`",
            self.name(expected),
            self.name(found)
        );
        let mut diagnostic = Diagnostic::new(Severity::Error, message, span);
        if let (
            TypeKind::Slice { element, .. },
            TypeKind::Array { element: found, .. } | TypeKind::DynArray { element: found },
        ) = (self.types.kind(expected), self.types.kind(found))
            && element == found
        {
            diagnostic = diagnostic
                .note("arrays do not convert to slices implicitly; borrow a view with `x[:]`");
        }
        self.diagnostics.push(diagnostic);
    }

    fn resolve_type(&mut self, ty: &ast::Type) -> Option<TypeId> {
        match ty {
            ast::Type::Named(name) => match self.res.uses.get(&name.span)? {
                Res::Primitive(ty) => Some(*ty),
                Res::Struct(id) => Some(self.types.struct_type(*id)),
                _ => None,
            },
            ast::Type::Array { element, size, .. } => {
                if !self.reject_stored_func(element) {
                    return None;
                }
                let element_ty = self.resolve_type(element)?;
                let value = self.expr(size, Some(TypeStore::INT))?;
                let sized = self.coerce(value, TypeStore::INT)?;
                let ExprKind::Const(Const::Int(n)) = sized.kind else {
                    self.error("array size must be a constant expression", size.span);
                    return None;
                };
                let Ok(count) = u32::try_from(n) else {
                    self.error(
                        "array size must be a nonnegative constant that fits in 32 bits",
                        size.span,
                    );
                    return None;
                };
                Some(self.types.array_type(element_ty, count))
            }
            ast::Type::DynArray { element, .. } => {
                if !self.reject_stored_func(element) {
                    return None;
                }
                let element_ty = self.resolve_type(element)?;
                if !self.reject_collected_mut_view(element_ty, element.span()) {
                    return None;
                }
                Some(self.types.dyn_array_type(element_ty))
            }
            ast::Type::Map { key, value, .. } => {
                let key_ty = self.resolve_type(key);
                let value_ty = if self.reject_stored_func(value) {
                    self.resolve_type(value)
                        .filter(|&ty| self.reject_collected_mut_view(ty, value.span()))
                } else {
                    None
                };
                let key_ty = key_ty?;
                if !self.is_map_key(key_ty) {
                    let message = format!(
                        "type `{}` cannot be a map key; keys are bool, integer, rune, or string",
                        self.name(key_ty)
                    );
                    self.error(message, key.span());
                    return None;
                }
                Some(self.types.map_type(key_ty, value_ty?))
            }
            ast::Type::Slice {
                element, mutable, ..
            } => {
                if !self.reject_stored_func(element) {
                    return None;
                }
                let element_ty = self.resolve_type(element)?;
                if !self.reject_collected_mut_view(element_ty, element.span()) {
                    return None;
                }
                Some(self.types.slice_type(element_ty, *mutable))
            }
            ast::Type::Func {
                params, results, ..
            } => {
                let mut ok = true;
                let mut checked_params = Vec::new();
                for param in params {
                    match self.param_mode_type(param.mode, &param.ty, param.ty.span()) {
                        Some(ty) => checked_params.push((param.mode, ty)),
                        None => ok = false,
                    }
                }
                let checked_results = self.closure_results(results);
                if !ok {
                    return None;
                }
                let results = checked_results?;
                Some(self.types.func_type(FuncSignature {
                    params: checked_params,
                    results,
                }))
            }
        }
    }

    /// Results of function types and literals cannot hold closures.
    fn closure_results(&mut self, results: &[ast::Type]) -> Option<Vec<TypeId>> {
        let mut checked = Vec::new();
        let mut ok = true;
        for result in results {
            match self.resolve_type(result) {
                Some(ty) if self.result_allowed(ty, result.span()) => checked.push(ty),
                _ => ok = false,
            }
        }
        ok.then_some(checked)
    }

    fn result_allowed(&mut self, ty: TypeId, span: Span) -> bool {
        if self.type_contains(ty, &|kind| matches!(kind, TypeKind::Func(_))) {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "a function cannot return a function value",
                    span,
                )
                .note("closures cannot escape the scope that created them"),
            );
            return false;
        }
        true
    }

    fn reject_stored_func(&mut self, ty: &ast::Type) -> bool {
        let ast::Type::Func { span, .. } = ty else {
            return true;
        };
        self.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                "function values cannot be stored in a struct field, array, slice, or map",
                *span,
            )
            .note("closures cannot escape the scope that created them"),
        );
        false
    }

    fn contains_mut_view(&self, ty: TypeId) -> bool {
        self.type_contains(ty, &|kind| {
            matches!(kind, TypeKind::Slice { mutable: true, .. })
        })
    }

    /// Struct fields are checked once all of them are resolved.
    fn reject_collected_mut_view(&mut self, element: TypeId, span: Span) -> bool {
        if self.resolving_fields || !self.contains_mut_view(element) {
            return true;
        }
        self.unsupported(
            "a `mut []T` view inside an `Array<T>`, map, or slice element is",
            span,
            "a mutable view can be a struct field, fixed-array element, parameter, result, or local",
        );
        false
    }

    fn collects_mut_view(&self, ty: TypeId) -> bool {
        let mut pending = vec![ty];
        let mut seen = Vec::new();
        while let Some(ty) = pending.pop() {
            if seen.contains(&ty) {
                continue;
            }
            seen.push(ty);
            match self.types.kind(ty) {
                TypeKind::DynArray { element: held, .. }
                | TypeKind::Slice { element: held, .. }
                | TypeKind::Map { value: held, .. } => {
                    if self.contains_mut_view(held) {
                        return true;
                    }
                    pending.push(held);
                }
                TypeKind::Array { element, .. } => pending.push(element),
                TypeKind::Struct(id) => pending.extend(
                    self.fields[id.0 as usize]
                        .iter()
                        .filter_map(|(_, field_ty, _)| *field_ty),
                ),
                _ => {}
            }
        }
        false
    }

    fn reject_collected_mut_views_in_fields(&mut self, structs: &[&ast::StructDecl]) {
        for (index, decl) in structs.iter().enumerate() {
            for (field, (_, ty, _)) in decl.fields.iter().zip(self.fields[index].clone()) {
                if ty.is_some_and(|ty| self.collects_mut_view(ty)) {
                    self.unsupported(
                        "a `mut []T` view inside an `Array<T>`, map, or slice element is",
                        field.ty.span(),
                        "a mutable view can be a struct field, fixed-array element, parameter, result, or local",
                    );
                }
            }
        }
    }

    /// Looks through fields and array elements, not slice elements.
    fn type_contains(&self, ty: TypeId, matches: &dyn Fn(TypeKind) -> bool) -> bool {
        let mut pending = vec![ty];
        let mut seen = Vec::new();
        while let Some(ty) = pending.pop() {
            if seen.contains(&ty) {
                continue;
            }
            seen.push(ty);
            let kind = self.types.kind(ty);
            if matches(kind) {
                return true;
            }
            match kind {
                TypeKind::Struct(id) => pending.extend(
                    self.fields[id.0 as usize]
                        .iter()
                        .filter_map(|(_, field_ty, _)| *field_ty),
                ),
                TypeKind::Array { element, .. } | TypeKind::DynArray { element } => {
                    pending.push(element)
                }
                TypeKind::Map { value, .. } => pending.push(value),
                _ => {}
            }
        }
        false
    }

    fn is_map_key(&self, ty: TypeId) -> bool {
        matches!(
            self.types.kind(ty),
            TypeKind::Bool | TypeKind::Int(_) | TypeKind::Rune | TypeKind::String
        )
    }

    /// Must agree with `hir::Package::is_copy`.
    fn type_is_copy(&self, ty: TypeId) -> bool {
        match self.types.kind(ty) {
            TypeKind::Bool
            | TypeKind::Int(_)
            | TypeKind::Float(_)
            | TypeKind::Rune
            | TypeKind::String
            | TypeKind::Error
            | TypeKind::Slice { .. } => true,
            TypeKind::Struct(id) => {
                !self.res.methods.contains_key(&(id, "drop".to_string()))
                    && self.fields[id.0 as usize]
                        .iter()
                        .all(|(_, field_ty, _)| field_ty.is_none_or(|ty| self.type_is_copy(ty)))
            }
            TypeKind::Array { element, .. } => self.type_is_copy(element),
            TypeKind::DynArray { .. } | TypeKind::Map { .. } | TypeKind::Func(_) => false,
        }
    }

    fn is_func(&self, ty: TypeId) -> bool {
        matches!(self.types.kind(ty), TypeKind::Func(_))
    }

    fn is_mut_slice(&self, ty: TypeId) -> bool {
        matches!(self.types.kind(ty), TypeKind::Slice { mutable: true, .. })
    }

    fn signatures_and_fields(&mut self) {
        // Cloned because `resolve_type` needs `&mut self`.
        let structs: Vec<&'a ast::StructDecl> = self.res.structs.clone();
        self.resolving_fields = true;
        self.fields = structs
            .iter()
            .map(|decl| {
                let fields = decl.fields.clone();
                fields
                    .iter()
                    .map(|f| {
                        let ty = if self.reject_stored_func(&f.ty) {
                            self.resolve_type(&f.ty)
                        } else {
                            None
                        };
                        (f.name.text.clone(), ty, f.name.span)
                    })
                    .collect()
            })
            .collect();
        self.resolving_fields = false;
        self.reject_collected_mut_views_in_fields(&structs);
        let functions: Vec<&'a ast::FuncDecl> = self.res.functions.clone();
        self.signatures = functions
            .iter()
            .map(|func| {
                let receiver_and_params: Vec<ast::Param> =
                    func.receiver.iter().chain(&func.params).cloned().collect();
                let params: Option<Vec<_>> = receiver_and_params
                    .iter()
                    .map(|p| self.param_type(p))
                    .collect();
                let results = func.results.clone();
                let results: Option<Vec<_>> = results
                    .iter()
                    .map(|t| {
                        let ty = self.resolve_type(t)?;
                        self.result_allowed(ty, t.span()).then_some(ty)
                    })
                    .collect();
                Some(Signature {
                    params: params?,
                    results: results?,
                })
            })
            .collect();
    }

    fn param_type(&mut self, param: &ast::Param) -> Option<TypeId> {
        self.param_mode_type(param.mode, &param.ty, param.span)
    }

    fn param_mode_type(
        &mut self,
        mode: ast::ParamMode,
        ty: &ast::Type,
        span: Span,
    ) -> Option<TypeId> {
        let ty = self.resolve_type(ty)?;
        if self.is_func(ty) && mode != ast::ParamMode::Borrow {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "a function-typed parameter cannot be `mut` or `own`",
                    span,
                )
                .note("a function value is passed by borrowing it exclusively for the call"),
            );
            return None;
        }
        if !matches!(self.types.kind(ty), TypeKind::Slice { .. }) {
            if mode == ast::ParamMode::Borrow && self.contains_mut_view(ty) {
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "a shared parameter of type `{}` cannot hold a `mut []T` view",
                            self.name(ty)
                        ),
                        span,
                    )
                    .note("declare it `mut` or `own`; a shared borrow of a container gives no mutable access through a view inside it"),
                );
                return None;
            }
            return Some(ty);
        }
        match mode {
            ast::ParamMode::Borrow => Some(ty),
            ast::ParamMode::Own => {
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        "`own []T` is not part of Zore; a slice never owns its elements",
                        span,
                    )
                    .note("own the elements with a fixed array instead"),
                );
                None
            }
            ast::ParamMode::Mut => {
                self.unsupported(
                    "a `mut` mode on a slice parameter is",
                    span,
                    "write `name mut []T` for mutable element access",
                );
                None
            }
        }
    }

    fn function(&mut self, id: FunctionId) -> Option<hir::Function> {
        let func = self.res.functions[id.0 as usize];
        self.current = id.0 as usize;
        self.locals = vec![None; self.res.locals[self.current].len()];
        let signature = self.signatures[self.current].as_ref();
        let has_signature = signature.is_some();
        let (params, results) = signature.map_or((Vec::new(), Vec::new()), |s| {
            (s.params.clone(), s.results.clone())
        });
        for (index, ty) in params.iter().enumerate() {
            self.locals[index] = Some(*ty);
        }
        self.results = results.clone();
        self.loop_depth = 0;
        let body = self.block(&func.body);
        if !results.is_empty() && !block_always_exits(&func.body) {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!(
                        "function `{}` can reach the end of its body without returning a value",
                        func.name.text
                    ),
                    func.name.span,
                )
                .note("every path must return the declared results"),
            );
        }
        let decls = &self.res.locals[id.0 as usize];
        let locals = decls
            .iter()
            .zip(&self.locals)
            .map(|(decl, ty)| {
                Some(hir::Local {
                    name: decl.name.clone(),
                    ty: (*ty)?,
                    kind: decl.kind,
                    span: decl.span,
                })
            })
            .collect::<Option<Vec<_>>>();
        if !has_signature {
            return None;
        }
        Some(hir::Function {
            name: self.function_name(id),
            span: func.name.span,
            params: (0..params.len()).map(|i| LocalId(i as u32)).collect(),
            results,
            locals: locals?,
            body,
            captures: Vec::new(),
            is_closure: false,
        })
    }

    /// Closures are named `enclosing$closureN`, numbered per declared function.
    fn function_name(&self, id: FunctionId) -> String {
        let declared = self.res.functions.len();
        let index = id.0 as usize;
        if index < declared {
            let func = self.res.functions[index];
            // Resolution already rejects any receiver that isn't a named struct type.
            return match func.receiver.as_ref().map(|receiver| &receiver.ty) {
                Some(ast::Type::Named(type_name)) => {
                    format!("{}.{}", type_name.text, func.name.text)
                }
                _ => func.name.text.clone(),
            };
        }
        let root = |mut id: FunctionId| {
            while id.0 as usize >= declared {
                id = self.res.closures[id.0 as usize - declared].parent;
            }
            id
        };
        let own_root = root(id);
        let ordinal = self.res.closures[..index - declared]
            .iter()
            .filter(|closure| root(closure.id) == own_root)
            .count()
            + 1;
        format!("{}$closure{ordinal}", self.function_name(own_root))
    }

    fn save_body(&mut self) -> BodyState {
        BodyState {
            current: self.current,
            locals: std::mem::take(&mut self.locals),
            results: std::mem::take(&mut self.results),
            loop_depth: self.loop_depth,
        }
    }

    fn restore_body(&mut self, state: BodyState) {
        self.current = state.current;
        self.locals = state.locals;
        self.results = state.results;
        self.loop_depth = state.loop_depth;
    }

    fn closure_parent(&self, function: usize) -> usize {
        let index = function - self.res.functions.len();
        self.res.closures[index].parent.0 as usize
    }

    fn binding_origin(&self, mut function: usize, mut local: LocalId) -> (usize, LocalId) {
        while let LocalKind::Capture(outer) = self.res.locals[function][local.0 as usize].kind {
            function = self.closure_parent(function);
            local = outer;
        }
        (function, local)
    }

    fn binding_kind(&self, local: LocalId) -> LocalKind {
        let (function, local) = self.binding_origin(self.current, local);
        self.res.locals[function][local.0 as usize].kind
    }

    /// Marks the whole capture chain behind `local` exclusive.
    fn mark_exclusive(&mut self, local: LocalId) {
        let (mut function, mut local) = (self.current, local);
        while let LocalKind::Capture(outer) = self.res.locals[function][local.0 as usize].kind {
            self.exclusive_captures.insert((function, local));
            function = self.closure_parent(function);
            local = outer;
        }
    }

    fn closure(&mut self, closure: &ast::Closure, span: Span) -> Option<Value> {
        // Resolution reports a literal it could not give a body.
        let index = self.res.closures.iter().position(|c| c.span == span)?;
        let id = self.res.closures[index].id;
        let captures = self.res.closures[index].captures.clone();
        let params: Vec<Option<TypeId>> = closure
            .params
            .iter()
            .map(|param| self.param_type(param))
            .collect();
        let results = self.closure_results(&closure.results);
        let state = self.save_body();
        self.current = id.0 as usize;
        self.locals = vec![None; self.res.locals[self.current].len()];
        for (index, ty) in params.iter().enumerate() {
            self.locals[index] = *ty;
        }
        for &(outer, local) in &captures {
            self.locals[local.0 as usize] = state.locals[outer.0 as usize];
        }
        self.results = results.clone().unwrap_or_default();
        self.loop_depth = 0;
        let body = self.block(&closure.body);
        if results.as_ref().is_some_and(|r| !r.is_empty()) && !block_always_exits(&closure.body) {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "function literal can reach the end of its body without returning a value",
                    span,
                )
                .note("every path must return the declared results"),
            );
        }
        let locals: Option<Vec<hir::Local>> = self.res.locals[self.current]
            .iter()
            .zip(&self.locals)
            .map(|(decl, ty)| {
                Some(hir::Local {
                    name: decl.name.clone(),
                    ty: (*ty)?,
                    kind: decl.kind,
                    span: decl.span,
                })
            })
            .collect();
        // Calling a captured closure or copying a captured `mut []T` needs exclusivity.
        for &(_, local) in &captures {
            if let Some(ty) = self.locals[local.0 as usize]
                && self.type_contains(ty, &|kind| {
                    matches!(
                        kind,
                        TypeKind::Func(_) | TypeKind::Slice { mutable: true, .. }
                    )
                })
            {
                self.mark_exclusive(local);
            }
        }
        let closure_function = self.current;
        self.restore_body(state);
        let params: Option<Vec<TypeId>> = params.into_iter().collect();
        let (params, results, locals) = (params?, results?, locals?);
        let signature = FuncSignature {
            params: closure
                .params
                .iter()
                .map(|param| param.mode)
                .zip(params.iter().copied())
                .collect(),
            results: results.clone(),
        };
        let ty = self.types.func_type(signature);
        self.closures[index] = Some(hir::Function {
            name: self.function_name(id),
            span,
            params: (0..params.len()).map(|i| LocalId(i as u32)).collect(),
            results,
            locals,
            body,
            captures: captures.iter().map(|&(_, local)| local).collect(),
            is_closure: true,
        });
        let captures = captures
            .iter()
            .map(|&(outer, local)| {
                let exclusive = self.exclusive_captures.contains(&(closure_function, local));
                (outer, exclusive)
            })
            .collect();
        Some(Value::Typed(typed(
            ExprKind::Closure {
                function: id,
                captures,
            },
            ty,
            span,
        )))
    }

    fn check_entry_point(&mut self, file: &ast::File) -> Option<FunctionId> {
        let package = file.package.as_ref()?;
        if package.text != "main" {
            return None;
        }
        let main = file.items.iter().find_map(|item| match item {
            ast::Item::Func(func) if func.receiver.is_none() && func.name.text == "main" => {
                Some(func)
            }
            _ => None,
        });
        let Some(main) = main else {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "package `main` has no `main` function",
                    package.span,
                )
                .note("an executable package declares `func main() { ... }`"),
            );
            return None;
        };
        let problem = if main.is_async {
            Some("the entry point `main` cannot be `async`")
        } else if !main.params.is_empty() {
            Some("the entry point `main` takes no parameters")
        } else if !main.results.is_empty() {
            Some("the entry point `main` returns no results")
        } else {
            None
        };
        if let Some(problem) = problem {
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, problem, main.name.span)
                    .note("declare it as `func main() { ... }`"),
            );
            return None;
        }
        self.res
            .functions
            .iter()
            .position(|f| std::ptr::eq(*f, main))
            .map(|index| FunctionId(index as u32))
    }

    fn eval_const(&mut self, id: ConstId) -> Option<ConstValue> {
        match &self.consts[id.0 as usize] {
            ConstState::Done(value) => return value.clone(),
            ConstState::InProgress => {
                let name = self.res.consts[id.0 as usize].name;
                let message = format!("constant `{}` is defined in terms of itself", name.text);
                self.error(message, name.span);
                self.consts[id.0 as usize] = ConstState::Done(None);
                return None;
            }
            ConstState::Pending => {}
        }
        self.consts[id.0 as usize] = ConstState::InProgress;
        let decl = &self.res.consts[id.0 as usize];
        let (value_ast, ty_ast, span) = (decl.value, decl.ty, decl.name.span);
        if is_nil(value_ast) {
            self.error("`nil` is not a constant expression", value_ast.span);
            self.consts[id.0 as usize] = ConstState::Done(None);
            return None;
        }
        let declared = ty_ast.and_then(|ty| self.resolve_type(ty));
        let value = self.expr(value_ast, declared);
        let result = match (value, ty_ast) {
            (None, _) => None,
            (Some(_), Some(_)) if declared.is_none() => None,
            (Some(value), _) => {
                let value = match declared {
                    Some(ty) => self.coerce(value, ty).map(Value::Typed),
                    None => Some(value),
                };
                match value {
                    None => None,
                    Some(Value::Untyped(value, _)) => Some(ConstValue::Untyped(value)),
                    Some(Value::Typed(expr)) => match &expr.kind {
                        ExprKind::Const(c) => Some(ConstValue::Typed(expr.ty(), c.clone())),
                        _ => {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    Severity::Error,
                                    "constant initializer is not a constant expression",
                                    expr.span,
                                )
                                .related(span, "constant declared here")
                                .note("constants use literals, other constants, operators, and numeric conversions"),
                            );
                            None
                        }
                    },
                }
            }
        };
        // A cycle may already have recorded this constant's failure.
        if matches!(self.consts[id.0 as usize], ConstState::InProgress) {
            self.consts[id.0 as usize] = ConstState::Done(result.clone());
            result
        } else {
            None
        }
    }

    fn coerce(&mut self, value: Value, target: TypeId) -> Option<hir::Expr> {
        let (value, span) = match value {
            Value::Untyped(value, span) => (value, span),
            Value::Typed(expr) => {
                let expr = self.single_value(expr)?;
                if expr.ty() == target {
                    return Some(expr);
                }
                self.mismatch(target, expr.ty(), expr.span);
                return None;
            }
        };
        let (article, kind_name) = match value {
            Untyped::Int(_) => ("an", "integer constant"),
            Untyped::Float(_) => ("a", "floating-point constant"),
        };
        let result = match self.types.kind(target) {
            TypeKind::Int(int) => match constant::to_int(&value, int) {
                Ok(v) => Ok(Const::Int(v)),
                Err(Unrepresentable::NotInteger) => Err(format!(
                    "constant `{}` is not an integer, so it cannot have type `{}`",
                    value.describe(),
                    self.name(target)
                )),
                Err(Unrepresentable::OutOfRange) => Err(format!(
                    "{kind_name} `{}` does not fit in `{}`",
                    value.describe(),
                    self.name(target)
                )),
            },
            TypeKind::Float(float) => constant::to_float(&value, float)
                .map(Const::Float)
                .ok_or_else(|| {
                    format!(
                        "{kind_name} `{}` overflows `{}`",
                        value.describe(),
                        self.name(target)
                    )
                }),
            _ => Err(format!(
                "mismatched types: expected `{}`, found {article} {kind_name}",
                self.name(target)
            )),
        };
        match result {
            Ok(c) => Some(typed(ExprKind::Const(c), target, span)),
            Err(message) => {
                self.error(message, span);
                None
            }
        }
    }

    fn with_default_type(&mut self, value: Value) -> Option<hir::Expr> {
        match value {
            Value::Untyped(Untyped::Int(_), _) => self.coerce(value, TypeStore::INT),
            Value::Untyped(Untyped::Float(_), _) => self.coerce(value, TypeStore::FLOAT64),
            Value::Typed(expr) => self.single_value(expr),
        }
    }

    fn const_error(&mut self, error: ConstError, op: &str, ty: Option<TypeId>, span: Span) {
        let diagnostic = match error {
            ConstError::DivisionByZero => Diagnostic::new(
                Severity::Error,
                "division by zero in a constant expression",
                span,
            ),
            ConstError::NeedsInteger => Diagnostic::new(
                Severity::Error,
                format!("`{op}` requires integer operands"),
                span,
            ),
            ConstError::NeedsBool => Diagnostic::new(
                Severity::Error,
                format!("`{op}` requires `bool` operands"),
                span,
            ),
            ConstError::IntLimit => Diagnostic::new(
                Severity::Error,
                format!(
                    "constant exceeds this compiler's {}-bit integer limit",
                    constant::MAX_INT_BITS
                ),
                span,
            )
            .note("the language requires at least 256 bits; the larger limit is an implementation limit"),
            ConstError::FloatOverflow => Diagnostic::new(
                Severity::Error,
                "floating-point constant is too large",
                span,
            )
            .note(format!(
                "constants must stay below 2^{} (implementation limit)",
                constant::MAX_FLOAT_LOG2
            )),
            ConstError::Overflow => {
                let ty = ty.map_or_else(String::new, |t| format!(" `{}`", self.name(t)));
                Diagnostic::new(
                    Severity::Error,
                    format!("constant expression overflows{ty}"),
                    span,
                )
            }
        };
        self.diagnostics.push(diagnostic);
    }

    fn single_value(&mut self, expr: hir::Expr) -> Option<hir::Expr> {
        match expr.types.len() {
            1 => Some(expr),
            0 => {
                self.error("this call has no value", expr.span);
                None
            }
            _ if matches!(expr.kind, ExprKind::MapLookup { .. }) => {
                self.error(
                    "map lookup produces two results, presence then value; bind both, as in `let found, value = m[key]`",
                    expr.span,
                );
                None
            }
            n => {
                let message = format!(
                    "expected one value, but this call returns {n} values; bind them first"
                );
                self.error(message, expr.span);
                None
            }
        }
    }

    fn expr(&mut self, expr: &ast::Expr, expected: Option<TypeId>) -> Option<Value> {
        let span = expr.span;
        let bool_const = |b| {
            Some(Value::Typed(typed(
                ExprKind::Const(Const::Bool(b)),
                TypeStore::BOOL,
                span,
            )))
        };
        match &expr.kind {
            ast::ExprKind::Name(name) => self.name_expr(name, span),
            ast::ExprKind::Int(base) => {
                let text = &self.text[span.start() as usize..span.end() as usize];
                self.literal(constant::parse_int(text, base.radix()), span)
            }
            ast::ExprKind::Float => {
                let text = &self.text[span.start() as usize..span.end() as usize];
                self.literal(constant::parse_float(text), span)
            }
            ast::ExprKind::String(value) => Some(Value::Typed(typed(
                ExprKind::Const(Const::String(value.clone())),
                TypeStore::STRING,
                span,
            ))),
            ast::ExprKind::Rune(value) => Some(Value::Typed(typed(
                ExprKind::Const(Const::Rune(*value)),
                TypeStore::RUNE,
                span,
            ))),
            ast::ExprKind::Bool(value) => bool_const(*value),
            ast::ExprKind::Nil => {
                if expected == Some(TypeStore::ERROR) {
                    Some(Value::Typed(typed(
                        ExprKind::Const(Const::Nil),
                        TypeStore::ERROR,
                        span,
                    )))
                } else {
                    self.error("`nil` needs an `error` context", span);
                    None
                }
            }
            ast::ExprKind::Malformed => None,
            ast::ExprKind::Paren(inner) => self.expr(inner, expected),
            ast::ExprKind::Unary { op, operand } => self.unary(*op, operand, span, expected),
            ast::ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, span, expected),
            ast::ExprKind::Await(_) => {
                self.unsupported("`await` is", span, "planned for a later milestone");
                None
            }
            ast::ExprKind::Try(inner) => self.try_expr(inner, span),
            ast::ExprKind::Call { callee, args } => self.call(callee, args, span),
            ast::ExprKind::Field { base, name } => self.field(base, name, span),
            ast::ExprKind::Index { base, index } => self.index(base, index, span),
            ast::ExprKind::Slice { base, low, high } => {
                self.slice(base, low.as_deref(), high.as_deref(), span, expected)
            }
            ast::ExprKind::StructLit { ty, fields } => self.struct_lit(ty, fields, span),
            ast::ExprKind::ArrayLit { ty, elements } => self.array_lit(ty, elements, span),
            ast::ExprKind::MapLit { ty, entries } => self.map_lit(ty, entries, span),
            ast::ExprKind::Closure(closure) => self.closure(closure, span),
        }
    }

    fn try_expr(&mut self, inner: &ast::Expr, span: Span) -> Option<Value> {
        fn call_or_await(expr: &ast::Expr) -> bool {
            match &expr.kind {
                ast::ExprKind::Call { .. } | ast::ExprKind::Await(_) => true,
                ast::ExprKind::Paren(inner) => call_or_await(inner),
                _ => false,
            }
        }

        if !call_or_await(inner) {
            self.error("`?` requires a call or awaited operation", span);
            return None;
        }
        if self.results.last() != Some(&TypeStore::ERROR) {
            self.error(
                "`?` requires a trailing `error` result in this function",
                span,
            );
            return None;
        }
        let Value::Typed(value) = self.expr(inner, None)? else {
            self.error("`?` requires a typed call", span);
            return None;
        };
        if value.types.last() != Some(&TypeStore::ERROR) {
            self.error("`?` requires a call with a trailing `error` result", span);
            return None;
        }
        let types = value.types[..value.types.len() - 1].to_vec();
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::Try(Box::new(value)),
            types,
            span,
        }))
    }

    fn name_expr(&mut self, name: &str, span: Span) -> Option<Value> {
        match *self.res.uses.get(&span)? {
            Res::Local(id) => {
                let ty = self.locals[id.0 as usize]?;
                Some(Value::Typed(typed(ExprKind::Local(id), ty, span)))
            }
            Res::Const(id) => match self.eval_const(id)? {
                ConstValue::Untyped(v) => Some(Value::Untyped(v, span)),
                ConstValue::Typed(ty, c) => Some(Value::Typed(typed(ExprKind::Const(c), ty, span))),
            },
            Res::Function(_) => {
                self.unsupported(
                    "declared functions used as values are",
                    span,
                    "wrap the call in a function literal, as in `func() { f() }`",
                );
                None
            }
            Res::Struct(_) | Res::Primitive(_) => {
                self.error(format!("`{name}` is a type, not a value"), span);
                None
            }
            Res::Println => {
                self.error("`println` can only be called", span);
                None
            }
            Res::Drop => {
                self.error("`drop` can only be called", span);
                None
            }
            Res::Clone => {
                self.error("`clone` can only be called", span);
                None
            }
            Res::Unsupported => None,
        }
    }

    fn literal(&mut self, value: Result<Untyped, ConstError>, span: Span) -> Option<Value> {
        match value {
            Ok(value) => Some(Value::Untyped(value, span)),
            Err(error) => {
                self.const_error(error, "", None, span);
                None
            }
        }
    }

    fn bool_value(value: bool, span: Span) -> Option<Value> {
        Some(Value::Typed(typed(
            ExprKind::Const(Const::Bool(value)),
            TypeStore::BOOL,
            span,
        )))
    }

    fn operator_applies(&self, op: BinaryOp, ty: TypeId) -> bool {
        let kind = self.types.kind(ty);
        let int = matches!(kind, TypeKind::Int(_));
        let numeric = self.types.is_numeric(ty);
        match op {
            BinaryOp::Add => numeric || kind == TypeKind::String,
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => numeric,
            BinaryOp::Rem
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => int,
            BinaryOp::Eq | BinaryOp::NotEq => {
                numeric
                    || matches!(
                        kind,
                        TypeKind::Bool | TypeKind::Rune | TypeKind::String | TypeKind::Error
                    )
            }
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                numeric || matches!(kind, TypeKind::Rune | TypeKind::String)
            }
            BinaryOp::And | BinaryOp::Or => kind == TypeKind::Bool,
        }
    }

    fn unary(
        &mut self,
        op: UnaryOp,
        operand: &ast::Expr,
        span: Span,
        expected: Option<TypeId>,
    ) -> Option<Value> {
        let value = self.expr(operand, if op == UnaryOp::Not { None } else { expected })?;
        let symbol = match op {
            UnaryOp::Plus => "+",
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::Complement => "^",
        };
        let expr = match value {
            Value::Untyped(value, _) => {
                if op == UnaryOp::Not {
                    self.error(
                        "`!` requires a `bool` operand, found a numeric constant",
                        span,
                    );
                    return None;
                }
                return match constant::untyped_unary(op, &value) {
                    Ok(value) => Some(Value::Untyped(value, span)),
                    Err(error) => {
                        self.const_error(error, symbol, None, span);
                        None
                    }
                };
            }
            Value::Typed(expr) => self.single_value(expr)?,
        };
        let ty = expr.ty();
        let valid = match op {
            UnaryOp::Not => ty == TypeStore::BOOL,
            UnaryOp::Plus | UnaryOp::Neg => self.types.is_numeric(ty),
            UnaryOp::Complement => self.types.int(ty).is_some(),
        };
        if !valid {
            let message = format!("unary `{symbol}` cannot be applied to `{}`", self.name(ty));
            self.error(message, span);
            return None;
        }
        let folded = match (constant(&expr), op) {
            (Some(&Const::Bool(b)), _) => Some(Ok(Const::Bool(!b))),
            (Some(&Const::Int(v)), UnaryOp::Plus) => Some(Ok(Const::Int(v))),
            (Some(&Const::Int(v)), UnaryOp::Neg) => {
                let int = self.types.int(ty).expect("integer type");
                Some(
                    v.checked_neg()
                        .filter(|&n| int.contains(n))
                        .map(Const::Int)
                        .ok_or(()),
                )
            }
            (Some(&Const::Int(v)), _) => {
                let int = self.types.int(ty).expect("integer type");
                Some(Ok(Const::Int(int.wrap(!v))))
            }
            // Constants never hold negative zero.
            (Some(&Const::Float(x)), UnaryOp::Neg) => {
                Some(Ok(Const::Float(if x == 0.0 { 0.0 } else { -x })))
            }
            (Some(&Const::Float(x)), _) => Some(Ok(Const::Float(x))),
            _ => None,
        };
        match folded {
            Some(Ok(c)) => Some(Value::Typed(typed(ExprKind::Const(c), ty, span))),
            Some(Err(())) => {
                self.const_error(ConstError::Overflow, symbol, Some(ty), span);
                None
            }
            None => Some(Value::Typed(typed(
                ExprKind::Unary {
                    op,
                    operand: Box::new(expr),
                },
                ty,
                span,
            ))),
        }
    }

    fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: Span,
        expected: Option<TypeId>,
    ) -> Option<Value> {
        if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
            return self.shift(op, lhs, rhs, span, expected);
        }
        let operand_expected = if op.is_comparison() || matches!(op, BinaryOp::And | BinaryOp::Or) {
            None
        } else {
            expected
        };
        let (left, right) =
            if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) && is_nil(lhs) && !is_nil(rhs) {
                let right = self.expr(rhs, None);
                let left_expected = match &right {
                    Some(Value::Typed(expr)) if expr.types.len() == 1 => Some(expr.ty()),
                    _ => None,
                };
                (self.expr(lhs, left_expected), right)
            } else {
                let left = self.expr(lhs, operand_expected);
                let right_expected = match &left {
                    Some(Value::Typed(expr)) if expr.types.len() == 1 => Some(expr.ty()),
                    _ => operand_expected,
                };
                let right = self.expr(rhs, right_expected);
                (left, right)
            };
        let (left, right) = (left?, right?);

        if let (Value::Untyped(a, _), Value::Untyped(b, _)) = (&left, &right) {
            return match constant::untyped_binary(op, a, b) {
                Ok(Folded::Untyped(value)) => Some(Value::Untyped(value, span)),
                Ok(Folded::Bool(value)) => Self::bool_value(value, span),
                Err(error) => {
                    self.const_error(error, op_str(op), None, span);
                    None
                }
            };
        }

        // The untyped side takes the typed side's type.
        let (left, right) = match (left, right) {
            (Value::Typed(l), r) => {
                let l = self.single_value(l)?;
                let r = self.coerce(r, l.ty())?;
                (l, r)
            }
            (l @ Value::Untyped(..), Value::Typed(r)) => {
                let r = self.single_value(r)?;
                let l = self.coerce(l, r.ty())?;
                (l, r)
            }
            (Value::Untyped(..), Value::Untyped(..)) => unreachable!("handled above"),
        };
        let ty = left.ty();
        if !self.operator_applies(op, ty) {
            let message = format!(
                "operator `{}` cannot be applied to `{}`",
                op_str(op),
                self.name(ty)
            );
            self.error(message, span);
            return None;
        }
        let result_ty = if op.is_comparison() {
            TypeStore::BOOL
        } else {
            ty
        };
        if let (Some(a), Some(b)) = (constant(&left), constant(&right)) {
            return self.fold(op, a.clone(), b.clone(), ty, span);
        }
        Some(Value::Typed(typed(
            ExprKind::Binary {
                op,
                lhs: Box::new(left),
                rhs: Box::new(right),
            },
            result_ty,
            span,
        )))
    }

    fn fold(&mut self, op: BinaryOp, a: Const, b: Const, ty: TypeId, span: Span) -> Option<Value> {
        if op.is_comparison() {
            let ordering = match (&a, &b) {
                (Const::Int(x), Const::Int(y)) => x.cmp(y),
                // Constants are finite, so floats are totally ordered.
                (Const::Float(x), Const::Float(y)) => x.partial_cmp(y).expect("finite"),
                (Const::Rune(x), Const::Rune(y)) => x.cmp(y),
                // Byte-wise UTF-8 order.
                (Const::String(x), Const::String(y)) => x.as_bytes().cmp(y.as_bytes()),
                (Const::Nil, Const::Nil) => std::cmp::Ordering::Equal,
                (Const::Bool(x), Const::Bool(y)) => x.cmp(y),
                _ => unreachable!("operands share a type"),
            };
            let result = match op {
                BinaryOp::Eq => ordering.is_eq(),
                BinaryOp::NotEq => ordering.is_ne(),
                BinaryOp::Lt => ordering.is_lt(),
                BinaryOp::LtEq => ordering.is_le(),
                BinaryOp::Gt => ordering.is_gt(),
                _ => ordering.is_ge(),
            };
            return Self::bool_value(result, span);
        }
        let result = match (a, b) {
            (Const::Bool(x), Const::Bool(y)) => {
                return Self::bool_value(if op == BinaryOp::And { x && y } else { x || y }, span);
            }
            (Const::String(x), Const::String(y)) => Ok(Const::String(x + &y)),
            (Const::Int(x), Const::Int(y)) => {
                let int = self.types.int(ty).expect("integer operands");
                constant::typed_int(op, x, y, int).map(Const::Int)
            }
            (Const::Float(x), Const::Float(y)) => {
                let float = self.types.float(ty).expect("float operands");
                constant::typed_float(op, x, y, float).map(Const::Float)
            }
            _ => unreachable!("operator validity was checked"),
        };
        match result {
            Ok(c) => Some(Value::Typed(typed(ExprKind::Const(c), ty, span))),
            Err(error) => {
                self.const_error(error, op_str(op), Some(ty), span);
                None
            }
        }
    }

    fn shift(
        &mut self,
        op: BinaryOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: Span,
        expected: Option<TypeId>,
    ) -> Option<Value> {
        let left = self.expr(lhs, expected);
        let right = self.expr(rhs, None);
        let (left, right) = (left?, right?);
        let (count, count_expr, count_span) = match right {
            Value::Untyped(value, count_span) => match value.to_integer() {
                Some(n) => (Some(n), None, count_span),
                None => {
                    let message = format!(
                        "shift count must be an integer, found `{}`",
                        value.describe()
                    );
                    self.error(message, count_span);
                    return None;
                }
            },
            Value::Typed(expr) => {
                let expr = self.single_value(expr)?;
                if self.types.int(expr.ty()).is_none() {
                    let message = format!(
                        "shift count must be an integer, found `{}`",
                        self.name(expr.ty())
                    );
                    self.error(message, expr.span);
                    return None;
                }
                let count = match constant(&expr) {
                    Some(&Const::Int(n)) => Some(BigInt::from_i128(n)),
                    _ => None,
                };
                let count_span = expr.span;
                (count, Some(expr), count_span)
            }
        };
        if let Some(n) = &count
            && n.is_negative()
        {
            self.error(format!("shift count `{n}` is negative"), count_span);
            return None;
        }
        let left = match left {
            Value::Untyped(value, left_span) => {
                let Some(integer) = value.to_integer() else {
                    let message =
                        format!("shifted constant `{}` must be an integer", value.describe());
                    self.error(message, left_span);
                    return None;
                };
                if let Some(n) = &count {
                    return match constant::untyped_shift(op, &integer, n) {
                        Ok(value) => Some(Value::Untyped(value, span)),
                        Err(error) => {
                            self.const_error(error, op_str(op), None, span);
                            None
                        }
                    };
                }
                // A runtime count gives the constant its contextual type.
                let ty = expected
                    .filter(|&t| self.types.int(t).is_some())
                    .unwrap_or(TypeStore::INT);
                self.coerce(Value::Untyped(Untyped::Int(integer), left_span), ty)?
            }
            Value::Typed(expr) => self.single_value(expr)?,
        };
        let ty = left.ty();
        let Some(int) = self.types.int(ty) else {
            let message = format!(
                "operator `{}` cannot be applied to `{}`",
                op_str(op),
                self.name(ty)
            );
            self.error(message, span);
            return None;
        };
        let count_value = match &count {
            Some(n) => match n.to_u64().filter(|&n| n < u64::from(int.bits)) {
                Some(n) => Some(n),
                None => {
                    let message = format!("shift count `{n}` is too large for `{}`", self.name(ty));
                    self.diagnostics.push(
                        Diagnostic::new(Severity::Error, message, count_span).note(format!(
                            "counts must be less than the width, {} bits",
                            int.bits
                        )),
                    );
                    return None;
                }
            },
            None => None,
        };
        if let (Some(&Const::Int(v)), Some(n)) = (constant(&left), count_value) {
            let value = shift_value(int, v, n as u32, op);
            return Some(Value::Typed(typed(
                ExprKind::Const(Const::Int(value)),
                ty,
                span,
            )));
        }
        let count_expr = count_expr.unwrap_or_else(|| {
            let n = count_value.expect("a constant count without an expression") as i128;
            typed(ExprKind::Const(Const::Int(n)), TypeStore::INT, count_span)
        });
        Some(Value::Typed(typed(
            ExprKind::Binary {
                op,
                lhs: Box::new(left),
                rhs: Box::new(count_expr),
            },
            ty,
            span,
        )))
    }

    fn call(&mut self, callee: &ast::Expr, args: &[ast::Expr], span: Span) -> Option<Value> {
        let res = match &callee.kind {
            ast::ExprKind::Name(_) => self.res.uses.get(&callee.span).copied(),
            ast::ExprKind::Field { base, name } => {
                return self.method_call(base, name, args, span);
            }
            _ => {
                let callee_expr = match self.expr(callee, None) {
                    Some(Value::Typed(expr)) => self.single_value(expr),
                    Some(Value::Untyped(..)) => {
                        self.error("this expression cannot be called", callee.span);
                        None
                    }
                    None => None,
                };
                return match callee_expr {
                    Some(expr) if self.is_func(expr.ty()) => self.value_call(expr, args, span),
                    Some(_) => {
                        self.error("this expression cannot be called", callee.span);
                        self.report_arg_errors(args);
                        None
                    }
                    None => {
                        self.report_arg_errors(args);
                        None
                    }
                };
            }
        };
        let name = match &callee.kind {
            ast::ExprKind::Name(name) => name.as_str(),
            _ => unreachable!(),
        };
        match res {
            Some(Res::Function(id)) => self.function_call(id, name, None, args, span),
            Some(Res::Println) => self.println(args, span),
            Some(Res::Drop) => self.drop_call(args, span),
            Some(Res::Clone) => self.clone_call(args, span),
            Some(Res::Primitive(ty)) if ty == TypeStore::ERROR => {
                self.error_constructor(args, span)
            }
            Some(Res::Primitive(ty)) => self.conversion(ty, args, span),
            Some(Res::Struct(_)) => {
                self.error(
                    format!("struct `{name}` is constructed with `{name}{{...}}`, not called"),
                    callee.span,
                );
                self.report_arg_errors(args);
                None
            }
            Some(Res::Local(id))
                if self.locals[id.0 as usize].is_some_and(|ty| self.is_func(ty)) =>
            {
                let ty = self.locals[id.0 as usize].expect("checked above");
                let callee = typed(ExprKind::Local(id), ty, callee.span);
                self.value_call(callee, args, span)
            }
            Some(Res::Local(id)) if self.locals[id.0 as usize].is_none() => {
                self.report_arg_errors(args);
                None
            }
            Some(Res::Local(_) | Res::Const(_)) => {
                self.error(format!("`{name}` is not a function"), callee.span);
                self.report_arg_errors(args);
                None
            }
            Some(Res::Unsupported) | None => {
                self.report_arg_errors(args);
                None
            }
        }
    }

    fn report_arg_errors(&mut self, args: &[ast::Expr]) {
        for arg in args {
            self.expr(arg, None);
        }
    }

    fn value_call(&mut self, callee: hir::Expr, args: &[ast::Expr], span: Span) -> Option<Value> {
        let signature = self
            .types
            .func_signature(callee.ty())
            .expect("a function-typed callee")
            .clone();
        if args.len() != signature.params.len() {
            let message = format!(
                "`{}` takes {} argument{} but {} {} given",
                self.source_text(callee.span),
                signature.params.len(),
                if signature.params.len() == 1 { "" } else { "s" },
                args.len(),
                if args.len() == 1 { "was" } else { "were" },
            );
            self.error(message, span);
            self.report_arg_errors(args);
            return None;
        }
        if let ExprKind::Local(local) = callee.kind {
            self.mark_exclusive(local);
        }
        let mut checked = Vec::new();
        let mut ok = true;
        for (arg, &(_, param)) in args.iter().zip(&signature.params) {
            match self
                .expr(arg, Some(param))
                .and_then(|v| self.coerce(v, param))
            {
                Some(expr) => checked.push(expr),
                None => ok = false,
            }
        }
        let accesses: Vec<ArgumentAccess> = signature
            .params
            .iter()
            .map(|&(mode, ty)| self.argument_access(mode, ty))
            .collect();
        if !ok || !self.check_argument_accesses(&accesses, &checked) {
            return None;
        }
        if let Some(callee_place) = argument_place(&callee)
            && let Some(arg) = checked.iter().find(|arg| {
                argument_place(arg).is_some_and(|place| places_overlap(&callee_place, &place))
            })
        {
            let name = self.source_text(callee.span).to_owned();
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("`{name}` is passed to its own call"),
                    arg.span,
                )
                .note("calling a function value uses it exclusively"),
            );
            return None;
        }
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::CallValue {
                callee: Box::new(callee),
                args: checked,
            },
            types: signature.results,
            span,
        }))
    }

    fn argument_access(&self, mode: ast::ParamMode, ty: TypeId) -> ArgumentAccess {
        let mutable_place = mode == ast::ParamMode::Mut || self.is_mut_slice(ty);
        ArgumentAccess {
            mutable_place,
            // Calling a function value may write through its captures.
            exclusive: mutable_place || self.is_func(ty),
            borrowing: mode != ast::ParamMode::Own,
        }
    }

    fn method_call(
        &mut self,
        base: &ast::Expr,
        name: &ast::Name,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let receiver = match self.expr(base, None) {
            Some(Value::Typed(expr)) => self.single_value(expr),
            Some(Value::Untyped(_, span)) => {
                self.error("an integer constant has no methods", span);
                None
            }
            None => None,
        };
        let Some(receiver) = receiver else {
            self.report_arg_errors(args);
            return None;
        };
        let ty = receiver.ty();
        if let TypeKind::Map { key, value } = self.types.kind(ty)
            && name.text == "remove"
        {
            return self.map_remove(receiver, key, value, args, span);
        }
        let strukt = self.types.struct_id(ty);
        let method = strukt.and_then(|s| self.res.methods.get(&(s, name.text.clone())).copied());
        let Some(id) = method else {
            let is_field = strukt.is_some_and(|s| {
                self.fields[s.0 as usize]
                    .iter()
                    .any(|(field, _, _)| *field == name.text)
            });
            let message = if is_field {
                format!("`{}` is a field, not a method", name.text)
            } else {
                format!("type `{}` has no method `{}`", self.name(ty), name.text)
            };
            let mut diagnostic = Diagnostic::new(Severity::Error, message, name.span);
            if matches!(self.types.kind(ty), TypeKind::DynArray { .. }) {
                diagnostic = diagnostic.note(
                    "`Array<T>` length, append, remove, and capacity APIs are not specified yet",
                );
            }
            if name.text == "clone" {
                diagnostic = diagnostic
                    .note("write `clone(value)`; a method form exists only for a custom `clone`");
            }
            if matches!(self.types.kind(ty), TypeKind::Map { .. }) {
                diagnostic = diagnostic
                    .note("map length, iteration, and borrowed entry APIs are not specified yet");
            }
            self.diagnostics.push(diagnostic);
            self.report_arg_errors(args);
            return None;
        };
        if name.text == "drop" {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "the `drop` method cannot be called directly",
                    name.span,
                )
                .note("destruction runs when ownership ends or through `drop(value)`"),
            );
            self.report_arg_errors(args);
            return None;
        }
        self.function_call(id, &name.text, Some(receiver), args, span)
    }

    fn map_remove(
        &mut self,
        map: hir::Expr,
        key_ty: TypeId,
        value_ty: TypeId,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let [key] = args else {
            let message = format!(
                "`remove` takes 1 argument but {} {} given",
                args.len(),
                if args.len() == 1 { "was" } else { "were" }
            );
            self.error(message, span);
            self.report_arg_errors(args);
            return None;
        };
        let key = self
            .expr(key, Some(key_ty))
            .and_then(|k| self.coerce(k, key_ty));
        let receiver_ok = self.mutable_place(&map, MutableUse::Argument);
        let key = key?;
        receiver_ok.then(|| {
            Value::Typed(hir::Expr {
                kind: ExprKind::MapRemove {
                    map: Box::new(map),
                    key: Box::new(key),
                },
                types: vec![TypeStore::BOOL, value_ty],
                span,
            })
        })
    }

    fn function_call(
        &mut self,
        id: FunctionId,
        name: &str,
        receiver: Option<hir::Expr>,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let Some(signature) = &self.signatures[id.0 as usize] else {
            self.report_arg_errors(args);
            return None;
        };
        let skipped = usize::from(receiver.is_some());
        let params = signature.params[skipped..].to_vec();
        let results = signature.results.clone();
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
        let mut checked: Vec<_> = receiver.into_iter().collect();
        let mut ok = true;
        for (arg, &param) in args.iter().zip(&params) {
            match self
                .expr(arg, Some(param))
                .and_then(|v| self.coerce(v, param))
            {
                Some(expr) => checked.push(expr),
                None => ok = false,
            }
        }
        if !ok || !self.check_mut_arguments(id, &checked) {
            return None;
        }
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::Call {
                function: id,
                args: checked,
            },
            types: results,
            span,
        }))
    }

    fn argument_accesses(&self, id: FunctionId, count: usize) -> Vec<ArgumentAccess> {
        let param_types = self.signatures[id.0 as usize]
            .as_ref()
            .map(|signature| signature.params.clone())
            .unwrap_or_default();
        self.res.locals[id.0 as usize]
            .iter()
            .take(count)
            .enumerate()
            .map(|(index, local)| {
                let LocalKind::Param(mode) = local.kind else {
                    unreachable!("parameters come first among a function's locals")
                };
                match param_types.get(index) {
                    Some(&ty) => self.argument_access(mode, ty),
                    None => ArgumentAccess {
                        mutable_place: mode == ast::ParamMode::Mut,
                        exclusive: mode == ast::ParamMode::Mut,
                        borrowing: mode != ast::ParamMode::Own,
                    },
                }
            })
            .collect()
    }

    fn check_mut_arguments(&mut self, id: FunctionId, args: &[hir::Expr]) -> bool {
        let accesses = self.argument_accesses(id, args.len());
        self.check_argument_accesses(&accesses, args)
    }

    fn check_argument_accesses(&mut self, accesses: &[ArgumentAccess], args: &[hir::Expr]) -> bool {
        let mut ok = true;
        for (arg, access) in args.iter().zip(accesses) {
            if access.mutable_place {
                ok &= self.mutable_place(arg, MutableUse::Argument);
            } else if access.exclusive
                && let ExprKind::Local(local) = arg.kind
            {
                self.mark_exclusive(local);
            }
        }
        if !ok {
            return false;
        }
        let places: Vec<_> = args.iter().map(argument_place).collect();
        for later in 1..args.len() {
            for earlier in 0..later {
                if !accesses[earlier].exclusive && !accesses[later].exclusive {
                    continue;
                }
                let (Some(a), Some(b)) = (&places[earlier], &places[later]) else {
                    continue;
                };
                if !places_overlap(a, b) {
                    continue;
                }
                let name = self.res.locals[self.current][a.0.0 as usize].name.clone();
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("`{name}` is also borrowed by another argument of this call"),
                        args[later].span,
                    )
                    .related(args[earlier].span, "the overlapping argument")
                    .note("a mutable borrow requires exclusive access"),
                );
                ok = false;
            }
        }
        ok && self.check_later_argument_mutation(accesses, args)
    }

    /// Rejects `f(x, g(x))` when `g` mutates `x`.
    fn check_later_argument_mutation(
        &mut self,
        accesses: &[ArgumentAccess],
        args: &[hir::Expr],
    ) -> bool {
        let mut ok = true;
        for later in 1..args.len() {
            let mut mutated = Vec::new();
            self.mutated_places(&args[later], &mut mutated);
            for earlier in 0..later {
                let Some(borrowed) =
                    argument_place(&args[earlier]).filter(|_| accesses[earlier].borrowing)
                else {
                    continue;
                };
                if !mutated.iter().any(|place| places_overlap(&borrowed, place)) {
                    continue;
                }
                let name = self.res.locals[self.current][borrowed.0.0 as usize]
                    .name
                    .clone();
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{name}` is borrowed by an earlier argument and mutated by a later one"
                        ),
                        args[later].span,
                    )
                    .related(args[earlier].span, "borrowed for the call here")
                    .note("a mutable borrow requires exclusive access"),
                );
                ok = false;
            }
        }
        ok
    }

    fn mutated_places(&self, expr: &hir::Expr, out: &mut Vec<ArgumentPlace>) {
        match &expr.kind {
            ExprKind::Call { function, args } => {
                let accesses = self.argument_accesses(*function, args.len());
                for (arg, access) in args.iter().zip(accesses) {
                    if access.exclusive
                        && let Some(place) = argument_place(arg)
                    {
                        out.push(place);
                    }
                }
            }
            ExprKind::CallValue { callee, args } => {
                out.extend(argument_place(callee));
                let signature = self
                    .types
                    .func_signature(callee.ty())
                    .expect("a function-typed callee");
                for (arg, &(mode, ty)) in args.iter().zip(&signature.params) {
                    if self.argument_access(mode, ty).exclusive
                        && let Some(place) = argument_place(arg)
                    {
                        out.push(place);
                    }
                }
            }
            // Creating a closure borrows its captures for as long as it lives.
            ExprKind::Closure { captures, .. } => {
                out.extend(
                    captures
                        .iter()
                        .filter(|(_, exclusive)| *exclusive)
                        .map(|&(local, _)| (local, Vec::new())),
                );
            }
            ExprKind::Slice {
                base,
                mutable: true,
                ..
            } => out.extend(argument_place(base)),
            ExprKind::MapRemove { map, .. } => out.extend(argument_place(map)),
            _ => {}
        }
        for child in subexpressions(expr) {
            self.mutated_places(child, out);
        }
    }

    fn mutable_place(&mut self, expr: &hir::Expr, usage: MutableUse) -> bool {
        match &expr.kind {
            ExprKind::Local(id) => {
                let decl = &self.res.locals[self.current][id.0 as usize];
                let (name, decl_span) = (decl.name.clone(), decl.span);
                let kind = self.binding_kind(*id);
                let (what, note) = match kind {
                    LocalKind::Var | LocalKind::Param(ast::ParamMode::Mut) => {
                        self.mark_exclusive(*id);
                        return true;
                    }
                    LocalKind::Param(ast::ParamMode::Borrow) if self.is_mut_slice(expr.ty()) => {
                        return true;
                    }
                    LocalKind::Capture(_) => unreachable!("binding_kind follows captures"),
                    LocalKind::Let => (
                        "immutable binding",
                        match usage {
                            MutableUse::Argument => "declare it with `var` to pass it as `mut`",
                            MutableUse::Slice => "declare it with `var` to borrow it mutably",
                        },
                    ),
                    LocalKind::Param(ast::ParamMode::Borrow) => (
                        "shared parameter",
                        "a parameter is a shared borrow unless declared `mut`",
                    ),
                    LocalKind::Param(ast::ParamMode::Own) => (
                        "`own` parameter",
                        "only `var` bindings and `mut` parameters are mutable places",
                    ),
                };
                let message = match usage {
                    MutableUse::Argument => {
                        format!("cannot pass {what} `{name}` as a `mut` argument")
                    }
                    MutableUse::Slice => format!("cannot take a mutable slice of {what} `{name}`"),
                };
                self.diagnostics.push(
                    Diagnostic::new(Severity::Error, message, expr.span)
                        .related(decl_span, "declared here")
                        .note(note),
                );
                false
            }
            ExprKind::Index { base, .. } => match self.types.kind(base.ty()) {
                TypeKind::Slice { mutable: true, .. } => true,
                TypeKind::Slice { .. } => {
                    let name = self.source_text(base.span).to_owned();
                    let message = match usage {
                        MutableUse::Argument => {
                            format!(
                                "cannot pass an element of shared slice `{name}` as a `mut` argument"
                            )
                        }
                        MutableUse::Slice => {
                            format!(
                                "cannot take a mutable slice of an element of shared slice `{name}`"
                            )
                        }
                    };
                    self.diagnostics.push(
                        Diagnostic::new(Severity::Error, message, expr.span)
                            .note("elements of `[]T` are read-only"),
                    );
                    false
                }
                _ => self.mutable_place(base, usage),
            },
            ExprKind::Field { base, .. } => self.mutable_place(base, usage),
            ExprKind::Slice { mutable: true, .. } if usage == MutableUse::Argument => true,
            _ => {
                let message = match usage {
                    MutableUse::Argument => {
                        "a `mut` argument must be a mutable place, not a temporary value"
                    }
                    MutableUse::Slice => {
                        "a mutable slice needs a mutable place, not a temporary value"
                    }
                };
                self.error(message, expr.span);
                false
            }
        }
    }

    fn println(&mut self, args: &[ast::Expr], span: Span) -> Option<Value> {
        let [arg] = args else {
            let message = format!(
                "`println` takes exactly 1 argument but {} were given",
                args.len()
            );
            self.error(message, span);
            self.report_arg_errors(args);
            return None;
        };
        let value = self.expr(arg, None)?;
        let expr = self.with_default_type(value)?;
        if !matches!(
            self.types.kind(expr.ty()),
            TypeKind::Bool
                | TypeKind::Int(_)
                | TypeKind::Float(_)
                | TypeKind::Rune
                | TypeKind::String
        ) {
            let message = format!(
                "`println` cannot print values of type `{}`",
                self.name(expr.ty())
            );
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, expr.span)
                    .note("printable types are bool, integers, floats, rune, and string"),
            );
            return None;
        }
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::Println(Box::new(expr)),
            types: Vec::new(),
            span,
        }))
    }

    fn drop_call(&mut self, args: &[ast::Expr], span: Span) -> Option<Value> {
        let [arg] = args else {
            self.error("`drop` takes exactly 1 argument", span);
            self.report_arg_errors(args);
            return None;
        };
        let value = self.expr(arg, None)?;
        let expr = self.with_default_type(value)?;
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::Drop(Box::new(expr)),
            types: Vec::new(),
            span,
        }))
    }

    fn custom_clone(&self, ty: TypeId) -> Option<FunctionId> {
        let id = self.types.struct_id(ty)?;
        self.res.methods.get(&(id, "clone".to_string())).copied()
    }

    /// The field or element type that stops `ty` from being cloned.
    fn clone_blocker(&self, ty: TypeId) -> Option<TypeId> {
        let element_blocker = |element: TypeId| {
            if self.type_is_copy(element) {
                None
            } else {
                self.clone_blocker(element)
            }
        };
        match self.types.kind(ty) {
            TypeKind::Struct(id) => {
                if self.custom_clone(ty).is_some() {
                    return None;
                }
                if self.res.methods.contains_key(&(id, "drop".to_string())) {
                    return Some(ty);
                }
                self.fields[id.0 as usize]
                    .iter()
                    .filter_map(|(_, field_ty, _)| *field_ty)
                    .find_map(element_blocker)
            }
            TypeKind::Array { element, .. } | TypeKind::DynArray { element } => {
                element_blocker(element)
            }
            TypeKind::Map { value, .. } => element_blocker(value),
            _ => Some(ty),
        }
    }

    fn clone_call(&mut self, args: &[ast::Expr], span: Span) -> Option<Value> {
        let [arg] = args else {
            self.error("`clone` takes exactly 1 argument", span);
            self.report_arg_errors(args);
            return None;
        };
        let value = self.expr(arg, None)?;
        let expr = self.with_default_type(value)?;
        let ty = expr.ty();
        if !matches!(
            self.types.kind(ty),
            TypeKind::Struct(_)
                | TypeKind::Array { .. }
                | TypeKind::DynArray { .. }
                | TypeKind::Map { .. }
        ) {
            let message = format!("cannot clone a value of type `{}`", self.name(ty));
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, expr.span)
                    .note("clone applies to structs, fixed arrays, `Array<T>`, and maps"),
            );
            return None;
        }
        if let Some(id) = self.custom_clone(ty) {
            return self.function_call(id, "clone", Some(expr), &[], span);
        }
        if self.contains_mut_view(ty) {
            let message = format!(
                "cannot clone `{}`, which holds a `mut []T` view",
                self.name(ty)
            );
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, expr.span)
                    .note("a clone would duplicate exclusive access to the viewed storage"),
            );
            return None;
        }
        if let Some(blocker) = self.clone_blocker(ty) {
            let message = format!("type `{}` cannot be cloned", self.name(blocker));
            let mut diagnostic = Diagnostic::new(Severity::Error, message, expr.span);
            diagnostic = if self.types.struct_id(blocker).is_some() {
                diagnostic.note(format!(
                    "a type with a custom `drop` needs a custom `clone`: `func (value {}) clone() {}`",
                    self.name(blocker),
                    self.name(blocker)
                ))
            } else {
                diagnostic.note("its fields or elements must be Copy or clonable")
            };
            self.diagnostics.push(diagnostic);
            return None;
        }
        Some(Value::Typed(typed(
            ExprKind::Clone(Box::new(expr)),
            ty,
            span,
        )))
    }

    fn error_constructor(&mut self, args: &[ast::Expr], span: Span) -> Option<Value> {
        let [arg] = args else {
            self.error("`error` takes exactly 1 string argument", span);
            self.report_arg_errors(args);
            return None;
        };
        let value = self.expr(arg, Some(TypeStore::STRING))?;
        let value = self.coerce(value, TypeStore::STRING)?;
        Some(Value::Typed(typed(
            ExprKind::Error(Box::new(value)),
            TypeStore::ERROR,
            span,
        )))
    }

    fn conversion(&mut self, target: TypeId, args: &[ast::Expr], span: Span) -> Option<Value> {
        let [arg] = args else {
            self.error("a conversion takes exactly one argument", span);
            self.report_arg_errors(args);
            return None;
        };
        if !self.types.is_numeric(target) {
            self.report_arg_errors(args);
            if target == TypeStore::RUNE {
                self.unsupported(
                    "rune conversions are",
                    span,
                    "planned with numeric typing (M5–M8)",
                );
            } else {
                let message = format!(
                    "`{}` is not a conversion; only numeric conversions exist",
                    self.name(target)
                );
                self.error(message, span);
            }
            return None;
        }
        let value = match self.expr(arg, None)? {
            untyped @ Value::Untyped(..) => {
                let mut expr = self.coerce(untyped, target)?;
                expr.span = span;
                return Some(Value::Typed(expr));
            }
            Value::Typed(expr) => self.single_value(expr)?,
        };
        let source = value.ty();
        if !self.types.is_numeric(source) {
            let message = format!(
                "cannot convert `{}` to `{}`",
                self.name(source),
                self.name(target)
            );
            self.error(message, span);
            return None;
        }
        let Some(c) = constant(&value) else {
            return Some(Value::Typed(typed(
                ExprKind::Convert(Box::new(value)),
                target,
                span,
            )));
        };
        let converted = match (c, self.types.kind(target)) {
            (&Const::Int(v), TypeKind::Int(int)) => {
                if int.contains(v) {
                    Ok(Const::Int(v))
                } else {
                    Err(format!(
                        "constant `{v}` does not fit in `{}`",
                        self.name(target)
                    ))
                }
            }
            (&Const::Int(v), TypeKind::Float(float)) => constant::int_to_float(v, float)
                .map(Const::Float)
                .ok_or_else(|| format!("constant `{v}` overflows `{}`", self.name(target))),
            (&Const::Float(x), TypeKind::Int(int)) => match constant::float_to_int(x, int) {
                Ok(v) => Ok(Const::Int(v)),
                Err(Unrepresentable::NotInteger) => Err(format!(
                    "constant `{x:?}` is not an integer, so it cannot be converted to `{}`",
                    self.name(target)
                )),
                Err(Unrepresentable::OutOfRange) => Err(format!(
                    "constant `{x:?}` does not fit in `{}`",
                    self.name(target)
                )),
            },
            (&Const::Float(x), TypeKind::Float(float)) => constant::float_to_float(x, float)
                .map(Const::Float)
                .ok_or_else(|| format!("constant `{x:?}` overflows `{}`", self.name(target))),
            _ => unreachable!("numeric constants and targets"),
        };
        match converted {
            Ok(c) => Some(Value::Typed(typed(ExprKind::Const(c), target, span))),
            Err(message) => {
                self.error(message, span);
                None
            }
        }
    }

    fn field_of(&mut self, ty: TypeId, name: &ast::Name) -> Option<(FieldId, TypeId)> {
        let Some(strukt) = self.types.struct_id(ty) else {
            let message = format!("type `{}` has no fields", self.name(ty));
            self.error(message, name.span);
            return None;
        };
        let fields = &self.fields[strukt.0 as usize];
        match fields.iter().position(|(n, _, _)| *n == name.text) {
            Some(index) => Some((FieldId(index as u32), fields[index].1?)),
            None => {
                let message = format!("no field `{}` on type `{}`", name.text, self.name(ty));
                self.error(message, name.span);
                None
            }
        }
    }

    fn field(&mut self, base: &ast::Expr, name: &ast::Name, span: Span) -> Option<Value> {
        let base = match self.expr(base, None)? {
            Value::Untyped(_, span) => {
                self.error("an integer constant has no fields", span);
                return None;
            }
            Value::Typed(expr) => self.single_value(expr)?,
        };
        let (field, ty) = self.field_of(base.ty(), name)?;
        Some(Value::Typed(typed(
            ExprKind::Field {
                base: Box::new(base),
                field,
            },
            ty,
            span,
        )))
    }

    fn index(&mut self, base: &ast::Expr, index: &ast::Expr, span: Span) -> Option<Value> {
        let base = self.indexable_base(base)?;
        if let TypeKind::Map { key, value } = self.types.kind(base.ty()) {
            return self.map_lookup(base, key, value, index, span);
        }
        let (index_expr, element) = self.checked_index(base.ty(), base.span, index)?;
        Some(Value::Typed(typed(
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index_expr),
            },
            element,
            span,
        )))
    }

    fn indexable_base(&mut self, base: &ast::Expr) -> Option<hir::Expr> {
        match self.expr(base, None)? {
            Value::Untyped(_, span) => {
                self.error("an integer constant cannot be indexed", span);
                None
            }
            Value::Typed(expr) => self.single_value(expr),
        }
    }

    fn checked_index(
        &mut self,
        base_ty: TypeId,
        base_span: Span,
        index: &ast::Expr,
    ) -> Option<(hir::Expr, TypeId)> {
        let (element, length, what) = match self.types.kind(base_ty) {
            TypeKind::Array { element, size } => (element, Some(size), "array"),
            TypeKind::DynArray { element } => (element, None, "array"),
            TypeKind::Slice { element, .. } => (element, None, "slice"),
            _ => {
                let message = format!("type `{}` cannot be indexed", self.name(base_ty));
                self.error(message, base_span);
                self.expr(index, None);
                return None;
            }
        };
        let index_value = self.expr(index, None)?;
        let index_expr = self.with_default_type(index_value)?;
        if self.types.int(index_expr.ty()).is_none() {
            let message = format!(
                "{what} index must be an integer, found `{}`",
                self.name(index_expr.ty())
            );
            self.error(message, index_expr.span);
            return None;
        }
        if let Some(&Const::Int(n)) = constant(&index_expr)
            && (n < 0 || length.is_some_and(|size| n >= i128::from(size)))
        {
            let message = format!(
                "{what} index `{n}` is out of range for `{}`",
                self.name(base_ty)
            );
            self.error(message, index_expr.span);
            return None;
        }
        Some((index_expr, element))
    }

    /// Exclusive only when the context expects `mut []T`.
    fn slice(
        &mut self,
        base: &ast::Expr,
        low: Option<&ast::Expr>,
        high: Option<&ast::Expr>,
        span: Span,
        expected: Option<TypeId>,
    ) -> Option<Value> {
        let base = self.indexable_base(base)?;
        let (element, length) = match self.types.kind(base.ty()) {
            TypeKind::Array { element, size } => (element, Some(size)),
            TypeKind::Slice { element, .. } | TypeKind::DynArray { element } => (element, None),
            _ => {
                let message = format!("type `{}` cannot be sliced", self.name(base.ty()));
                self.error(message, base.span);
                for bound in [low, high].into_iter().flatten() {
                    self.expr(bound, None);
                }
                return None;
            }
        };
        let low = low.map(|bound| self.slice_bound(bound, base.ty(), length));
        let high = high.map(|bound| self.slice_bound(bound, base.ty(), length));
        if [&low, &high]
            .iter()
            .any(|bound| matches!(bound, Some(None)))
        {
            return None;
        }
        let (low, high) = (low.flatten(), high.flatten());
        if let (Some(lo), Some(hi)) = (
            low.as_ref().and_then(constant),
            high.as_ref().and_then(constant),
        ) && let (Const::Int(lo), Const::Int(hi)) = (lo, hi)
            && lo > hi
        {
            self.error(
                format!("slice lower bound `{lo}` exceeds upper bound `{hi}`"),
                span,
            );
            return None;
        }
        let mutable = expected.is_some_and(|ty| self.is_mut_slice(ty));
        if mutable && !self.mutable_slice_source(&base) {
            return None;
        }
        Some(Value::Typed(typed(
            ExprKind::Slice {
                base: Box::new(base),
                low: low.map(Box::new),
                high: high.map(Box::new),
                mutable,
            },
            self.types.slice_type(element, mutable),
            span,
        )))
    }

    fn slice_bound(
        &mut self,
        bound: &ast::Expr,
        base_ty: TypeId,
        length: Option<u32>,
    ) -> Option<hir::Expr> {
        let value = self.expr(bound, None)?;
        let bound = self.with_default_type(value)?;
        if self.types.int(bound.ty()).is_none() {
            let message = format!(
                "slice bound must be an integer, found `{}`",
                self.name(bound.ty())
            );
            self.error(message, bound.span);
            return None;
        }
        if let Some(&Const::Int(n)) = constant(&bound)
            && (n < 0 || length.is_some_and(|size| n > i128::from(size)))
        {
            let message = format!(
                "slice bound `{n}` is out of range for `{}`",
                self.name(base_ty)
            );
            self.error(message, bound.span);
            return None;
        }
        Some(bound)
    }

    fn mutable_slice_source(&mut self, base: &hir::Expr) -> bool {
        if let TypeKind::Slice { mutable, .. } = self.types.kind(base.ty()) {
            if !mutable {
                let name = self.source_text(base.span).to_owned();
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("cannot take a mutable slice of shared slice `{name}`"),
                        base.span,
                    )
                    .note("a shared view cannot be upgraded to `mut []T`"),
                );
            }
            return mutable;
        }
        self.mutable_place(base, MutableUse::Slice)
    }

    fn source_text(&self, span: Span) -> &str {
        &self.text[span.start() as usize..span.end() as usize]
    }

    /// Only Copy values can be copied out through the shared borrow.
    fn map_lookup(
        &mut self,
        map: hir::Expr,
        key_ty: TypeId,
        value_ty: TypeId,
        key: &ast::Expr,
        span: Span,
    ) -> Option<Value> {
        let key = self
            .expr(key, Some(key_ty))
            .and_then(|k| self.coerce(k, key_ty))?;
        if !self.type_is_copy(value_ty) {
            let message = format!(
                "cannot look up a Move value of type `{}`; use `m.remove(key)` to take ownership",
                self.name(value_ty)
            );
            self.error(message, span);
            return None;
        }
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::MapLookup {
                map: Box::new(map),
                key: Box::new(key),
            },
            types: vec![TypeStore::BOOL, value_ty],
            span,
        }))
    }

    fn map_lit(&mut self, ty: &ast::Type, entries: &[ast::MapEntry], span: Span) -> Option<Value> {
        let map_ty = self.resolve_type(ty);
        let Some(TypeKind::Map { key, value }) = map_ty.map(|ty| self.types.kind(ty)) else {
            for entry in entries {
                self.expr(&entry.key, None);
                self.expr(&entry.value, None);
            }
            return None;
        };
        let mut checked = Vec::with_capacity(entries.len());
        let mut constant_keys: Vec<(Const, Span)> = Vec::new();
        let mut ok = true;
        for entry in entries {
            let key_expr = self
                .expr(&entry.key, Some(key))
                .and_then(|k| self.coerce(k, key));
            let value_expr = self
                .expr(&entry.value, Some(value))
                .and_then(|v| self.coerce(v, value));
            let (Some(key_expr), Some(value_expr)) = (key_expr, value_expr) else {
                ok = false;
                continue;
            };
            if let Some(constant) = constant(&key_expr) {
                if let Some((_, first)) = constant_keys.iter().find(|(seen, _)| seen == constant) {
                    let text = self.source_text(key_expr.span).to_owned();
                    self.diagnostics.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("duplicate key `{text}` in map literal"),
                            key_expr.span,
                        )
                        .related(*first, "first used here"),
                    );
                    ok = false;
                } else {
                    constant_keys.push((constant.clone(), key_expr.span));
                }
            }
            checked.push((key_expr, value_expr));
        }
        ok.then(|| {
            Value::Typed(typed(
                ExprKind::MapLit {
                    key,
                    value,
                    entries: checked,
                },
                map_ty.expect("resolved above"),
                span,
            ))
        })
    }

    fn array_lit(&mut self, ty: &ast::Type, elements: &[ast::Expr], span: Span) -> Option<Value> {
        let array_ty = self.resolve_type(ty)?;
        let (element, size) = match self.types.kind(array_ty) {
            TypeKind::Array { element, size } => (element, Some(size)),
            TypeKind::DynArray { element } => (element, None),
            _ => unreachable!("array literals are written with an array type"),
        };
        if let Some(count) = size.map(|size| size as usize)
            && elements.len() != count
        {
            let message = format!(
                "array literal has {} element{}, expected {count}",
                elements.len(),
                if elements.len() == 1 { "" } else { "s" }
            );
            self.error(message, span);
            for element_expr in elements {
                self.expr(element_expr, Some(element));
            }
            return None;
        }
        let mut checked = Vec::with_capacity(elements.len());
        let mut ok = true;
        for element_expr in elements {
            match self.expr(element_expr, Some(element)) {
                Some(value) => match self.coerce(value, element) {
                    Some(expr) => checked.push(expr),
                    None => ok = false,
                },
                None => ok = false,
            }
        }
        ok.then(|| {
            Value::Typed(typed(
                ExprKind::ArrayLit {
                    element,
                    elements: checked,
                },
                array_ty,
                span,
            ))
        })
    }

    fn struct_lit(
        &mut self,
        ty: &ast::Name,
        inits: &[ast::FieldInit],
        span: Span,
    ) -> Option<Value> {
        let Some(Res::Struct(strukt)) = self.res.uses.get(&ty.span).copied() else {
            for init in inits {
                self.expr(&init.value, None);
            }
            return None;
        };
        let struct_ty = self.types.struct_type(strukt);
        let mut seen: HashMap<usize, Span> = HashMap::new();
        let mut fields = Vec::new();
        let mut ok = true;
        for init in inits {
            let declared = &self.fields[strukt.0 as usize];
            let Some(index) = declared.iter().position(|(n, _, _)| *n == init.name.text) else {
                let message = format!("struct `{}` has no field `{}`", ty.text, init.name.text);
                self.error(message, init.name.span);
                self.expr(&init.value, None);
                ok = false;
                continue;
            };
            let field_ty = declared[index].1;
            if let Some(&first) = seen.get(&index) {
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("field `{}` is initialized more than once", init.name.text),
                        init.name.span,
                    )
                    .related(first, "first initialized here"),
                );
                ok = false;
            }
            seen.insert(index, init.name.span);
            let value = self.expr(&init.value, field_ty);
            match (value, field_ty) {
                (Some(value), Some(field_ty)) => match self.coerce(value, field_ty) {
                    Some(expr) => fields.push((FieldId(index as u32), expr)),
                    None => ok = false,
                },
                _ => ok = false,
            }
        }
        let missing: Vec<String> = self.fields[strukt.0 as usize]
            .iter()
            .enumerate()
            .filter(|(index, _)| !seen.contains_key(index))
            .map(|(_, (name, _, _))| format!("`{name}`"))
            .collect();
        if !missing.is_empty() {
            let message = format!(
                "missing field{} {} in `{}` literal",
                if missing.len() == 1 { "" } else { "s" },
                missing.join(", "),
                ty.text
            );
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, span)
                    .note("struct literals must initialize every field"),
            );
            ok = false;
        }
        ok.then(|| {
            Value::Typed(typed(
                ExprKind::StructLit { strukt, fields },
                struct_ty,
                span,
            ))
        })
    }

    fn block(&mut self, block: &ast::Block) -> hir::Block {
        hir::Block {
            stmts: block.stmts.iter().filter_map(|s| self.stmt(s)).collect(),
            span: block.span,
        }
    }

    fn stmt(&mut self, stmt: &ast::Stmt) -> Option<hir::Stmt> {
        let span = stmt.span;
        let kind = match &stmt.kind {
            ast::StmtKind::Binding(binding) => self.binding(binding)?,
            ast::StmtKind::Assign {
                targets,
                op,
                values,
            } => self.assign(targets, *op, values, span)?,
            ast::StmtKind::Expr(expr) => match self.expr(expr, None)? {
                Value::Typed(expr) if expr.types.contains(&TypeStore::ERROR) => {
                    self.error(
                        "error result must be used or explicitly discarded",
                        expr.span,
                    );
                    return None;
                }
                Value::Typed(expr) => hir::StmtKind::Expr(expr),
                Value::Untyped(..) => unreachable!("the parser accepts only call statements"),
            },
            ast::StmtKind::Return(values) => self.return_stmt(values, span)?,
            ast::StmtKind::Break | ast::StmtKind::Continue => {
                let keyword = if matches!(stmt.kind, ast::StmtKind::Break) {
                    "break"
                } else {
                    "continue"
                };
                if self.loop_depth == 0 {
                    self.error(format!("`{keyword}` outside of a loop"), span);
                    return None;
                }
                if keyword == "break" {
                    hir::StmtKind::Break
                } else {
                    hir::StmtKind::Continue
                }
            }
            ast::StmtKind::If(if_stmt) => self.if_stmt(if_stmt)?,
            ast::StmtKind::For(for_stmt) => {
                let (init, condition, update) = match &for_stmt.header {
                    ForHeader::Infinite => (None, None, None),
                    ForHeader::Condition(condition) => {
                        (None, Some(self.condition(condition)), None)
                    }
                    ForHeader::Counting {
                        init,
                        condition,
                        update,
                    } => {
                        let init = self.stmt(init).map(Box::new);
                        let condition = self.condition(condition);
                        let update = self.stmt(update).map(Box::new);
                        (Some(init), Some(condition), Some(update))
                    }
                };
                self.loop_depth += 1;
                let body = self.block(&for_stmt.body);
                self.loop_depth -= 1;
                // An inner `None` marks a part that failed to check.
                hir::StmtKind::Loop {
                    init: match init {
                        Some(init) => Some(init?),
                        None => None,
                    },
                    condition: match condition {
                        Some(condition) => Some(condition?),
                        None => None,
                    },
                    update: match update {
                        Some(update) => Some(update?),
                        None => None,
                    },
                    body,
                }
            }
            ast::StmtKind::Block(block) => hir::StmtKind::Block(self.block(block)),
        };
        Some(hir::Stmt { kind, span })
    }

    fn condition(&mut self, condition: &ast::Expr) -> Option<hir::Expr> {
        let value = self.expr(condition, Some(TypeStore::BOOL))?;
        self.coerce(value, TypeStore::BOOL)
    }

    fn if_stmt(&mut self, if_stmt: &ast::If) -> Option<hir::StmtKind> {
        let condition = self.condition(&if_stmt.condition);
        let then_block = self.block(&if_stmt.then_block);
        let else_block = match &if_stmt.else_branch {
            None => None,
            Some(ast::Else::Block(block)) => Some(self.block(block)),
            Some(ast::Else::If(inner)) => {
                let kind = self.if_stmt(inner);
                Some(hir::Block {
                    stmts: kind
                        .map(|kind| hir::Stmt {
                            kind,
                            span: inner.span,
                        })
                        .into_iter()
                        .collect(),
                    span: inner.span,
                })
            }
        };
        Some(hir::StmtKind::If {
            condition: condition?,
            then_block,
            else_block,
        })
    }

    fn set_local(&mut self, name: &ast::Name, ty: TypeId) {
        if let Some(Res::Local(id)) = self.res.declarations.get(&name.span) {
            self.locals[id.0 as usize] = Some(ty);
        }
    }

    fn local_of(&self, target: &BindingTarget) -> Option<LocalId> {
        match target {
            BindingTarget::Name(name) => match self.res.declarations.get(&name.span) {
                Some(Res::Local(id)) => Some(*id),
                _ => None,
            },
            BindingTarget::Discard(_) => None,
        }
    }

    fn binding(&mut self, binding: &ast::Binding) -> Option<hir::StmtKind> {
        if binding.kind == BindingKind::Const {
            if let [BindingTarget::Name(name)] = &binding.targets[..]
                && let Some(Res::Const(id)) = self.res.declarations.get(&name.span).copied()
            {
                self.eval_const(id);
            }
            return None;
        }
        let declared = binding.ty.as_ref().map(|ty| self.resolve_type(ty));
        let value = self.expr(&binding.value, declared.flatten());
        if let Some(None) = declared {
            return None;
        }
        let value = value?;
        let targets: Vec<Option<LocalId>> =
            binding.targets.iter().map(|t| self.local_of(t)).collect();
        if let [target] = &binding.targets[..] {
            let expr = match declared.flatten() {
                Some(ty) => self.coerce(value, ty)?,
                None => self.with_default_type(value)?,
            };
            if let BindingTarget::Name(name) = target {
                self.set_local(name, expr.ty());
            }
            return Some(hir::StmtKind::Let {
                targets,
                value: expr,
            });
        }
        let expr = match value {
            Value::Typed(expr) if expr.types.len() == targets.len() => expr,
            other => {
                let found = match &other {
                    Value::Typed(expr) => expr.types.len(),
                    Value::Untyped(..) => 1,
                };
                let message = format!(
                    "expected {} values for this binding, found {found}",
                    targets.len()
                );
                self.error(message, other.span());
                return None;
            }
        };
        for (target, &ty) in binding.targets.iter().zip(&expr.types) {
            if let BindingTarget::Name(name) = target {
                self.set_local(name, ty);
            }
        }
        Some(hir::StmtKind::Let {
            targets,
            value: expr,
        })
    }

    fn assignable_place(&mut self, expr: &ast::Expr) -> Option<hir::Place> {
        let (place, slice_deref) = self.target_place(expr)?;
        self.writable_place(place, slice_deref, expr)
    }

    fn writable_place(
        &mut self,
        place: hir::Place,
        slice_deref: Option<SliceDeref>,
        expr: &ast::Expr,
    ) -> Option<hir::Place> {
        match slice_deref {
            Some(SliceDeref { mutable: true, .. }) => Some(place),
            Some(SliceDeref { base_span, .. }) => {
                let name = self.source_text(base_span).to_owned();
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("cannot assign through shared slice `{name}`"),
                        expr.span,
                    )
                    .note("elements of `[]T` are read-only"),
                );
                None
            }
            None => self.writable_root(&place, expr).then_some(place),
        }
    }

    fn writable_root(&mut self, place: &hir::Place, expr: &ast::Expr) -> bool {
        let decl = &self.res.locals[self.current][place.root.0 as usize];
        let (name, decl_span) = (decl.name.clone(), decl.span);
        match self.binding_kind(place.root) {
            LocalKind::Var | LocalKind::Param(ast::ParamMode::Mut) => {
                self.mark_exclusive(place.root);
                true
            }
            LocalKind::Capture(_) => unreachable!("binding_kind follows captures"),
            LocalKind::Let => {
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("cannot assign to immutable binding `{name}`"),
                        expr.span,
                    )
                    .related(decl_span, "declared with `let` here")
                    .note("declare it with `var` to allow assignment"),
                );
                false
            }
            LocalKind::Param(ast::ParamMode::Borrow) => {
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("cannot assign to parameter `{name}`"),
                        expr.span,
                    )
                    .related(decl_span, "a shared borrow by default"),
                );
                false
            }
            LocalKind::Param(ast::ParamMode::Own) => {
                self.unsupported(
                    "assigning to `own` parameters is",
                    expr.span,
                    "planned for a later milestone",
                );
                false
            }
        }
    }

    /// The last slice indexed through decides writability instead of the root.
    fn target_place(&mut self, expr: &ast::Expr) -> Option<(hir::Place, Option<SliceDeref>)> {
        let is_place = |base: &ast::Expr| {
            matches!(
                base.kind,
                ast::ExprKind::Name(_) | ast::ExprKind::Field { .. } | ast::ExprKind::Index { .. }
            )
        };
        match &expr.kind {
            ast::ExprKind::Name(name) => {
                let res = *self.res.uses.get(&expr.span)?;
                let Res::Local(id) = res else {
                    let what = match res {
                        Res::Const(_) => "a constant",
                        Res::Function(_) => "a function",
                        _ => "this name",
                    };
                    self.error(
                        format!("cannot assign to `{name}`, which is {what}"),
                        expr.span,
                    );
                    return None;
                };
                let ty = self.locals[id.0 as usize]?;
                let place = hir::Place {
                    root: id,
                    projections: Vec::new(),
                    ty,
                    span: expr.span,
                };
                Some((place, None))
            }
            ast::ExprKind::Field { base, name } => {
                if !is_place(base) {
                    self.expr(base, None);
                    self.error("cannot assign to a field of a temporary value", expr.span);
                    return None;
                }
                let (mut place, slice_deref) = self.target_place(base)?;
                let (field, ty) = self.field_of(place.ty, name)?;
                place.projections.push(hir::Projection::Field(field));
                place.ty = ty;
                place.span = expr.span;
                Some((place, slice_deref))
            }
            ast::ExprKind::Index { base, index } => {
                if !is_place(base) {
                    self.expr(base, None);
                    self.expr(index, None);
                    self.error("cannot assign to an index of a temporary value", expr.span);
                    return None;
                }
                let (mut place, mut slice_deref) = self.target_place(base)?;
                if matches!(self.types.kind(place.ty), TypeKind::Map { .. }) {
                    self.expr(index, None);
                    self.error(
                        "map entries are not addressable places; assign `m[key] = value` on its own, or update a copy and assign it back",
                        expr.span,
                    );
                    return None;
                }
                if let TypeKind::Slice { mutable, .. } = self.types.kind(place.ty) {
                    slice_deref = Some(SliceDeref {
                        mutable,
                        base_span: base.span,
                    });
                }
                let (index_expr, element) = self.checked_index(place.ty, base.span, index)?;
                place
                    .projections
                    .push(hir::Projection::Index(Box::new(index_expr)));
                place.ty = element;
                place.span = expr.span;
                Some((place, slice_deref))
            }
            _ => unreachable!("the parser accepts only name, field, and index targets"),
        }
    }

    /// `Some(None)` means the base already failed to check.
    #[allow(clippy::type_complexity)]
    fn map_entry_target<'e>(
        &mut self,
        target: &'e ast::Expr,
    ) -> Option<Option<(hir::Place, Option<SliceDeref>, &'e ast::Expr, &'e ast::Expr)>> {
        let ast::ExprKind::Index { base, index } = &target.kind else {
            return None;
        };
        if !matches!(
            base.kind,
            ast::ExprKind::Name(_) | ast::ExprKind::Field { .. } | ast::ExprKind::Index { .. }
        ) {
            return None;
        }
        let Some((place, slice_deref)) = self.target_place(base) else {
            return Some(None);
        };
        matches!(self.types.kind(place.ty), TypeKind::Map { .. }).then_some(Some((
            place,
            slice_deref,
            &**base,
            &**index,
        )))
    }

    #[allow(clippy::too_many_arguments)]
    fn map_assign(
        &mut self,
        map: hir::Place,
        slice_deref: Option<SliceDeref>,
        base: &ast::Expr,
        key: &ast::Expr,
        op: AssignOp,
        values: &[ast::Expr],
        span: Span,
    ) -> Option<hir::StmtKind> {
        let TypeKind::Map {
            key: key_ty,
            value: value_ty,
        } = self.types.kind(map.ty)
        else {
            unreachable!("checked by map_entry_target")
        };
        if let AssignOp::Compound(_) = op {
            self.error(
                "compound map assignment is not allowed; look up, compute, then assign",
                span,
            );
            self.report_arg_errors(values);
            return None;
        }
        let [value] = values else {
            let message = format!("assignment has 1 target but {} values", values.len());
            self.error(message, span);
            return None;
        };
        let map = self.writable_place(map, slice_deref, base);
        let key = self
            .expr(key, Some(key_ty))
            .and_then(|k| self.coerce(k, key_ty));
        let value = self
            .expr(value, Some(value_ty))
            .and_then(|v| self.coerce(v, value_ty));
        Some(hir::StmtKind::MapAssign {
            map: map?,
            key: key?,
            value: value?,
        })
    }

    fn assign(
        &mut self,
        targets: &[AssignTarget],
        op: AssignOp,
        values: &[ast::Expr],
        span: Span,
    ) -> Option<hir::StmtKind> {
        if let [AssignTarget::Place(target)] = targets {
            match self.map_entry_target(target) {
                Some(Some((map, slice_deref, base, key))) => {
                    return self.map_assign(map, slice_deref, base, key, op, values, span);
                }
                Some(None) => {
                    self.report_arg_errors(values);
                    return None;
                }
                None => {}
            }
        }
        // `None` is a discard; `Some(None)` is a target that failed to check.
        let places: Vec<Option<Option<hir::Place>>> = targets
            .iter()
            .map(|t| match t {
                AssignTarget::Place(expr) => Some(self.assignable_place(expr)),
                AssignTarget::Discard(_) => None,
            })
            .collect();
        if let AssignOp::Compound(op) = op {
            let place = places.into_iter().next().flatten().flatten();
            let value = self.expr(&values[0], place.as_ref().map(|p| p.ty));
            return self.compound(place?, op, value?, span);
        }
        let place_types: Vec<Option<Option<TypeId>>> = places
            .iter()
            .map(|p| p.as_ref().map(|p| p.as_ref().map(|p| p.ty)))
            .collect();
        let mut checked = Vec::new();
        let mut ok = places.iter().all(|p| !matches!(p, Some(None)));
        if values.len() == 1 && targets.len() > 1 {
            match self.expr(&values[0], None) {
                Some(Value::Typed(expr)) if expr.types.len() == targets.len() => {
                    for (ty, target) in expr.types.iter().zip(&place_types) {
                        if let Some(Some(target)) = target
                            && target != ty
                        {
                            self.mismatch(*target, *ty, expr.span);
                            ok = false;
                        }
                    }
                    checked.push(expr);
                }
                Some(other) => {
                    let found = match &other {
                        Value::Typed(expr) => expr.types.len(),
                        Value::Untyped(..) => 1,
                    };
                    let message = format!(
                        "assignment has {} targets but the value provides {found}",
                        targets.len()
                    );
                    self.error(message, other.span());
                    return None;
                }
                None => return None,
            }
        } else if values.len() != targets.len() {
            let message = format!(
                "assignment has {} target{} but {} value{}",
                targets.len(),
                if targets.len() == 1 { "" } else { "s" },
                values.len(),
                if values.len() == 1 { "" } else { "s" },
            );
            self.error(message, span);
            return None;
        } else {
            for (value, target) in values.iter().zip(&place_types) {
                let expected = target.flatten();
                let value = self.expr(value, expected);
                let expr = match (value, target) {
                    (Some(value), Some(Some(ty))) => self.coerce(value, *ty),
                    (Some(value), None) => self.with_default_type(value),
                    _ => None,
                };
                match expr {
                    Some(expr) => checked.push(expr),
                    None => ok = false,
                }
            }
        }
        // Targets must be provably disjoint.
        let concrete: Vec<&hir::Place> =
            places.iter().filter_map(|p| p.as_ref()?.as_ref()).collect();
        for (i, a) in concrete.iter().enumerate() {
            for b in &concrete[i + 1..] {
                let shared = a.projections.len().min(b.projections.len());
                let prefix_matches = a.projections[..shared]
                    .iter()
                    .zip(&b.projections[..shared])
                    .all(|(x, y)| projections_conservatively_equal(x, y));
                if a.root == b.root && prefix_matches {
                    self.diagnostics.push(
                        Diagnostic::new(Severity::Error, "assignment targets overlap", b.span)
                            .related(a.span, "overlaps this target"),
                    );
                    ok = false;
                }
            }
        }
        ok.then(|| hir::StmtKind::Assign {
            targets: places.into_iter().map(|p| p.flatten()).collect(),
            values: checked,
        })
    }

    fn compound(
        &mut self,
        place: hir::Place,
        op: BinaryOp,
        value: Value,
        span: Span,
    ) -> Option<hir::StmtKind> {
        let ty = place.ty;
        if !self.operator_applies(op, ty) {
            let message = format!(
                "operator `{}=` cannot be applied to `{}`",
                op_str(op),
                self.name(ty)
            );
            self.error(message, span);
            return None;
        }
        if let Some(int) = self.types.int(ty)
            && matches!(op, BinaryOp::Shl | BinaryOp::Shr)
        {
            let count = match value {
                Value::Untyped(value, count_span) => {
                    let n = value.to_integer().and_then(|n| n.to_i128());
                    match n.filter(|n| (0..i128::from(int.bits)).contains(n)) {
                        Some(n) => {
                            typed(ExprKind::Const(Const::Int(n)), TypeStore::INT, count_span)
                        }
                        None => {
                            let message = format!(
                                "shift count `{}` is out of range for `{}`",
                                value.describe(),
                                self.name(ty)
                            );
                            self.error(message, count_span);
                            return None;
                        }
                    }
                }
                Value::Typed(expr) => self.single_value(expr)?,
            };
            if self.types.int(count.ty()).is_none() {
                let message = format!(
                    "shift count must be an integer, found `{}`",
                    self.name(count.ty())
                );
                self.error(message, count.span);
                return None;
            }
            if let Some(&Const::Int(n)) = constant(&count)
                && !(0..i128::from(int.bits)).contains(&n)
            {
                let message = format!("shift count `{n}` is out of range for `{}`", self.name(ty));
                self.error(message, count.span);
                return None;
            }
            return Some(hir::StmtKind::CompoundAssign {
                place,
                op,
                value: count,
            });
        }
        // A zero divisor here panics at runtime; the dividend is not constant.
        let value = self.coerce(value, ty)?;
        Some(hir::StmtKind::CompoundAssign { place, op, value })
    }

    fn return_stmt(&mut self, values: &[ast::Expr], span: Span) -> Option<hir::StmtKind> {
        let results = self.results.clone();
        if values.is_empty() {
            if !results.is_empty() {
                self.error(
                    "this function must return a value; a bare `return` is not allowed",
                    span,
                );
                return None;
            }
            return Some(hir::StmtKind::Return(Vec::new()));
        }
        if results.is_empty() {
            self.error("this function does not return a value", values[0].span);
            self.report_arg_errors(values);
            return None;
        }
        if let [value] = values
            && is_try(value)
            && results.last() == Some(&TypeStore::ERROR)
        {
            return match self.expr(value, None)? {
                Value::Typed(expr) if expr.types == results[..results.len() - 1] => {
                    Some(hir::StmtKind::Return(vec![expr]))
                }
                other => {
                    self.error(
                        "`?` result does not match this function's non-error results",
                        other.span(),
                    );
                    None
                }
            };
        }
        if values.len() == 1 && results.len() > 1 {
            // Forward all results of one call.
            return match self.expr(&values[0], None)? {
                Value::Typed(expr) if expr.types == results => {
                    Some(hir::StmtKind::Return(vec![expr]))
                }
                other => {
                    let message = format!("expected {} return values", results.len());
                    self.error(message, other.span());
                    None
                }
            };
        }
        if values.len() != results.len() {
            let message = format!(
                "expected {} return value{}, found {}",
                results.len(),
                if results.len() == 1 { "" } else { "s" },
                values.len()
            );
            self.error(message, span);
            self.report_arg_errors(values);
            return None;
        }
        let mut checked = Vec::new();
        for (value, &ty) in values.iter().zip(&results) {
            checked.push(self.expr(value, Some(ty)).and_then(|v| self.coerce(v, ty)));
        }
        checked
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .map(hir::StmtKind::Return)
    }
}

struct SliceDeref {
    mutable: bool,
    base_span: Span,
}

struct ArgumentAccess {
    mutable_place: bool,
    exclusive: bool,
    borrowing: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum MutableUse {
    Argument,
    Slice,
}

/// In evaluation order.
fn subexpressions(expr: &hir::Expr) -> Vec<&hir::Expr> {
    match &expr.kind {
        ExprKind::Const(_) | ExprKind::Local(_) | ExprKind::Closure { .. } => Vec::new(),
        ExprKind::Field { base, .. } => vec![base],
        ExprKind::Index { base, index } => vec![base, index],
        ExprKind::Slice {
            base, low, high, ..
        } => std::iter::once(&**base)
            .chain(low.as_deref())
            .chain(high.as_deref())
            .collect(),
        ExprKind::Call { args, .. } => args.iter().collect(),
        ExprKind::CallValue { callee, args } => std::iter::once(&**callee).chain(args).collect(),
        ExprKind::StructLit { fields, .. } => fields.iter().map(|(_, value)| value).collect(),
        ExprKind::ArrayLit { elements, .. } => elements.iter().collect(),
        ExprKind::MapLit { entries, .. } => entries
            .iter()
            .flat_map(|(key, value)| [key, value])
            .collect(),
        ExprKind::MapLookup { map, key } | ExprKind::MapRemove { map, key } => vec![map, key],
        ExprKind::Println(inner)
        | ExprKind::Drop(inner)
        | ExprKind::Convert(inner)
        | ExprKind::Clone(inner)
        | ExprKind::Error(inner)
        | ExprKind::Try(inner)
        | ExprKind::Unary { operand: inner, .. } => vec![inner],
        ExprKind::Binary { lhs, rhs, .. } => vec![lhs, rhs],
    }
}

/// Index values are ignored: two indices never prove disjointness.
#[derive(PartialEq)]
enum ArgumentProjection {
    Field(FieldId),
    Index,
}

type ArgumentPlace = (LocalId, Vec<ArgumentProjection>);

fn argument_place(expr: &hir::Expr) -> Option<ArgumentPlace> {
    match &expr.kind {
        ExprKind::Local(id) => Some((*id, Vec::new())),
        ExprKind::Field { base, field } => {
            let (root, mut path) = argument_place(base)?;
            path.push(ArgumentProjection::Field(*field));
            Some((root, path))
        }
        ExprKind::Index { base, .. } => {
            let (root, mut path) = argument_place(base)?;
            path.push(ArgumentProjection::Index);
            Some((root, path))
        }
        _ => None,
    }
}

/// Two indices never prove disjointness.
fn projections_conservatively_equal(a: &hir::Projection, b: &hir::Projection) -> bool {
    match (a, b) {
        (hir::Projection::Field(x), hir::Projection::Field(y)) => x == y,
        (hir::Projection::Index(_), hir::Projection::Index(_)) => true,
        _ => false,
    }
}

fn places_overlap(a: &ArgumentPlace, b: &ArgumentPlace) -> bool {
    a.0 == b.0
        && a.1.iter().zip(&b.1).all(|(x, y)| match (x, y) {
            (ArgumentProjection::Field(x), ArgumentProjection::Field(y)) => x == y,
            (ArgumentProjection::Index, ArgumentProjection::Index) => true,
            _ => false,
        })
}

fn shift_value(int: IntType, value: i128, count: u32, op: BinaryOp) -> i128 {
    if op == BinaryOp::Shl {
        int.wrap(((value as u128) << count) as i128)
    } else {
        value >> count
    }
}

fn block_always_exits(block: &ast::Block) -> bool {
    block.stmts.iter().any(stmt_always_exits)
}

fn stmt_always_exits(stmt: &ast::Stmt) -> bool {
    match &stmt.kind {
        ast::StmtKind::Return(_) => true,
        ast::StmtKind::Block(block) => block_always_exits(block),
        ast::StmtKind::If(if_stmt) => if_always_exits(if_stmt),
        ast::StmtKind::For(for_stmt) => {
            matches!(for_stmt.header, ForHeader::Infinite) && !contains_break(&for_stmt.body)
        }
        _ => false,
    }
}

fn if_always_exits(if_stmt: &ast::If) -> bool {
    block_always_exits(&if_stmt.then_block)
        && match &if_stmt.else_branch {
            None => false,
            Some(ast::Else::Block(block)) => block_always_exits(block),
            Some(ast::Else::If(inner)) => if_always_exits(inner),
        }
}

fn contains_break(block: &ast::Block) -> bool {
    block.stmts.iter().any(|stmt| match &stmt.kind {
        ast::StmtKind::Break => true,
        ast::StmtKind::Block(block) => contains_break(block),
        ast::StmtKind::If(if_stmt) => if_contains_break(if_stmt),
        _ => false,
    })
}

fn if_contains_break(if_stmt: &ast::If) -> bool {
    contains_break(&if_stmt.then_block)
        || match &if_stmt.else_branch {
            None => false,
            Some(ast::Else::Block(block)) => contains_break(block),
            Some(ast::Else::If(inner)) => if_contains_break(inner),
        }
}
