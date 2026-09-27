//! Type checking of resolved syntax into typed HIR (spec §5–8, §6.5–6.6,
//! §7, §3.19, §37.1).
//!
//! Untyped integer constants keep exact values (up to a 128-bit
//! implementation limit) until context gives them a type. Constant
//! expressions are folded; typed constant overflow and invalid constant shift
//! counts are compile-time errors. Features outside the checker's current
//! subset are diagnosed as unsupported rather than guessed.

use std::collections::HashMap;

use crate::ast::{
    self, AssignOp, AssignTarget, BinaryOp, BindingKind, BindingTarget, ForHeader, UnaryOp,
};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, Const, ExprKind, FieldId, FunctionId, LocalId, LocalKind};
use crate::resolve::{ConstId, Res, Resolution};
use crate::source::Span;
use crate::types::{IntType, TypeId, TypeKind, TypeStore};

/// Check a resolved file. HIR is returned only when the resolver and the
/// checker reported no diagnostics.
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
    };
    checker.signatures_and_fields();
    let mut functions = Vec::new();
    for index in 0..checker.res.functions.len() {
        functions.push(checker.function(FunctionId(index as u32)));
    }
    for index in 0..checker.res.consts.len() {
        checker.eval_const(ConstId(index as u32));
    }
    let entry = checker.entry_point(file);
    if !checker.diagnostics.is_empty() {
        return (None, checker.diagnostics);
    }
    let structs = checker
        .res
        .structs
        .iter()
        .zip(&checker.fields)
        .map(|(decl, fields)| hir::Struct {
            name: decl.name.text.clone(),
            span: decl.name.span,
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
    let package = hir::Package {
        name: checker.res.package.clone(),
        types: std::mem::take(&mut checker.types),
        structs,
        functions: functions.into_iter().map(|f| f.expect("checked")).collect(),
        entry,
    };
    // Ownership analysis is not implemented yet. It is vacuous only while
    // every accepted type is Copy and `mut` parameters are unsupported, so
    // refuse anything else instead of reporting unchecked success.
    let mut diagnostics = checker.diagnostics;
    for function in &package.functions {
        for local in &function.locals {
            if !package.is_copy(local.ty) {
                diagnostics.push(Diagnostic::new(
                    Severity::Error,
                    "Move types require ownership analysis, which is not implemented yet",
                    local.span,
                ));
            }
        }
    }
    if diagnostics.is_empty() {
        (Some(package), diagnostics)
    } else {
        (None, diagnostics)
    }
}

#[derive(Clone)]
enum ConstValue {
    Untyped(i128),
    Typed(TypeId, Const),
}

enum ConstState {
    Pending,
    InProgress,
    Done(Option<ConstValue>),
}

/// A checked expression before context has fixed an untyped constant's type.
enum Value {
    Untyped(i128, Span),
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
    /// Field names and types of each struct; `None` if the type failed.
    fields: Vec<Vec<(String, Option<TypeId>, Span)>>,
    signatures: Vec<Option<Signature>>,
    consts: Vec<ConstState>,
    diagnostics: Vec<Diagnostic>,
    // Per-function state.
    current: usize,
    locals: Vec<Option<TypeId>>,
    results: Vec<TypeId>,
    loop_depth: usize,
}

const LIMIT_NOTE: &str = "untyped integer constants are currently limited to 128 bits";

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
    // ----- diagnostics -----

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
        self.error(message, span);
    }

    // ----- declarations -----

    fn resolve_type(&self, ty: &ast::Type) -> Option<TypeId> {
        match self.res.uses.get(&ty.name.span)? {
            Res::Primitive(ty) => Some(*ty),
            Res::Struct(id) => Some(self.types.struct_type(*id)),
            _ => None,
        }
    }

    fn signatures_and_fields(&mut self) {
        self.fields = self
            .res
            .structs
            .iter()
            .map(|decl| {
                decl.fields
                    .iter()
                    .map(|f| (f.name.text.clone(), self.resolve_type(&f.ty), f.name.span))
                    .collect()
            })
            .collect();
        self.signatures = self
            .res
            .functions
            .iter()
            .map(|func| {
                let params: Option<Vec<_>> = func
                    .params
                    .iter()
                    .map(|p| self.resolve_type(&p.ty))
                    .collect();
                let results: Option<Vec<_>> =
                    func.results.iter().map(|t| self.resolve_type(t)).collect();
                Some(Signature {
                    params: params?,
                    results: results?,
                })
            })
            .collect();
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
        if !results.is_empty() && !block_terminates(&func.body) {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!(
                        "function `{}` can reach the end of its body without returning a value",
                        func.name.text
                    ),
                    func.name.span,
                )
                .note("every path must return the declared results (§7.7)"),
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
            name: func.name.text.clone(),
            span: func.name.span,
            params: (0..params.len()).map(|i| LocalId(i as u32)).collect(),
            results,
            locals: locals?,
            body,
        })
    }

    /// Enforce the §3.19 entry-point contract for a `main` package.
    fn entry_point(&mut self, file: &ast::File) -> Option<FunctionId> {
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
                .note("an executable package declares `func main() { ... }` (§3.19)"),
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
                    .note("declare it as `func main() { ... }` (§3.19)"),
            );
            return None;
        }
        self.res
            .functions
            .iter()
            .position(|f| std::ptr::eq(*f, main))
            .map(|index| FunctionId(index as u32))
    }

    // ----- constants -----

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
                                .note("constants use literals, other constants, operators, and numeric conversions (§5.3)"),
                            );
                            None
                        }
                    },
                }
            }
        };
        // A cycle may already have recorded failure for this constant.
        if matches!(self.consts[id.0 as usize], ConstState::InProgress) {
            self.consts[id.0 as usize] = ConstState::Done(result.clone());
            result
        } else {
            None
        }
    }

    // ----- values and coercion -----

    /// Give a value the type `target`, range-checking untyped constants.
    fn coerce(&mut self, value: Value, target: TypeId) -> Option<hir::Expr> {
        match value {
            Value::Untyped(v, span) => match self.types.int(target) {
                Some(int) if int.contains(v) => {
                    Some(typed(ExprKind::Const(Const::Int(v)), target, span))
                }
                Some(_) => {
                    let message = format!(
                        "integer constant `{v}` does not fit in `{}`",
                        self.name(target)
                    );
                    self.error(message, span);
                    None
                }
                None => {
                    let message = format!(
                        "mismatched types: expected `{}`, found an integer constant",
                        self.name(target)
                    );
                    self.error(message, span);
                    None
                }
            },
            Value::Typed(expr) => {
                let expr = self.single(expr)?;
                if expr.ty() == target {
                    Some(expr)
                } else {
                    self.mismatch(target, expr.ty(), expr.span);
                    None
                }
            }
        }
    }

    /// The value with default typing: untyped integers become `int`.
    fn default(&mut self, value: Value) -> Option<hir::Expr> {
        match value {
            Value::Untyped(..) => self.coerce(value, TypeStore::INT),
            Value::Typed(expr) => self.single(expr),
        }
    }

    /// Require exactly one value from an expression (§7.8).
    fn single(&mut self, expr: hir::Expr) -> Option<hir::Expr> {
        match expr.types.len() {
            1 => Some(expr),
            0 => {
                self.error("this call has no value", expr.span);
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

    // ----- expressions -----

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
            ast::ExprKind::Int(base) => self.int_literal(*base, span),
            ast::ExprKind::Float => {
                self.unsupported(
                    "floating-point values are",
                    span,
                    "§6.7 float constants are not implemented yet",
                );
                None
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
                self.unsupported(
                    "`nil` is",
                    span,
                    "`nil` applies only to `error` and `Task` (M18–M29)",
                );
                None
            }
            ast::ExprKind::Malformed => None,
            ast::ExprKind::Paren(inner) => self.expr(inner, expected),
            ast::ExprKind::Unary { op, operand } => self.unary(*op, operand, span, expected),
            ast::ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, span, expected),
            ast::ExprKind::Await(_) => {
                self.unsupported("`await` is", span, "planned for roadmap milestone M25–M29");
                None
            }
            ast::ExprKind::Try(_) => {
                self.unsupported(
                    "the `?` operator is",
                    span,
                    "planned for roadmap milestone M18–M19",
                );
                None
            }
            ast::ExprKind::Call { callee, args } => self.call(callee, args, span),
            ast::ExprKind::Field { base, name } => self.field(base, name, span),
            ast::ExprKind::StructLit { ty, fields } => self.struct_lit(ty, fields, span),
        }
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
                    "function values are",
                    span,
                    "functions can currently only be called (closures and function types: M24)",
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
            Res::Unsupported => None,
        }
    }

    fn int_literal(&mut self, base: crate::token::IntBase, span: Span) -> Option<Value> {
        let text = &self.text[span.start() as usize..span.end() as usize];
        let digits: String = text
            .get(
                if base == crate::token::IntBase::Decimal {
                    0
                } else {
                    2
                }..,
            )
            .unwrap_or("")
            .chars()
            .filter(|&c| c != '_')
            .collect();
        match i128::from_str_radix(&digits, base.radix()) {
            Ok(value) => Some(Value::Untyped(value, span)),
            Err(_) => {
                self.diagnostics.push(
                    Diagnostic::new(Severity::Error, "integer literal is too large", span)
                        .note(LIMIT_NOTE),
                );
                None
            }
        }
    }

    fn untyped_result(&mut self, value: Option<i128>, span: Span) -> Option<Value> {
        match value {
            Some(value) => Some(Value::Untyped(value, span)),
            None => {
                self.diagnostics.push(
                    Diagnostic::new(Severity::Error, "constant value is too large", span)
                        .note(LIMIT_NOTE),
                );
                None
            }
        }
    }

    /// Range-check a folded typed integer constant (§6.6 checked arithmetic).
    fn int_const(&mut self, value: Option<i128>, ty: TypeId, span: Span) -> Option<Value> {
        let int = self.types.int(ty).expect("integer type");
        match value {
            Some(v) if int.contains(v) => Some(Value::Typed(typed(
                ExprKind::Const(Const::Int(v)),
                ty,
                span,
            ))),
            _ => {
                let message = format!("constant expression overflows `{}`", self.name(ty));
                self.error(message, span);
                None
            }
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
        let expr = match value {
            Value::Untyped(v, _) => {
                return match op {
                    UnaryOp::Plus => Some(Value::Untyped(v, span)),
                    UnaryOp::Neg => self.untyped_result(v.checked_neg(), span),
                    UnaryOp::Complement => {
                        self.unsupported(
                            "unary `^` on an untyped constant is",
                            span,
                            "§6.7 untyped bitwise operators are not implemented yet; convert first, e.g. `^uint8(x)`",
                        );
                        None
                    }
                    UnaryOp::Not => {
                        self.error(
                            "`!` requires a `bool` operand, found an integer constant",
                            span,
                        );
                        None
                    }
                };
            }
            Value::Typed(expr) => self.single(expr)?,
        };
        let ty = expr.ty();
        let int = self.types.int(ty);
        let valid = match op {
            UnaryOp::Not => ty == TypeStore::BOOL,
            _ => int.is_some(),
        };
        if !valid {
            let symbol = match op {
                UnaryOp::Plus => "+",
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
                UnaryOp::Complement => "^",
            };
            let message = format!("unary `{symbol}` cannot be applied to `{}`", self.name(ty));
            self.error(message, span);
            return None;
        }
        match (constant(&expr), int) {
            (Some(Const::Bool(b)), _) => Some(Value::Typed(typed(
                ExprKind::Const(Const::Bool(!b)),
                ty,
                span,
            ))),
            (Some(&Const::Int(v)), Some(int)) => match op {
                UnaryOp::Plus => self.int_const(Some(v), ty, span),
                UnaryOp::Neg => self.int_const(v.checked_neg(), ty, span),
                _ => self.int_const(Some(int.wrap(!v)), ty, span),
            },
            _ => Some(Value::Typed(typed(
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
        let left = self.expr(lhs, operand_expected);
        let right_expected = match &left {
            Some(Value::Typed(expr)) if expr.types.len() == 1 => Some(expr.ty()),
            _ => operand_expected,
        };
        let right = self.expr(rhs, right_expected);
        let (left, right) = (left?, right?);

        if let (Value::Untyped(a, _), Value::Untyped(b, _)) = (&left, &right) {
            let (a, b) = (*a, *b);
            let bool_value = |value| {
                Some(Value::Typed(typed(
                    ExprKind::Const(Const::Bool(value)),
                    TypeStore::BOOL,
                    span,
                )))
            };
            return match op {
                BinaryOp::Add => self.untyped_result(a.checked_add(b), span),
                BinaryOp::Sub => self.untyped_result(a.checked_sub(b), span),
                BinaryOp::Mul => self.untyped_result(a.checked_mul(b), span),
                BinaryOp::Eq => bool_value(a == b),
                BinaryOp::NotEq => bool_value(a != b),
                BinaryOp::Lt => bool_value(a < b),
                BinaryOp::LtEq => bool_value(a <= b),
                BinaryOp::Gt => bool_value(a > b),
                BinaryOp::GtEq => bool_value(a >= b),
                BinaryOp::And | BinaryOp::Or => {
                    let message = format!("`{}` requires `bool` operands", op_str(op));
                    self.error(message, span);
                    None
                }
                _ => {
                    self.unsupported(
                        &format!("`{}` between two untyped constants is", op_str(op)),
                        span,
                        "this §6.7 operation is not implemented yet; give one operand a type, e.g. `int(7) / 2`",
                    );
                    None
                }
            };
        }

        // At least one side is typed; the other takes its type.
        let (left, right) = match (left, right) {
            (Value::Typed(l), r) => {
                let l = self.single(l)?;
                let r = self.coerce(r, l.ty())?;
                (l, r)
            }
            (l @ Value::Untyped(..), Value::Typed(r)) => {
                let r = self.single(r)?;
                let l = self.coerce(l, r.ty())?;
                (l, r)
            }
            (Value::Untyped(..), Value::Untyped(..)) => unreachable!("handled above"),
        };
        let ty = left.ty();
        let kind = self.types.kind(ty);
        let is_int = matches!(kind, TypeKind::Int(_));
        let valid = match op {
            BinaryOp::Add => is_int || kind == TypeKind::String,
            BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Rem
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor => is_int,
            BinaryOp::Eq | BinaryOp::NotEq => {
                matches!(
                    kind,
                    TypeKind::Bool | TypeKind::Int(_) | TypeKind::Rune | TypeKind::String
                )
            }
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                matches!(kind, TypeKind::Int(_) | TypeKind::Rune | TypeKind::String)
            }
            BinaryOp::And | BinaryOp::Or => kind == TypeKind::Bool,
            BinaryOp::Shl | BinaryOp::Shr => unreachable!("shifts are checked separately"),
        };
        if !valid {
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

    /// Fold a binary operation on two typed constants of type `ty`.
    fn fold(&mut self, op: BinaryOp, a: Const, b: Const, ty: TypeId, span: Span) -> Option<Value> {
        let bool_value = |value| {
            Some(Value::Typed(typed(
                ExprKind::Const(Const::Bool(value)),
                TypeStore::BOOL,
                span,
            )))
        };
        let ordering = match (&a, &b) {
            (Const::Int(x), Const::Int(y)) => x.cmp(y),
            (Const::Rune(x), Const::Rune(y)) => x.cmp(y),
            // Byte-wise UTF-8 order (§6.6).
            (Const::String(x), Const::String(y)) => x.as_bytes().cmp(y.as_bytes()),
            (Const::Bool(x), Const::Bool(y)) => x.cmp(y),
            _ => unreachable!("operands share a type"),
        };
        match op {
            BinaryOp::Eq => return bool_value(ordering.is_eq()),
            BinaryOp::NotEq => return bool_value(ordering.is_ne()),
            BinaryOp::Lt => return bool_value(ordering.is_lt()),
            BinaryOp::LtEq => return bool_value(ordering.is_le()),
            BinaryOp::Gt => return bool_value(ordering.is_gt()),
            BinaryOp::GtEq => return bool_value(ordering.is_ge()),
            _ => {}
        }
        match (a, b) {
            (Const::Bool(x), Const::Bool(y)) => {
                bool_value(if op == BinaryOp::And { x && y } else { x || y })
            }
            (Const::String(x), Const::String(y)) => Some(Value::Typed(typed(
                ExprKind::Const(Const::String(x + &y)),
                ty,
                span,
            ))),
            (Const::Int(x), Const::Int(y)) => {
                let int = self.types.int(ty).expect("integer operands");
                if matches!(op, BinaryOp::Div | BinaryOp::Rem) && y == 0 {
                    self.error("division by zero in a constant expression", span);
                    return None;
                }
                let value = match op {
                    BinaryOp::Add => x.checked_add(y),
                    BinaryOp::Sub => x.checked_sub(y),
                    BinaryOp::Mul => x.checked_mul(y),
                    BinaryOp::Div => x.checked_div(y),
                    BinaryOp::Rem => x.checked_rem(y),
                    BinaryOp::BitAnd => Some(int.wrap(x & y)),
                    BinaryOp::BitOr => Some(int.wrap(x | y)),
                    BinaryOp::BitXor => Some(int.wrap(x ^ y)),
                    _ => unreachable!("remaining integer operators"),
                };
                self.int_const(value, ty, span)
            }
            _ => unreachable!("operator validity was checked"),
        }
    }

    /// Shifts: the left operand fixes the type; the count is any integer
    /// (§6.6, Q11).
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
        let (count, count_expr) = match right {
            Value::Untyped(n, count_span) => (
                Some(n),
                typed(ExprKind::Const(Const::Int(n)), TypeStore::INT, count_span),
            ),
            Value::Typed(expr) => {
                let expr = self.single(expr)?;
                if self.types.int(expr.ty()).is_none() {
                    let message = format!(
                        "shift count must be an integer, found `{}`",
                        self.name(expr.ty())
                    );
                    self.error(message, expr.span);
                    return None;
                }
                let count = match constant(&expr) {
                    Some(&Const::Int(n)) => Some(n),
                    _ => None,
                };
                (count, expr)
            }
        };
        if let Some(n) = count
            && n < 0
        {
            self.error(format!("shift count `{n}` is negative"), count_expr.span);
            return None;
        }
        let left = match left {
            Value::Untyped(v, _) => match count {
                // Exact untyped shifts; right shift rounds toward negative infinity.
                Some(n) if op == BinaryOp::Shl => {
                    let shifted = u32::try_from(n).ok().and_then(|n| {
                        let value = v.checked_mul(1i128.checked_shl(n)?)?;
                        (n < 127).then_some(value)
                    });
                    return self.untyped_result(shifted, span);
                }
                Some(n) => {
                    let shifted = if n >= 127 {
                        if v < 0 { -1 } else { 0 }
                    } else {
                        v >> n
                    };
                    return Some(Value::Untyped(shifted, span));
                }
                // A runtime count gives the constant its contextual type.
                None => {
                    let ty = expected
                        .filter(|&t| self.types.int(t).is_some())
                        .unwrap_or(TypeStore::INT);
                    self.coerce(left, ty)?
                }
            },
            Value::Typed(expr) => self.single(expr)?,
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
        if let Some(n) = count
            && n >= i128::from(int.bits)
        {
            let message = format!("shift count `{n}` is too large for `{}`", self.name(ty));
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, count_expr.span).note(format!(
                    "counts must be less than the width, {} bits",
                    int.bits
                )),
            );
            return None;
        }
        if let (Some(&Const::Int(v)), Some(n)) = (constant(&left), count) {
            let value = shift_value(int, v, n as u32, op);
            return Some(Value::Typed(typed(
                ExprKind::Const(Const::Int(value)),
                ty,
                span,
            )));
        }
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
            ast::ExprKind::Field { .. } => {
                self.unsupported(
                    "method calls are",
                    callee.span,
                    "planned for roadmap milestone M5–M8",
                );
                self.check_args(args);
                return None;
            }
            _ => {
                if self.expr(callee, None).is_some() {
                    self.error("this expression cannot be called", callee.span);
                }
                self.check_args(args);
                return None;
            }
        };
        let name = match &callee.kind {
            ast::ExprKind::Name(name) => name.as_str(),
            _ => unreachable!(),
        };
        match res {
            Some(Res::Function(id)) => self.function_call(id, name, args, span),
            Some(Res::Println) => self.println(args, span),
            Some(Res::Primitive(ty)) => self.conversion(ty, args, span),
            Some(Res::Struct(_)) => {
                self.error(
                    format!("struct `{name}` is constructed with `{name}{{...}}`, not called"),
                    callee.span,
                );
                self.check_args(args);
                None
            }
            Some(Res::Local(_) | Res::Const(_)) => {
                self.error(format!("`{name}` is not a function"), callee.span);
                self.check_args(args);
                None
            }
            Some(Res::Unsupported) | None => {
                self.check_args(args);
                None
            }
        }
    }

    /// Check arguments of a call that cannot be typed, for their own errors.
    fn check_args(&mut self, args: &[ast::Expr]) {
        for arg in args {
            self.expr(arg, None);
        }
    }

    fn function_call(
        &mut self,
        id: FunctionId,
        name: &str,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let Some(signature) = &self.signatures[id.0 as usize] else {
            self.check_args(args);
            return None;
        };
        let (params, results) = (signature.params.clone(), signature.results.clone());
        if args.len() != params.len() {
            let message = format!(
                "`{name}` takes {} argument{} but {} {} given",
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len(),
                if args.len() == 1 { "was" } else { "were" },
            );
            self.error(message, span);
            self.check_args(args);
            return None;
        }
        let mut checked = Vec::new();
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
        if !ok {
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

    /// `println` (§37.1): one printable argument, no results.
    fn println(&mut self, args: &[ast::Expr], span: Span) -> Option<Value> {
        let [arg] = args else {
            let message = format!(
                "`println` takes exactly 1 argument but {} were given",
                args.len()
            );
            self.error(message, span);
            self.check_args(args);
            return None;
        };
        let value = self.expr(arg, None)?;
        let expr = self.default(value)?;
        if !matches!(
            self.types.kind(expr.ty()),
            TypeKind::Bool | TypeKind::Int(_) | TypeKind::Rune | TypeKind::String
        ) {
            let message = format!(
                "`println` cannot print values of type `{}`",
                self.name(expr.ty())
            );
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, expr.span)
                    .note("printable types are bool, integers, floats, rune, and string (§37.1)"),
            );
            return None;
        }
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::Println(Box::new(expr)),
            types: Vec::new(),
            span,
        }))
    }

    /// Numeric conversion `T(value)` (§6.6); only integer targets so far.
    fn conversion(&mut self, target: TypeId, args: &[ast::Expr], span: Span) -> Option<Value> {
        let [arg] = args else {
            self.error("a conversion takes exactly one argument", span);
            self.check_args(args);
            return None;
        };
        let Some(int) = self.types.int(target) else {
            self.check_args(args);
            if target == TypeStore::RUNE {
                self.unsupported(
                    "rune conversions are",
                    span,
                    "planned with numeric typing (M5–M8)",
                );
            } else {
                let message = format!(
                    "`{}` is not a conversion; only numeric conversions exist (§6.6)",
                    self.name(target)
                );
                self.error(message, span);
            }
            return None;
        };
        let value = match self.expr(arg, None)? {
            Value::Untyped(v, value_span) => {
                if !int.contains(v) {
                    let message = format!(
                        "integer constant `{v}` does not fit in `{}`",
                        self.name(target)
                    );
                    self.error(message, value_span);
                    return None;
                }
                return Some(Value::Typed(typed(
                    ExprKind::Const(Const::Int(v)),
                    target,
                    span,
                )));
            }
            Value::Typed(expr) => self.single(expr)?,
        };
        if self.types.int(value.ty()).is_none() {
            let message = format!(
                "cannot convert `{}` to `{}`",
                self.name(value.ty()),
                self.name(target)
            );
            self.error(message, span);
            return None;
        }
        if let Some(&Const::Int(v)) = constant(&value) {
            if !int.contains(v) {
                let message = format!("constant `{v}` does not fit in `{}`", self.name(target));
                self.error(message, span);
                return None;
            }
            return Some(Value::Typed(typed(
                ExprKind::Const(Const::Int(v)),
                target,
                span,
            )));
        }
        Some(Value::Typed(typed(
            ExprKind::Convert(Box::new(value)),
            target,
            span,
        )))
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
            Value::Typed(expr) => self.single(expr)?,
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

    /// `T{field: value}` with every field named exactly once (§8.4).
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
                    .note("struct literals must initialize every field (§8.4)"),
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

    // ----- statements -----

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
                // An inner `None` is a part that failed to check.
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
                None => self.default(value)?,
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

    /// An assignable place: a `var` local or a field projection of one.
    fn place(&mut self, expr: &ast::Expr) -> Option<hir::Place> {
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
                let decl = &self.res.locals[self.current][id.0 as usize];
                let (kind, decl_span) = (decl.kind, decl.span);
                match kind {
                    LocalKind::Var => {}
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
                        return None;
                    }
                    LocalKind::Param(ast::ParamMode::Borrow) => {
                        self.diagnostics.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!("cannot assign to parameter `{name}`"),
                                expr.span,
                            )
                            .related(decl_span, "a shared borrow by default (§7.3)"),
                        );
                        return None;
                    }
                    LocalKind::Param(_) => {
                        self.unsupported(
                            "assigning to `own` or `mut` parameters is",
                            expr.span,
                            "planned for roadmap milestone M13–M17",
                        );
                        return None;
                    }
                }
                let ty = self.locals[id.0 as usize]?;
                Some(hir::Place {
                    root: id,
                    fields: Vec::new(),
                    ty,
                    span: expr.span,
                })
            }
            ast::ExprKind::Field { base, name } => {
                if !matches!(
                    base.kind,
                    ast::ExprKind::Name(_) | ast::ExprKind::Field { .. }
                ) {
                    self.expr(base, None);
                    self.error("cannot assign to a field of a temporary value", expr.span);
                    return None;
                }
                let mut place = self.place(base)?;
                let (field, ty) = self.field_of(place.ty, name)?;
                place.fields.push(field);
                place.ty = ty;
                place.span = expr.span;
                Some(place)
            }
            _ => unreachable!("the parser accepts only name and field targets"),
        }
    }

    fn assign(
        &mut self,
        targets: &[AssignTarget],
        op: AssignOp,
        values: &[ast::Expr],
        span: Span,
    ) -> Option<hir::StmtKind> {
        // `None` is a discard; `Some(None)` is a target that failed to check.
        let places: Vec<Option<Option<hir::Place>>> = targets
            .iter()
            .map(|t| match t {
                AssignTarget::Place(expr) => Some(self.place(expr)),
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
                    (Some(value), None) => self.default(value),
                    _ => None,
                };
                match expr {
                    Some(expr) => checked.push(expr),
                    None => ok = false,
                }
            }
        }
        // Targets must be provably disjoint (§5.6).
        let concrete: Vec<&hir::Place> =
            places.iter().filter_map(|p| p.as_ref()?.as_ref()).collect();
        for (i, a) in concrete.iter().enumerate() {
            for b in &concrete[i + 1..] {
                let shared = a.fields.len().min(b.fields.len());
                if a.root == b.root && a.fields[..shared] == b.fields[..shared] {
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
        let kind = self.types.kind(ty);
        if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
            let Some(int) = self.types.int(ty) else {
                let message = format!(
                    "operator `{}=` cannot be applied to `{}`",
                    op_str(op),
                    self.name(ty)
                );
                self.error(message, span);
                return None;
            };
            let count = match value {
                Value::Untyped(n, count_span) => {
                    typed(ExprKind::Const(Const::Int(n)), TypeStore::INT, count_span)
                }
                Value::Typed(expr) => self.single(expr)?,
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
        let valid = match op {
            BinaryOp::Add => matches!(kind, TypeKind::Int(_) | TypeKind::String),
            _ => matches!(kind, TypeKind::Int(_)),
        };
        if !valid {
            let message = format!(
                "operator `{}=` cannot be applied to `{}`",
                op_str(op),
                self.name(ty)
            );
            self.error(message, span);
            return None;
        }
        // A constant zero divisor with a runtime dividend panics at runtime;
        // it is not a constant expression (§6.6).
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
            self.check_args(values);
            return None;
        }
        if values.len() == 1 && results.len() > 1 {
            // Whole-result forwarding (§7.8).
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
            self.check_args(values);
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

/// Fold a typed shift; the count is already validated (§6.6, Q11).
fn shift_value(int: IntType, value: i128, count: u32, op: BinaryOp) -> i128 {
    if op == BinaryOp::Shl {
        int.wrap(((value as u128) << count) as i128)
    } else {
        value >> count
    }
}

/// Whether a block cannot complete normally (§7.7), conservatively.
fn block_terminates(block: &ast::Block) -> bool {
    block.stmts.iter().any(stmt_terminates)
}

fn stmt_terminates(stmt: &ast::Stmt) -> bool {
    match &stmt.kind {
        ast::StmtKind::Return(_) => true,
        ast::StmtKind::Block(block) => block_terminates(block),
        ast::StmtKind::If(if_stmt) => if_terminates(if_stmt),
        ast::StmtKind::For(for_stmt) => {
            matches!(for_stmt.header, ForHeader::Infinite) && !breaks(&for_stmt.body)
        }
        _ => false,
    }
}

fn if_terminates(if_stmt: &ast::If) -> bool {
    block_terminates(&if_stmt.then_block)
        && match &if_stmt.else_branch {
            None => false,
            Some(ast::Else::Block(block)) => block_terminates(block),
            Some(ast::Else::If(inner)) => if_terminates(inner),
        }
}

/// Whether a loop body contains a `break` for that loop.
fn breaks(block: &ast::Block) -> bool {
    block.stmts.iter().any(|stmt| match &stmt.kind {
        ast::StmtKind::Break => true,
        ast::StmtKind::Block(block) => breaks(block),
        ast::StmtKind::If(if_stmt) => if_breaks(if_stmt),
        _ => false,
    })
}

fn if_breaks(if_stmt: &ast::If) -> bool {
    breaks(&if_stmt.then_block)
        || match &if_stmt.else_branch {
            None => false,
            Some(ast::Else::Block(block)) => breaks(block),
            Some(ast::Else::If(inner)) => if_breaks(inner),
        }
}
