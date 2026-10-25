use crate::ast::ParamMode;
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, Const, Expr, ExprKind, StmtKind};
use crate::resolve::{FieldId, FunctionId, LocalId, LocalKind};
use crate::source::Span;
use crate::types::{FuncSignature, InterfaceId, TypeId, TypeStore};

use super::signature::{function_mismatch, method_mismatch};

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
            let ExprKind::Closure {
                function: callback,
                captures,
                ..
            } = &args[2].kind
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
            let signature = package
                .types
                .func_signature(callable_type)
                .cloned()
                .expect("a function literal or name has a function type");
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

const HTTP: &str = "std/net/http";

impl HandlerContract {
    fn resolve(package: &mut hir::Package, hook: usize) -> Result<Self, Box<Diagnostic>> {
        let span = package.functions[hook].span;
        let fail = |member: &str| Box::new(contract_error(member, span));
        let registration = package
            .functions
            .iter()
            .position(|function| function.name == "std/net/http.RouterBuilder.register")
            .ok_or_else(|| fail("missing RouterBuilder.register"))?;
        let declared = |name: &str| {
            package
                .declared_type(HTTP, name)
                .ok_or_else(|| fail(&format!("missing {HTTP}.{name}")))
        };
        let router = declared("RouterBuilder")?;
        let factory_type = declared("HandlerFactory")?;
        let handler_type = declared("Handler")?;
        let response_writer = declared("ResponseWriter")?;
        let request = declared("Request")?;
        let interface = |ty: TypeId, name: &str| {
            package
                .types
                .interface_of(ty)
                .ok_or_else(|| fail(&format!("{HTTP}.{name} is not an interface")))
        };
        let factory_interface = interface(factory_type, "HandlerFactory")?;
        let handler_interface = interface(handler_type, "Handler")?;
        if let Some(detail) = interface_method_mismatch(
            &package.types,
            factory_interface,
            "HandlerFactory",
            "NewHandler",
            &FuncSignature {
                is_async: false,
                params: Vec::new(),
                results: vec![handler_type, TypeStore::ERROR],
            },
        ) {
            return Err(fail(&detail));
        }
        let handle_signature = FuncSignature {
            is_async: false,
            params: vec![(ParamMode::Mut, response_writer), (ParamMode::Mut, request)],
            results: vec![TypeStore::ERROR],
        };
        if let Some(detail) = interface_method_mismatch(
            &package.types,
            handler_interface,
            "Handler",
            "Handle",
            &handle_signature,
        ) {
            return Err(fail(&detail));
        }
        let handler_function = package.types.func_type(handle_signature);
        let package = &*package;
        let registration_shape = |callback: TypeId| FuncSignature {
            is_async: false,
            params: vec![
                (ParamMode::Mut, router),
                (ParamMode::Borrow, TypeStore::STRING),
                (ParamMode::Own, callback),
            ],
            results: Vec::new(),
        };
        if let Some(detail) = function_mismatch(
            &package.types,
            &package.functions[hook],
            &registration_shape(handler_function),
        ) {
            return Err(fail(&format!("RouterBuilder.Handle: {detail}")));
        }
        if let Some(detail) = function_mismatch(
            &package.types,
            &package.functions[registration],
            &registration_shape(factory_type),
        ) {
            return Err(fail(&format!("RouterBuilder.register: {detail}")));
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

fn interface_method_mismatch(
    types: &TypeStore,
    interface: InterfaceId,
    interface_name: &str,
    method_name: &str,
    expected: &FuncSignature,
) -> Option<String> {
    let [method] = types.interface_methods(interface) else {
        return Some(format!(
            "{interface_name} must declare exactly one method, {method_name}"
        ));
    };
    if method.name != method_name {
        return Some(format!(
            "{interface_name} must declare exactly one method, {method_name}"
        ));
    }
    if method.receiver != ParamMode::Borrow {
        return Some(format!(
            "{interface_name}.{method_name}: receiver must be a shared borrow"
        ));
    }
    method_mismatch(types, method, expected)
        .map(|detail| format!("{interface_name}.{method_name}: {detail}"))
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
                HandlerContract::resolve(&mut package, hook).unwrap_or_else(|d| panic!("{d:?}"));
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
        let contract =
            HandlerContract::resolve(&mut package, hook).unwrap_or_else(|d| panic!("{d:?}"));
        let register = &mut package.functions[contract.registration];
        let original = register.params[2];
        let replacement = LocalId(register.locals.len() as u32);
        register
            .locals
            .push(register.locals[original.0 as usize].clone());
        register.locals[original.0 as usize].ty = TypeStore::BOOL;
        register.params[2] = replacement;
        assert!(HandlerContract::resolve(&mut package, hook).is_ok());
    }

    #[test]
    fn lookalike_types_with_the_same_display_name_are_rejected() {
        let (mut package, hook) = bundled();
        let contract =
            HandlerContract::resolve(&mut package, hook).unwrap_or_else(|d| panic!("{d:?}"));
        let methods = package
            .types
            .interface_methods(contract.factory_interface)
            .to_vec();
        let (lookalike, lookalike_type) = package.types.add_interface("http.HandlerFactory");
        package.types.set_interface_methods(lookalike, methods);
        let register = &mut package.functions[contract.registration];
        let factory_parameter = register.params[2].0 as usize;
        register.locals[factory_parameter].ty = lookalike_type;
        let diagnostics = adapt(&mut package);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "bundled HTTP adapter contract mismatch: RouterBuilder.register: parameter 3 is a different type named `own http.HandlerFactory`"
        );

        let (mut package, hook) = bundled();
        let contract =
            HandlerContract::resolve(&mut package, hook).unwrap_or_else(|d| panic!("{d:?}"));
        let mut methods = package
            .types
            .interface_methods(contract.handler_interface)
            .to_vec();
        let (_, lookalike_request) = package.types.add_struct("http.Request");
        methods[0].params[1].1 = lookalike_request;
        package
            .types
            .set_interface_methods(contract.handler_interface, methods);
        let diagnostics = adapt(&mut package);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "bundled HTTP adapter contract mismatch: Handler.Handle: parameter 2 is a different type named `mut http.Request`"
        );
    }

