//! Type checking of resolved syntax into typed HIR.

use std::collections::HashMap;

use crate::ast::{
    self, AssignOp, AssignTarget, BinaryOp, BindingKind, BindingTarget, ForHeader, UnaryOp,
};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, Const, ExprKind, FieldId, FunctionId, LocalId, LocalKind};
use crate::resolve::{ConstId, Res, Resolution};
use crate::source::Span;
use crate::types::bignum::BigInt;
use crate::types::constant::{self, ConstError, Folded, Unrepresentable, Untyped};
use crate::types::{IntType, StructId, TypeId, TypeKind, TypeStore};

/// Type-checks a resolved file; HIR is returned only when there are no diagnostics.
pub fn check(
    file: &ast::File,
    mut resolution: Resolution<'_>,
    text: &str,
    allow_move_types: bool,
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
    let entry = checker.check_entry_point(file);
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
    let mut diagnostics = checker.diagnostics;
    for function in &package.functions {
        for local in &function.locals {
            if !allow_move_types && !package.is_copy(local.ty) {
                diagnostics.push(Diagnostic::new(
                    Severity::Error,
                    "Move types require drop insertion, which is not implemented yet",
                    local.span,
                ));
            }
        }
        if !allow_move_types {
            for &result in &function.results {
                if !package.is_copy(result) {
                    diagnostics.push(Diagnostic::new(
                        Severity::Error,
                        "Move types require drop insertion, which is not implemented yet",
                        function.span,
                    ));
                }
            }
            reject_move_expressions(&package, &function.body, &mut diagnostics);
        }
    }
    if diagnostics.is_empty() {
        (Some(package), diagnostics)
    } else {
        (None, diagnostics)
    }
}

fn reject_move_expressions(
    package: &hir::Package,
    block: &hir::Block,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for stmt in &block.stmts {
        reject_move_statement(package, stmt, diagnostics);
    }
}

fn reject_move_statement(
    package: &hir::Package,
    stmt: &hir::Stmt,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &stmt.kind {
            hir::StmtKind::Let { value, .. } => reject_move_expr(package, value, diagnostics),
            hir::StmtKind::Assign { values, .. } | hir::StmtKind::Return(values) => {
                for value in values {
                    reject_move_expr(package, value, diagnostics);
                }
            }
            hir::StmtKind::CompoundAssign { value, .. } | hir::StmtKind::Expr(value) => {
                reject_move_expr(package, value, diagnostics);
            }
            hir::StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                reject_move_expr(package, condition, diagnostics);
                reject_move_expressions(package, then_block, diagnostics);
                if let Some(else_block) = else_block {
                    reject_move_expressions(package, else_block, diagnostics);
                }
            }
            hir::StmtKind::Loop {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    reject_move_statement(package, init, diagnostics);
                }
                if let Some(condition) = condition {
                    reject_move_expr(package, condition, diagnostics);
                }
                if let Some(update) = update {
                    reject_move_statement(package, update, diagnostics);
                }
                reject_move_expressions(package, body, diagnostics);
            }
            hir::StmtKind::Block(block) => reject_move_expressions(package, block, diagnostics),
            hir::StmtKind::Break | hir::StmtKind::Continue => {}
    }
}

