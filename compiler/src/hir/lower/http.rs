use crate::ast::ParamMode;
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, Const, Expr, ExprKind, StmtKind};
use crate::resolve::{FieldId, FunctionId, LocalId, LocalKind};
use crate::source::Span;
use crate::types::{TypeId, TypeStore};

pub(super) fn adapt(package: &mut hir::Package) -> Vec<Diagnostic> {
    let Some(hook) = package
        .functions
        .iter()
        .position(|f| f.name == "std/net/http.RouterBuilder.Handle")
    else {
        return Vec::new();
    };
    let registration = package
        .functions
        .iter()
        .position(|f| f.name == "std/net/http.RouterBuilder.register")
        .expect("HTTP registration helper");
    let factory_type = package.functions[registration].locals[2].ty;
    let factory_interface = package
        .types
        .interface_of(factory_type)
        .expect("factory interface");
    let handler_type = package.types.interface_methods(factory_interface)[0].results[0];
    let handler_interface = package
        .types
        .interface_of(handler_type)
        .expect("handler interface");
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
                .expect("handler signature")
                .clone();
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
        call_once: false,
        type_params: Vec::new(),
        probe: false,
        locals,
        body: hir::Block { stmts, span },
    }
}
