use super::{Checker, MutableUse, Value};
use crate::ast::{self, ParamMode};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, ExprKind, Implementation};
use crate::source::Span;
use crate::types::{FuncSignature, InterfaceId, InterfaceMethod, TypeId, TypeKind};

/// Why a type does not satisfy an interface, as the reason clause of a diagnostic.
struct Unsatisfied {
    reason: String,
    note: Option<String>,
}

fn mode_word(mode: ParamMode) -> &'static str {
    match mode {
        ParamMode::Borrow => "a shared",
        ParamMode::Mut => "a `mut`",
        ParamMode::Own => "an `own`",
    }
}

/// Holding the value exclusively, or owning it, is enough to make any weaker call.
fn receiver_serves(entry: ParamMode, method: ParamMode) -> bool {
    match entry {
        ParamMode::Borrow => method == ParamMode::Borrow,
        ParamMode::Mut => method != ParamMode::Own,
        ParamMode::Own => true,
    }
}

impl Checker<'_> {
    pub(super) fn interface_entries(&mut self) {
        let interfaces = self.res.interfaces.clone();
        for (decl, _, ty) in interfaces {
            let Some(id) = self.types.interface_of(ty) else {
                continue;
            };
            let mut entries = Vec::new();
            for method in &decl.methods {
                let params: Vec<Option<(ParamMode, TypeId)>> = method
                    .params
                    .iter()
                    .map(|param| self.param_type(param).map(|ty| (param.mode, ty)))
                    .collect();
                let results = self.closure_results(&method.results);
                let (Some(params), Some(results)) =
                    (params.into_iter().collect::<Option<Vec<_>>>(), results)
                else {
                    continue;
                };
                entries.push(InterfaceMethod {
                    name: method.name.text.clone(),
                    receiver: method.receiver,
                    is_async: method.is_async,
                    params,
                    results,
                });
            }
            self.types.set_interface_methods(id, entries);
        }
    }

    fn interface_package(&self, id: InterfaceId) -> usize {
        let ty = self.types.interface_type(id);
        self.res
            .interfaces
            .iter()
            .find(|(_, _, declared)| *declared == ty)
            .map_or(self.res.entry_package, |&(_, package, _)| package)
    }

    fn signature_text(&mut self, params: &[(ParamMode, TypeId)], results: &[TypeId]) -> String {
        let ty = self.types.func_type(FuncSignature {
            is_async: false,
            params: params.to_vec(),
            results: results.to_vec(),
        });
        self.name(ty)
    }

    /// The method serving each entry of `interface`, or why `source` has none.
    fn satisfaction(
        &mut self,
        source: TypeId,
        interface: InterfaceId,
    ) -> Result<Vec<Implementation>, Unsatisfied> {
        let entries = self.types.interface_methods(interface).to_vec();
        let home = self.interface_package(interface);
        let mut served = Vec::new();
        for entry in &entries {
            let exported = entry.name.starts_with(|c: char| c.is_ascii_uppercase());
            let (implementation, receiver, is_async, params, results, package) =
                match self.types.interface_of(source) {
                    Some(other) => {
                        let methods = self.types.interface_methods(other);
                        let Some(index) = methods.iter().position(|m| m.name == entry.name) else {
                            return Err(Unsatisfied {
                                reason: format!("it has no method `{}`", entry.name),
                                note: None,
                            });
                        };
                        let method = methods[index].clone();
                        (
                            Implementation::Entry(index),
                            method.receiver,
                            method.is_async,
                            method.params,
                            method.results,
                            self.interface_package(other),
                        )
                    }
                    None => {
                        let Some(id) = self.method_of(source, &entry.name) else {
                            return Err(Unsatisfied {
                                reason: format!("it has no method `{}`", entry.name),
                                note: None,
                            });
                        };
                        let declaration = self.res.functions[id.0 as usize];
                        let Some(signature) = &self.signatures[id.0 as usize] else {
                            return Err(Unsatisfied {
                                reason: format!("method `{}` has errors", entry.name),
                                note: None,
                            });
                        };
                        let (signature_params, signature_results) =
                            (signature.params.clone(), signature.results.clone());
                        let bound = self.receiver_type_arguments(id, &signature_params[0], source);
                        let params = declaration
                            .params
                            .iter()
                            .map(|param| param.mode)
                            .zip(signature_params[1..].iter().copied())
                            .map(|(mode, ty)| (mode, self.types.substitute(ty, &bound)))
                            .collect();
                        let results = signature_results
                            .iter()
                            .map(|&ty| self.types.substitute(ty, &bound))
                            .collect();
                        let receiver = declaration
                            .receiver
                            .as_ref()
                            .map_or(ParamMode::Borrow, |param| param.mode);
                        (
                            Implementation::Method(id),
                            receiver,
                            declaration.is_async,
                            params,
                            results,
                            self.package_of(id.0 as usize),
                        )
                    }
                };
            if !exported && package != home {
                return Err(Unsatisfied {
                    reason: format!(
                        "its method `{}` is unexported and declared outside the interface's package",
                        entry.name
                    ),
                    note: Some(
                        "an unexported interface method can only be satisfied in the interface's package"
                            .to_string(),
                    ),
                });
            }
            if params != entry.params || results != entry.results {
                let wanted = self.signature_text(&entry.params, &entry.results);
                let found = self.signature_text(&params, &results);
                return Err(Unsatisfied {
                    reason: format!("method `{}` has a different signature", entry.name),
                    note: Some(format!(
                        "the interface needs `{wanted}`, and the method is `{found}`, ignoring the receiver"
                    )),
                });
            }
            if is_async != entry.is_async {
                let reason = if entry.is_async {
                    format!("method `{}` is not `async`", entry.name)
                } else {
                    format!("method `{}` is `async`", entry.name)
                };
                return Err(Unsatisfied { reason, note: None });
            }
            if !receiver_serves(entry.receiver, receiver) {
                return Err(Unsatisfied {
                    reason: format!(
                        "method `{}` has {} receiver, but the interface calls it with {} receiver",
                        entry.name,
                        mode_word(receiver),
                        mode_word(entry.receiver)
                    ),
                    note: Some(
                        "a method satisfies an entry when its receiver needs no more access than the entry gives"
                            .to_string(),
                    ),
                });
            }
            served.push(implementation);
        }
        Ok(served)
    }

    /// Records what serves each entry, or reports why nothing does.
    pub(super) fn require_satisfied(
        &mut self,
        source: TypeId,
        interface: InterfaceId,
        span: Span,
    ) -> bool {
        let key = match self.types.kind(source) {
            TypeKind::InterfaceView { interface, .. } => self.types.interface_type(interface),
            _ => source,
        };
        if self.implementations.contains_key(&(key, interface)) {
            return true;
        }
        match self.satisfaction(key, interface) {
            Ok(served) => {
                self.implementations.insert((key, interface), served);
                true
            }
            Err(unsatisfied) => {
                let target = self.types.interface_type(interface);
                let message = format!(
                    "`{}` does not satisfy `{}`: {}",
                    self.name(key),
                    self.name(target),
                    unsatisfied.reason
                );
                let mut diagnostic = Diagnostic::new(Severity::Error, message, span);
                if let Some(note) = unsatisfied.note {
                    diagnostic = diagnostic.note(note);
                }
                self.diagnostics.push(diagnostic);
                false
            }
        }
    }

    /// Gives `expr` back unchanged when `target` is not an interface type.
    pub(super) fn interface_conversion(
        &mut self,
        expr: hir::Expr,
        target: TypeId,
    ) -> Result<Option<hir::Expr>, hir::Expr> {
        let source = expr.ty();
        let span = expr.span;
        if self.types.interface_of(target).is_some() && self.types.mentions_param(source) {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!(
                        "converting a value of type `{}`, which uses a type parameter, to an interface type is not supported yet",
                        self.name(source)
                    ),
                    span,
                )
                .note("call the constraint's methods on the value directly"),
            );
            return Ok(None);
        }
        match self.types.kind(target) {
            TypeKind::Interface(interface) => {
                if let TypeKind::InterfaceView { .. } = self.types.kind(source) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "a borrowed interface value cannot become an owned `{}`",
                                self.name(target)
                            ),
                            span,
                        )
                        .note("a borrowed parameter does not own the value inside it; pass it on to another borrowed parameter instead"),
                    );
                    return Ok(None);
                }
                if !self.require_satisfied(source, interface, span) {
                    return Ok(None);
                }
                if self.type_contains(source, &|kind| {
                    matches!(
                        kind,
                        TypeKind::Slice { .. } | TypeKind::Func(_) | TypeKind::InterfaceView { .. }
                    )
                }) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "`{}` holds a view, so it cannot become an owned `{}`",
                                self.name(source),
                                self.name(target)
                            ),
                            span,
                        )
                        .note("an owned interface value cannot hold a slice, a function value, or a borrowed interface value"),
                    );
                    return Ok(None);
                }
                Ok(Some(hir::Expr {
                    kind: ExprKind::InterfaceBox(Box::new(expr)),
                    types: vec![target],
                    span,
                }))
            }
            TypeKind::InterfaceView { interface, mutable } => {
                if let TypeKind::InterfaceView {
                    mutable: inner_mutable,
                    ..
                } = self.types.kind(source)
                    && mutable
                    && !inner_mutable
                {
                    self.diagnostics.push(
                        Diagnostic::new(
                            Severity::Error,
                            "a shared borrowed interface value cannot be passed as `mut`",
                            span,
                        )
                        .note("declare the parameter it came from `mut`"),
                    );
                    return Ok(None);
                }
                let same = self.types.interface_of(source) == Some(interface);
                if !same && !self.require_satisfied(source, interface, span) {
                    return Ok(None);
                }
                Ok(Some(hir::Expr {
                    kind: ExprKind::InterfaceView {
                        source: Box::new(expr),
                        mutable,
                    },
                    types: vec![target],
                    span,
                }))
            }
            _ => Err(expr),
        }
    }

    pub(super) fn interface_method_value(&mut self, name: &ast::Name) -> Option<Value> {
        self.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!(
                    "method `{}` of an interface value cannot be used as a value",
                    name.text
                ),
                name.span,
            )
            .note("call it, or wrap the call in a function literal"),
        );
        None
    }

    pub(super) fn interface_call(
        &mut self,
        receiver: hir::Expr,
        name: &ast::Name,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let ty = receiver.ty();
        let interface = self.types.interface_of(ty)?;
        let entries = self.types.interface_methods(interface).to_vec();
        let Some(method) = entries.iter().position(|entry| entry.name == name.text) else {
            let message = format!(
                "interface `{}` has no method `{}`",
                self.name(self.types.interface_type(interface)),
                name.text
            );
            self.error(message, name.span);
            self.report_arg_errors(args);
            return None;
        };
        let entry = entries[method].clone();
        if !self.can_use_member(self.interface_package(interface), &name.text) {
            self.unexported_member("method", &name.text, ty, name.span);
            self.report_arg_errors(args);
            return None;
        }
        if entry.is_async && self.awaited_call != Some(span) {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("call to async method `{}` is not awaited", name.text),
                    span,
                )
                .note("write `await` before the call inside an `async func`"),
            );
            self.report_arg_errors(args);
            return None;
        }
        let receiver_ok = match (self.types.kind(ty), entry.receiver) {
            (TypeKind::InterfaceView { .. }, ParamMode::Own) => {
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{}` consumes its receiver, but a borrowed interface value does not own the value inside",
                            name.text
                        ),
                        span,
                    )
                    .note("take the interface value as an `own` parameter to call this method"),
                );
                false
            }
            (TypeKind::InterfaceView { mutable: false, .. }, ParamMode::Mut) => {
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{}` needs mutable access, but this borrowed interface value is shared",
                            name.text
                        ),
                        span,
                    )
                    .note("declare the parameter `mut`"),
                );
                false
            }
            (TypeKind::Interface(_), ParamMode::Mut) => {
                self.mutable_place(&receiver, MutableUse::Argument)
            }
            _ => true,
        };
        if args.len() != entry.params.len() {
            let message = format!(
                "`{}` takes {} argument{} but {} {} given",
                name.text,
                entry.params.len(),
                if entry.params.len() == 1 { "" } else { "s" },
                args.len(),
                if args.len() == 1 { "was" } else { "were" },
            );
            self.error(message, span);
            self.report_arg_errors(args);
            return None;
        }
        let mut checked = Vec::new();
        let mut ok = receiver_ok;
        for (arg, &(_, param)) in args.iter().zip(&entry.params) {
            match self
                .expr(arg, Some(param))
                .and_then(|v| self.coerce(v, param))
            {
                Some(expr) => checked.push(expr),
                None => ok = false,
            }
        }
        let accesses: Vec<_> = entry
            .params
            .iter()
            .map(|&(mode, ty)| self.argument_access(mode, ty))
            .collect();
        if !ok || !self.check_argument_accesses(&accesses, &checked) {
            return None;
        }
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::InterfaceCall {
                receiver: Box::new(receiver),
                method,
                args: checked,
            },
            types: entry.results,
            span,
        }))
    }
}