fn reject_move_expr(
    package: &hir::Package,
    expr: &hir::Expr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if expr.types.iter().any(|&ty| !package.is_copy(ty)) {
        diagnostics.push(Diagnostic::new(
            Severity::Error,
            "Move types require drop insertion, which is not implemented yet",
            expr.span,
        ));
    }
    match &expr.kind {
        ExprKind::Field { base, .. }
        | ExprKind::Println(base)
        | ExprKind::Convert(base)
        | ExprKind::Unary { operand: base, .. } => reject_move_expr(package, base, diagnostics),
        ExprKind::Drop(base) => {
            diagnostics.push(Diagnostic::new(
                Severity::Error,
                "`drop(value)` requires drop insertion, which is not implemented yet",
                expr.span,
            ));
            reject_move_expr(package, base, diagnostics);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            reject_move_expr(package, lhs, diagnostics);
            reject_move_expr(package, rhs, diagnostics);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                reject_move_expr(package, arg, diagnostics);
            }
        }
        ExprKind::StructLit { fields, .. } => {
            for (_, value) in fields {
                reject_move_expr(package, value, diagnostics);
            }
        }
        ExprKind::Const(_) | ExprKind::Local(_) => {}
    }
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
        self.error(message, span);
    }

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
                    .receiver
                    .iter()
                    .chain(&func.params)
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
        let name = match &func.receiver {
            Some(receiver) => format!("{}.{}", receiver.ty.name.text, func.name.text),
            None => func.name.text.clone(),
        };
        Some(hir::Function {
            name,
            span: func.name.span,
            params: (0..params.len()).map(|i| LocalId(i as u32)).collect(),
            results,
            locals: locals?,
            body,
        })
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
            .note("§6.7 requires at least 256 bits; the larger limit is an implementation limit"),
            ConstError::FloatOverflow => Diagnostic::new(
                Severity::Error,
                "floating-point constant is too large",
                span,
            )
            .note(format!(
                "constants must stay below 2^{} (§6.7 implementation limit)",
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
            Res::Drop => {
                self.error("`drop` can only be called", span);
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
                numeric || matches!(kind, TypeKind::Bool | TypeKind::Rune | TypeKind::String)
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
        let left = self.expr(lhs, operand_expected);
        let right_expected = match &left {
            Some(Value::Typed(expr)) if expr.types.len() == 1 => Some(expr.ty()),
            _ => operand_expected,
        };
        let right = self.expr(rhs, right_expected);
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
                if self.expr(callee, None).is_some() {
                    self.error("this expression cannot be called", callee.span);
                }
                self.report_arg_errors(args);
                return None;
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
            Some(Res::Primitive(ty)) => self.conversion(ty, args, span),
            Some(Res::Struct(_)) => {
                self.error(
                    format!("struct `{name}` is constructed with `{name}{{...}}`, not called"),
                    callee.span,
                );
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
            self.error(message, name.span);
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
                .note(
                    "destruction runs when ownership ends or through `drop(value)` (§14.3, §14.4)",
                ),
            );
            self.report_arg_errors(args);
            return None;
        }
        self.function_call(id, &name.text, Some(receiver), args, span)
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

    fn check_mut_arguments(&mut self, id: FunctionId, args: &[hir::Expr]) -> bool {
        let by_mut: Vec<bool> = self.res.locals[id.0 as usize]
            .iter()
            .take(args.len())
            .map(|local| local.kind == LocalKind::Param(ast::ParamMode::Mut))
            .collect();
        let mut ok = true;
        for (arg, &is_mut) in args.iter().zip(&by_mut) {
            if is_mut {
                ok &= self.mutable_place(arg);
            }
        }
        if !ok {
            return false;
        }
        let places: Vec<_> = args.iter().map(argument_place).collect();
        for later in 1..args.len() {
            for earlier in 0..later {
                if !by_mut[earlier] && !by_mut[later] {
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
                    .note("a mutable borrow requires exclusive access (§11.3)"),
                );
                ok = false;
            }
        }
        ok
    }

    fn mutable_place(&mut self, expr: &hir::Expr) -> bool {
        match &expr.kind {
            ExprKind::Local(id) => {
                let decl = &self.res.locals[self.current][id.0 as usize];
                let (name, kind, decl_span) = (decl.name.clone(), decl.kind, decl.span);
                let (what, note) = match kind {
                    LocalKind::Var | LocalKind::Param(ast::ParamMode::Mut) => return true,
                    LocalKind::Let => (
                        "immutable binding",
                        "declare it with `var` to pass it as `mut`",
                    ),
                    LocalKind::Param(ast::ParamMode::Borrow) => (
                        "shared parameter",
                        "a parameter is a shared borrow unless declared `mut` (§7.3)",
                    ),
                    LocalKind::Param(ast::ParamMode::Own) => (
                        "`own` parameter",
                        "only `var` bindings and `mut` parameters are mutable places (§11.6)",
                    ),
                };
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("cannot pass {what} `{name}` as a `mut` argument"),
                        expr.span,
                    )
                    .related(decl_span, "declared here")
                    .note(note),
                );
                false
            }
            ExprKind::Field { base, .. } => self.mutable_place(base),
            _ => {
                self.error(
                    "a `mut` argument must be a mutable place, not a temporary value",
                    expr.span,
                );
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
                    "`{}` is not a conversion; only numeric conversions exist (§6.6)",
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
                    LocalKind::Param(ast::ParamMode::Mut) => {}
                    LocalKind::Param(ast::ParamMode::Own) => {
                        self.unsupported(
                            "assigning to `own` parameters is",
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
                let mut place = self.assignable_place(base)?;
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

type ArgumentPlace = (LocalId, Vec<FieldId>);

/// The local and field path an argument names, if it is a plain place.
fn argument_place(expr: &hir::Expr) -> Option<ArgumentPlace> {
    match &expr.kind {
        ExprKind::Local(id) => Some((*id, Vec::new())),
        ExprKind::Field { base, field } => {
            let (root, mut fields) = argument_place(base)?;
            fields.push(*field);
            Some((root, fields))
        }
        _ => None,
    }
}

fn places_overlap(a: &ArgumentPlace, b: &ArgumentPlace) -> bool {
    a.0 == b.0 && a.1.iter().zip(&b.1).all(|(x, y)| x == y)
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
