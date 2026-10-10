use super::{Checker, MutableUse, Value, typed};
use crate::ast::{self, ParamMode};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, ExprKind, StmtKind};
use crate::resolve::{FunctionId, LocalId, LocalKind};
use crate::source::Span;
use crate::types::{FuncSignature, TypeKind};

impl Checker<'_> {
    pub(super) fn method_value(
        &mut self,
        receiver: hir::Expr,
        method: FunctionId,
        name: &ast::Name,
        span: Span,
    ) -> Option<Value> {
        let ty = receiver.ty();
        let strukt = self.types.struct_id(ty)?;
        if !self.can_use_member(self.res.struct_package[strukt.0 as usize], &name.text) {
            self.unexported_member("method", &name.text, ty, name.span);
            return None;
        }
        let declaration = self.res.functions[method.0 as usize];
        let problem = if name.text == "drop" {
            Some("the `drop` method cannot be used as a value")
        } else if declaration.is_async {
            Some("an `async` method cannot be used as a value")
        } else {
            None
        };
        if let Some(problem) = problem {
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, problem, name.span)
                    .note("call it directly, or wrap the call in a function literal"),
            );
            return None;
        }
        let Some(root) = receiver_root(&receiver) else {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "the receiver of a method value must be a local or a field of a local",
                    receiver.span,
                )
                .note("bind the receiver to a local first"),
            );
            return None;
        };
        let receiver_mode = declaration
            .receiver
            .as_ref()
            .map_or(ParamMode::Borrow, |param| param.mode);
        if receiver_mode == ParamMode::Mut && !self.mutable_place(&receiver, MutableUse::Argument) {
            return None;
        }
        let root_ty = self.locals[root.0 as usize]?;
        let signature = self.signatures[method.0 as usize].as_ref()?;
        let params: Vec<_> = signature.params[1..].to_vec();
        let results = signature.results.clone();
        let modes: Vec<ParamMode> = declaration.params.iter().map(|param| param.mode).collect();
        let exclusive = receiver_mode == ParamMode::Mut
            || self.type_contains(root_ty, &|kind| {
                matches!(
                    kind,
                    TypeKind::Func(_) | TypeKind::Slice { mutable: true, .. }
                )
            });
        let ty = self.types.func_type(FuncSignature {
            is_async: false,
            params: modes.iter().copied().zip(params.iter().copied()).collect(),
            results: results.clone(),
        });
        let function = self.method_closure(
            method, &receiver, root, root_ty, &params, &modes, results, span,
        );
        Some(Value::Typed(typed(
            ExprKind::Closure {
                function,
                captures: vec![(root, exclusive)],
                owning: false,
            },
            ty,
            span,
        )))
    }

    /// A closure over the receiver's root local that calls the method with its own parameters.
    #[allow(clippy::too_many_arguments)]
    fn method_closure(
        &mut self,
        method: FunctionId,
        receiver: &hir::Expr,
        root: LocalId,
        root_ty: crate::types::TypeId,
        params: &[crate::types::TypeId],
        modes: &[ParamMode],
        results: Vec<crate::types::TypeId>,
        span: Span,
    ) -> FunctionId {
        let id = FunctionId(
            (self.res.functions.len() + self.closures.len() + self.generated_functions.len())
                as u32,
        );
        let capture = LocalId(params.len() as u32);
        let mut locals: Vec<hir::Local> = params
            .iter()
            .zip(modes)
            .enumerate()
            .map(|(index, (&ty, &mode))| hir::Local {
                name: format!("arg{index}"),
                ty,
                kind: LocalKind::Param(mode),
                span,
            })
            .collect();
        locals.push(hir::Local {
            name: self.res.locals[self.current][root.0 as usize].name.clone(),
            ty: root_ty,
            kind: LocalKind::Capture(root),
            span,
        });
        let mut args = vec![rebased(receiver, capture)];
        args.extend(
            params
                .iter()
                .enumerate()
                .map(|(index, &ty)| typed(ExprKind::Local(LocalId(index as u32)), ty, span)),
        );
        let call = hir::Expr {
            kind: ExprKind::Call {
                function: method,
                args,
            },
            types: results.clone(),
            span,
        };
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
        let call_once = self.consumes_capture(&body, &[capture]);
        let name = format!(
            "{}$method{}",
            self.function_name(FunctionId(self.current as u32)),
            self.generated_functions.len()
        );
        self.generated_functions.push(hir::Function {
            name,
            span,
            params: (0..params.len()).map(|i| LocalId(i as u32)).collect(),
            results,
            captures: vec![capture],
            is_closure: true,
            is_async: false,
            native: false,
            call_once,
            type_params: Vec::new(),
            probe: false,
            locals,
            body,
        });
        id
    }
}

fn receiver_root(expr: &hir::Expr) -> Option<LocalId> {
    match &expr.kind {
        ExprKind::Local(local) => Some(*local),
        ExprKind::Field { base, .. } => receiver_root(base),
        _ => None,
    }
}

fn rebased(expr: &hir::Expr, capture: LocalId) -> hir::Expr {
    let mut copy = expr.clone();
    let mut node = &mut copy;
    loop {
        match &mut node.kind {
            ExprKind::Local(local) => {
                *local = capture;
                break;
            }
            ExprKind::Field { base, .. } => node = base,
            _ => break,
        }
    }
    copy
}
