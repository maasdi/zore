use crate::ast::ParamMode;
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, Const, Expr, ExprKind, StmtKind};
use crate::resolve::{FieldId, FunctionId, LocalId, LocalKind};
use crate::source::Span;
use crate::types::{FuncSignature, InterfaceId, TypeId, TypeKind, TypeStore};

pub(super) fn adapt(package: &mut hir::Package) -> Vec<Diagnostic> {
    let Some(hook) = package
        .functions
        .iter()
        .position(|f| f.name == "std/net/http.RouterBuilder.Handle")
    else {
        return Vec::new();
    };
    let contract = match HandlerContract::resolve(package, hook) {
        Ok(contract) => contract,
        Err(diagnostic) => return vec![*diagnostic],
    };
    let HandlerContract {
        registration,
        factory_type,
        factory_interface,
        handler_type,
        handler_interface,
    } = contract;
    let mut diagnostics = Vec::new();
    package.functions[hook].probe = true;
    let original_count = package.functions.len();
    for index in 0..original_count {
        if !package.functions[index].compiled() {
            continue;
        }
        let locals = package.functions[index].locals.clone();
        let span = package.functions[index].body.span;
        let mut body = std::mem::replace(
            &mut package.functions[index].body,
            hir::Block {
                stmts: Vec::new(),
                span,
            },
        );
        super::generic::each_expr(&mut body, &mut |expr| {
            let ExprKind::Call { function, args } = &mut expr.kind else {
                return;
            };
            if function.0 as usize != hook {
                return;
            }
            let Some(callback_argument) = args.get(2) else {
                diagnostics.push(contract_error("RouterBuilder.Handle call", expr.span));
                return;
            };
            let ExprKind::Closure {
                function: callback,
                captures,
                ..
            } = &callback_argument.kind
            else {
                diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        "HTTP registration needs a declared function name or inline function literal",
                        args[2].span,
                    ).note("pass the function name or literal directly; use HandleFactory for per-request state"),
                );
                return;
            };
            let source = package.function(*callback);
            if source.call_once || source.is_async {
                diagnostics.push(Diagnostic::new(
                    Severity::Error,
                    "HTTP handlers must be synchronous and reusable",
                    args[2].span,
                ));
                return;
            }
            for &(local, exclusive) in captures {
                let captured = &locals[local.0 as usize];
                if exclusive || !package.is_copy(captured.ty) || package.contains_view(captured.ty)
                {
                    diagnostics.push(Diagnostic::new(Severity::Error, format!("HTTP handler capture `{}` must be a shared-read Copy value without views or functions", captured.name), args[2].span).note("share mutable state through a Mutex or channel, or use HandleFactory"));
                    return;
                }
            }
            let callback = *callback;
            let captures = captures.clone();
            let callable_type = args[2].ty();
            let (strukt, adapter_type) = package
                .types
                .add_struct(&format!("http$handler{}", package.structs.len()));
            package.structs.push(hir::Struct {
                name: package.types.display(adapter_type).to_string(),
                span: expr.span,
                fields: captures
                    .iter()
                    .map(|&(local, _)| hir::Field {
                        name: locals[local.0 as usize].name.clone(),
                        ty: locals[local.0 as usize].ty,
                        span: expr.span,
                    })
                    .collect(),
                drop: None,
                clone: None,
                generic: false,
            });
            let constructor = FunctionId(package.functions.len() as u32);
            let receiver = local(0, adapter_type, expr.span);
            package.functions.push(make_function(
                format!("http$factory{}", constructor.0),
                vec![parameter(
                    "self",
                    adapter_type,
                    ParamMode::Borrow,
                    expr.span,
                )],
                vec![handler_type, TypeStore::ERROR],
                vec![statement(
                    StmtKind::Return(vec![
                        typed(
                            ExprKind::InterfaceBox(Box::new(receiver.clone())),
                            handler_type,
                            expr.span,
                        ),
                        typed(ExprKind::Const(Const::Nil), TypeStore::ERROR, expr.span),
                    ]),
                    expr.span,
                )],
                expr.span,
            ));
            let invoke = FunctionId(package.functions.len() as u32);
            let Some(signature) = package.types.func_signature(callable_type).cloned() else {
                diagnostics.push(contract_error("handler callback", expr.span));
                return;
            };
            let mut invocation_locals = vec![parameter(
                "self",
                adapter_type,
                ParamMode::Borrow,
                expr.span,
            )];
            invocation_locals.extend(
                signature
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, &(mode, ty))| parameter(&format!("arg{i}"), ty, mode, expr.span)),
            );
            let mut statements = Vec::new();
            let mut invocation_captures = Vec::new();
            for (field, &(captured, _)) in captures.iter().enumerate() {
                let id = LocalId(invocation_locals.len() as u32);
                let original = &locals[captured.0 as usize];
                invocation_locals.push(hir::Local {
                    name: original.name.clone(),
                    ty: original.ty,
                    kind: LocalKind::Let,
                    span: expr.span,
                });
                statements.push(statement(
                    StmtKind::Let {
                        targets: vec![Some(id)],
                        value: typed(
                            ExprKind::Field {
                                base: Box::new(receiver.clone()),
                                field: FieldId(field as u32),
                            },
                            original.ty,
                            expr.span,
                        ),
                    },
                    expr.span,
                ));
                invocation_captures.push((id, false));
            }
            let callee = typed(
                ExprKind::Closure {
                    function: callback,
                    captures: invocation_captures,
                    owning: true,
                    capture_modes: Vec::new(),
                },
                callable_type,
                expr.span,
            );
            let arguments = signature
                .params
                .iter()
                .enumerate()
                .map(|(i, &(_, ty))| local((i + 1) as u32, ty, expr.span))
                .collect();
            statements.push(statement(
                StmtKind::Return(vec![Expr {
                    kind: ExprKind::CallValue {
                        callee: Box::new(callee),
                        args: arguments,
                        once: false,
                    },
                    types: signature.results.clone(),
                    span: expr.span,
                }]),
                expr.span,
            ));
            let parameter_count = signature.params.len() + 1;
            let mut implementation = make_function(
                format!("http$invoke{}", invoke.0),
                invocation_locals,
                signature.results,
                statements,
                expr.span,
            );
            implementation.params = (0..parameter_count).map(|i| LocalId(i as u32)).collect();
            package.functions.push(implementation);
            package.implementations.insert(
                (adapter_type, factory_interface),
                vec![hir::Implementation::Method(constructor)],
            );
            package.implementations.insert(
                (adapter_type, handler_interface),
                vec![hir::Implementation::Method(invoke)],
            );
            let fields = captures
                .iter()
                .enumerate()
                .map(|(i, &(capture, _))| {
                    (
                        FieldId(i as u32),
                        local(capture.0, locals[capture.0 as usize].ty, args[2].span),
                    )
                })
                .collect();
            let value = typed(
                ExprKind::StructLit { strukt, fields },
                adapter_type,
                args[2].span,
            );
            args[2] = typed(
                ExprKind::InterfaceBox(Box::new(value)),
                factory_type,
                args[2].span,
            );
            *function = FunctionId(registration as u32);
        });
        package.functions[index].body = body;
    }
    diagnostics
}