    #[test]
    fn contract_drift_names_the_failed_condition() {
        type Mutation = fn(&mut hir::Package, usize, usize);
        let cases: [(Mutation, &str); 9] = [
            (
                |package, _, _| {
                    package
                        .declared_types
                        .remove(&(HTTP.to_owned(), "Handler".to_owned()));
                },
                "missing std/net/http.Handler",
            ),
            (
                |package, _, _| {
                    package.declared_types.insert(
                        (HTTP.to_owned(), "HandlerFactory".to_owned()),
                        TypeStore::BOOL,
                    );
                },
                "std/net/http.HandlerFactory is not an interface",
            ),
            (
                |package, _, _| {
                    let interface = handler_interface(package);
                    let mut methods = package.types.interface_methods(interface).to_vec();
                    methods[0].receiver = ParamMode::Mut;
                    package.types.set_interface_methods(interface, methods);
                },
                "Handler.Handle: receiver must be a shared borrow",
            ),
            (
                |package, _, _| {
                    let interface = handler_interface(package);
                    let mut methods = package.types.interface_methods(interface).to_vec();
                    methods[0].params.swap(0, 1);
                    package.types.set_interface_methods(interface, methods);
                },
                "Handler.Handle: parameter 1 is `mut http.Request`, expected `mut http.ResponseWriter`",
            ),
            (
                |package, _, register| package.functions[register].is_async = true,
                "RouterBuilder.register: must be synchronous",
            ),
            (
                |package, hook, _| package.functions[hook].results.push(TypeStore::ERROR),
                "RouterBuilder.Handle: returns `error`, expected `nothing`",
            ),
            (
                |package, _, register| {
                    let function = &mut package.functions[register];
                    let local = function.params[2].0 as usize;
                    function.locals[local].kind = LocalKind::Param(ParamMode::Borrow);
                },
                "RouterBuilder.register: parameter 3 is `http.HandlerFactory`, expected `own http.HandlerFactory`",
            ),
            (
                |package, hook, _| {
                    let function = &mut package.functions[hook];
                    let local = function.params[0].0 as usize;
                    function.locals[local].kind = LocalKind::Param(ParamMode::Borrow);
                },
                "RouterBuilder.Handle: parameter 1 is `http.RouterBuilder`, expected `mut http.RouterBuilder`",
            ),
            (
                |package, hook, _| {
                    let function = &mut package.functions[hook];
                    let local = function.params[1].0 as usize;
                    function.locals[local].ty = TypeStore::INT64;
                },
                "RouterBuilder.Handle: parameter 2 is `int64`, expected `string`",
            ),
        ];
        for (mutate, expected) in cases {
            let (mut package, hook) = bundled();
            let register = package
                .functions
                .iter()
                .position(|f| f.name == "std/net/http.RouterBuilder.register")
                .unwrap();
            mutate(&mut package, hook, register);
            let diagnostics = adapt(&mut package);
            assert_eq!(diagnostics.len(), 1, "{expected}: {diagnostics:?}");
            assert_eq!(
                diagnostics[0].message(),
                format!("bundled HTTP adapter contract mismatch: {expected}")
            );
            assert_eq!(diagnostics[0].span(), package.functions[hook].span);
        }
    }

    fn handler_interface(package: &hir::Package) -> InterfaceId {
        package
            .types
            .interface_of(package.declared_type(HTTP, "Handler").unwrap())
            .unwrap()
    }
}
