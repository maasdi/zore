use crate::ast::{self, ExprKind, StmtKind, Type};
use crate::source::Span;

pub(super) fn used(file: &ast::File) -> Option<Span> {
    file.items.iter().find_map(|item| match item {
        ast::Item::Func(function) => function
            .receiver
            .iter()
            .chain(&function.params)
            .find_map(|param| ty(&param.ty))
            .or_else(|| function.results.iter().find_map(ty))
            .or_else(|| block(&function.body)),
        ast::Item::Struct(declaration) => declaration.fields.iter().find_map(|field| ty(&field.ty)),
        ast::Item::Named(declaration) => ty(&declaration.base),
        ast::Item::Interface(declaration) => declaration.methods.iter().find_map(|method| {
            if crate::stdlib::string_method(&method.name.text) {
                return Some(method.name.span);
            }
            method
                .params
                .iter()
                .find_map(|param| ty(&param.ty))
                .or_else(|| method.results.iter().find_map(ty))
        }),
        ast::Item::Binding(value) => binding(value),
    })
}

fn ty(value: &Type) -> Option<Span> {
    match value {
        Type::Named(_) | Type::Qualified { .. } => None,
        Type::Array { element, size, .. } => expr(size).or_else(|| ty(element)),
        Type::DynArray { element, .. }
        | Type::Slice { element, .. }
        | Type::Channel { element, .. }
        | Type::Mutex { element, .. } => ty(element),
        Type::Map { key, value, .. } => ty(key).or_else(|| ty(value)),
        Type::Func {
            params, results, ..
        } => params
            .iter()
            .find_map(|param| ty(&param.ty))
            .or_else(|| results.iter().find_map(ty)),
        Type::Instance { base, args, .. } => ty(base).or_else(|| args.iter().find_map(ty)),
        Type::Task { results, .. } => results.iter().find_map(ty),
    }
}

fn block(value: &ast::Block) -> Option<Span> {
    value.stmts.iter().find_map(statement)
}

fn binding(value: &ast::Binding) -> Option<Span> {
    value
        .ty
        .as_ref()
        .and_then(ty)
        .or_else(|| expr(&value.value))
}

fn branch(value: &ast::If) -> Option<Span> {
    expr(&value.condition)
        .or_else(|| block(&value.then_block))
        .or_else(|| {
            value.else_branch.as_ref().and_then(|value| match value {
                ast::Else::If(value) => branch(value),
                ast::Else::Block(value) => block(value),
            })
        })
}

fn statement(value: &ast::Stmt) -> Option<Span> {
    match &value.kind {
        StmtKind::Binding(value) => binding(value),
        StmtKind::MixedBinding(value) => value
            .targets
            .iter()
            .find_map(|target| match target {
                ast::MixedTarget::Assign(value) => expr(value),
                _ => None,
            })
            .or_else(|| expr(&value.value)),
        StmtKind::Assign {
            targets, values, ..
        } => targets
            .iter()
            .find_map(|target| match target {
                ast::AssignTarget::Place(value) => expr(value),
                ast::AssignTarget::Discard(_) => None,
            })
            .or_else(|| values.iter().find_map(expr)),
        StmtKind::Expr(value) => expr(value),
        StmtKind::Return(values) => values.iter().find_map(expr),
        StmtKind::Break | StmtKind::Continue => None,
        StmtKind::If(value) => branch(value),
        StmtKind::For(value) => {
            let header = match &value.header {
                ast::ForHeader::Infinite => None,
                ast::ForHeader::Condition(value) => expr(value),
                ast::ForHeader::Counting {
                    init,
                    condition,
                    update,
                } => statement(init)
                    .or_else(|| expr(condition))
                    .or_else(|| statement(update)),
                ast::ForHeader::Each { collection, .. } => expr(collection),
            };
            header.or_else(|| block(&value.body))
        }
        StmtKind::Select(value) => value
            .arms
            .iter()
            .find_map(|arm| {
                match &arm.comm {
                    ast::SelectComm::Bind(value) => binding(value),
                    ast::SelectComm::Expr(value) => expr(value),
                }
                .or_else(|| block(&arm.body))
            })
            .or_else(|| value.default.as_ref().and_then(block)),
        StmtKind::Block(value) | StmtKind::TaskScope(value) => block(value),
    }
}

fn expr(value: &ast::Expr) -> Option<Span> {
    match &value.kind {
        ExprKind::Field { base, name } => {
            if crate::stdlib::string_method(&name.text) {
                Some(name.span)
            } else {
                expr(base)
            }
        }
        ExprKind::Paren(value)
        | ExprKind::Await(value)
        | ExprKind::Go(value)
        | ExprKind::Try(value)
        | ExprKind::Unary { operand: value, .. } => expr(value),
        ExprKind::Binary { lhs, rhs, .. } => expr(lhs).or_else(|| expr(rhs)),
        ExprKind::Call { callee, args } => expr(callee).or_else(|| args.iter().find_map(expr)),
        ExprKind::Index { base, index } => expr(base).or_else(|| expr(index)),
        ExprKind::Slice { base, low, high } => expr(base)
            .or_else(|| low.as_deref().and_then(expr))
            .or_else(|| high.as_deref().and_then(expr)),
        ExprKind::StructLit {
            type_args, fields, ..
        } => type_args
            .iter()
            .find_map(ty)
            .or_else(|| fields.iter().find_map(|field| expr(&field.value))),
        ExprKind::ArrayLit {
            ty: element,
            elements,
        } => ty(element).or_else(|| elements.iter().find_map(expr)),
        ExprKind::MapLit {
            ty: element,
            entries,
        } => ty(element).or_else(|| {
            entries
                .iter()
                .find_map(|entry| expr(&entry.key).or_else(|| expr(&entry.value)))
        }),
        ExprKind::Closure(value) => value
            .params
            .iter()
            .find_map(|param| ty(&param.ty))
            .or_else(|| value.results.iter().find_map(ty))
            .or_else(|| block(&value.body)),
        ExprKind::Channel { element, capacity } => {
            ty(element).or_else(|| capacity.as_deref().and_then(expr))
        }
        ExprKind::Name(_)
        | ExprKind::Int(_)
        | ExprKind::Float
        | ExprKind::String(_)
        | ExprKind::Rune(_)
        | ExprKind::Bool(_)
        | ExprKind::Nil
        | ExprKind::Malformed => None,
    }
}