fn typed(kind: ExprKind, ty: TypeId, span: Span) -> Expr {
    Expr {
        kind,
        types: vec![ty],
        span,
    }
}
fn local(id: u32, ty: TypeId, span: Span) -> Expr {
    typed(ExprKind::Local(LocalId(id)), ty, span)
}
fn parameter(name: &str, ty: TypeId, mode: ParamMode, span: Span) -> hir::Local {
    hir::Local {
        name: name.into(),
        ty,
        kind: LocalKind::Param(mode),
        span,
    }
}
fn statement(kind: StmtKind, span: Span) -> hir::Stmt {
    hir::Stmt { kind, span }
}
fn make_function(
    name: String,
    locals: Vec<hir::Local>,
    results: Vec<TypeId>,
    stmts: Vec<hir::Stmt>,
    span: Span,
) -> hir::Function {
    hir::Function {
        name,
        span,
        params: (0..locals.len()).map(|i| LocalId(i as u32)).collect(),
        results,
        captures: Vec::new(),
        is_closure: false,
        is_async: false,
        native: false,
        native_symbol: None,
        native_wait: None,
        call_once: false,
        type_params: Vec::new(),
        probe: false,
        locals,
        body: hir::Block { stmts, span },
    }
}

struct HandlerContract {
    registration: usize,
    factory_type: TypeId,
    factory_interface: InterfaceId,
    handler_type: TypeId,
    handler_interface: InterfaceId,
}

fn contract_error(member: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        format!("bundled HTTP adapter contract mismatch: {member}"),
        span,
    )
    .note("update the bundled std/net/http declarations and compiler adapter together")
}

impl HandlerContract {
    fn resolve(package: &hir::Package, hook: usize) -> Result<Self, Box<Diagnostic>> {
        let hook = &package.functions[hook];
        let fail = |member: &str| Box::new(contract_error(member, hook.span));
        let registration = package
            .functions
            .iter()
            .position(|function| function.name == "std/net/http.RouterBuilder.register")
            .ok_or_else(|| fail("missing RouterBuilder.register"))?;
        let register = &package.functions[registration];
        let factory_type = parameter_at(register, 2)
            .ok_or_else(|| fail("RouterBuilder.register parameters"))?
            .ty;
        if !matches!(package.types.kind(factory_type), TypeKind::Interface(_))
            || package.types.display(factory_type).to_string() != "http.HandlerFactory"
        {
            return Err(fail("owned HandlerFactory interface"));
        }
        let factory_interface = package
            .types
            .interface_of(factory_type)
            .ok_or_else(|| fail("HandlerFactory interface"))?;
        let [factory] = package.types.interface_methods(factory_interface) else {
            return Err(fail("HandlerFactory needs exactly NewHandler"));
        };
        let [handler_type, error] = factory.results.as_slice() else {
            return Err(fail("HandlerFactory.NewHandler results"));
        };
        if factory.name != "NewHandler"
            || factory.receiver != ParamMode::Borrow
            || factory.is_async
            || !factory.params.is_empty()
            || *error != TypeStore::ERROR
        {
            return Err(fail("HandlerFactory.NewHandler signature"));
        }
        let handler_type = *handler_type;
        if !matches!(package.types.kind(handler_type), TypeKind::Interface(_))
            || package.types.display(handler_type).to_string() != "http.Handler"
        {
            return Err(fail("owned Handler interface"));
        }
        let handler_interface = package
            .types
            .interface_of(handler_type)
            .ok_or_else(|| fail("Handler interface"))?;
        let [handler] = package.types.interface_methods(handler_interface) else {
            return Err(fail("Handler needs exactly Handle"));
        };
        if handler.name != "Handle"
            || handler.receiver != ParamMode::Borrow
            || handler.is_async
            || handler.results != [TypeStore::ERROR]
            || handler.params.len() != 2
            || handler
                .params
                .iter()
                .any(|(mode, _)| *mode != ParamMode::Mut)
            || handler
                .params
                .iter()
                .zip(["ResponseWriter", "Request"])
                .any(|((_, ty), name)| {
                    package.types.display(*ty).to_string() != format!("http.{name}")
                })
        {
            return Err(fail("Handler.Handle signature"));
        }
        let signature = FuncSignature {
            is_async: false,
            params: handler.params.clone(),
            results: handler.results.clone(),
        };
        let Some(callback) = parameter_at(hook, 2) else {
            return Err(fail("RouterBuilder.Handle parameters"));
        };
        if hook.params.len() != 3
            || register.params.len() != 3
            || hook.is_async
            || register.is_async
            || !hook.results.is_empty()
            || !register.results.is_empty()
            || callback.kind != LocalKind::Param(ParamMode::Own)
            || package.types.func_signature(callback.ty) != Some(&signature)
            || parameter_at(register, 2).is_none_or(|p| p.kind != LocalKind::Param(ParamMode::Own))
            || [hook, register].iter().any(|function| {
                parameter_at(function, 0).is_none_or(|p| p.kind != LocalKind::Param(ParamMode::Mut))
                    || parameter_at(function, 1).is_none_or(|p| {
                        p.ty != TypeStore::STRING || p.kind != LocalKind::Param(ParamMode::Borrow)
                    })
            })
            || parameter_at(hook, 0).map(|p| p.ty) != parameter_at(register, 0).map(|p| p.ty)
        {
            return Err(fail("RouterBuilder.Handle/register signature"));
        }
        Ok(Self {
            registration,
            factory_type,
            factory_interface,
            handler_type,
            handler_interface,
        })
    }
}

fn parameter_at(function: &hir::Function, index: usize) -> Option<&hir::Local> {
    function
        .params
        .get(index)
        .and_then(|id| function.locals.get(id.0 as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceMap;

    fn bundled() -> (hir::Package, usize) {
        let mut sources = SourceMap::new();
        let id = sources
            .add(
                "test.ore",
                "package main\nimport \"std/net/http\"\nfunc main() { _ = http.NewRouterBuilder() }\n".into(),
            )
            .unwrap();
        let checked = crate::check::check_file(sources.file(id).unwrap());
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let package = checked.package.unwrap();
        let hook = package
            .functions
            .iter()
            .position(|f| f.name == "std/net/http.RouterBuilder.Handle")
            .unwrap();
        (package, hook)
    }

    #[test]
    fn factory_and_handler_shape_drift_reports_contract_diagnostics() {
        for mutation in 0..8 {
            let (mut package, hook) = bundled();
            let contract =
                HandlerContract::resolve(&package, hook).unwrap_or_else(|d| panic!("{d:?}"));
            let interface = if mutation < 4 {
                contract.factory_interface
            } else {
                contract.handler_interface
            };
            let mut methods = package.types.interface_methods(interface).to_vec();
            match mutation % 4 {
                0 => methods.clear(),
                1 => methods[0].results.clear(),
                2 => methods[0].is_async = true,
                3 => methods.push(methods[0].clone()),
                _ => unreachable!(),
            }
            package.types.set_interface_methods(interface, methods);
            let diagnostics = adapt(&mut package);
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert!(
                diagnostics[0]
                    .message()
                    .starts_with("bundled HTTP adapter contract mismatch:")
            );
            assert_eq!(diagnostics[0].span(), package.functions[hook].span);
        }
    }

    #[test]
    fn registration_factory_is_selected_by_parameter_id() {
        let (mut package, hook) = bundled();
        let contract = HandlerContract::resolve(&package, hook).unwrap_or_else(|d| panic!("{d:?}"));
        let register = &mut package.functions[contract.registration];
        let original = register.params[2];
        let replacement = LocalId(register.locals.len() as u32);
        register
            .locals
            .push(register.locals[original.0 as usize].clone());
        register.locals[original.0 as usize].ty = TypeStore::BOOL;
        register.params[2] = replacement;
        assert!(HandlerContract::resolve(&package, hook).is_ok());
    }
}
