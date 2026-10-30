use super::{Checker, ConstValue, Value, typed};
use crate::ast;
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, Const, ExprKind};
use crate::resolve::{ConstId, FunctionId};
use crate::source::Span;
use crate::types::{TypeId, TypeStore};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum LogKind {
    Level(&'static str),
    Log,
    With,
}

const RESERVED_KEYS: [&str; 3] = ["time", "level", "msg"];

fn valid_key(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn parse_holes(template: &str) -> Result<Vec<String>, String> {
    let mut holes = Vec::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => return Err("a `{` hole is not closed by `}`".into()),
                    }
                }
                if !valid_key(&name) {
                    return Err(format!(
                        "`{{{name}}}` is not a hole name; use a letter or `_` followed by letters, digits, `_`, `.`, or `-`"
                    ));
                }
                holes.push(name);
            }
            '}' => return Err("a lone `}` must be written `}}`".into()),
            _ => {}
        }
    }
    Ok(holes)
}

fn plain_text(text: String, span: Span) -> hir::Expr {
    typed(
        ExprKind::Const(Const::String(text)),
        TypeStore::STRING,
        span,
    )
}

impl Checker<'_> {
    pub(super) fn log_kind(&self, id: FunctionId) -> Option<LogKind> {
        let index = id.0 as usize;
        if index >= self.res.functions.len()
            || self.res.packages[self.res.function_package[index]].path != "std/log"
            || self.res.functions[index].receiver.is_none()
        {
            return None;
        }
        let receiver = self.signatures[index].as_ref()?.params.first().copied()?;
        let strukt = self.types.struct_id(receiver)?;
        if self.res.structs[self.types.struct_origin(strukt).0 as usize]
            .name
            .text
            != "Logger"
        {
            return None;
        }
        Some(match self.res.functions[index].name.text.as_str() {
            "Debug" => LogKind::Level("LevelDebug"),
            "Info" => LogKind::Level("LevelInfo"),
            "Warn" => LogKind::Level("LevelWarn"),
            "Error" => LogKind::Level("LevelError"),
            "Log" => LogKind::Log,
            "With" => LogKind::With,
            _ => return None,
        })
    }

    fn log_helper(&self, name: &str) -> FunctionId {
        let index = (0..self.res.functions.len())
            .find(|&index| {
                self.res.packages[self.res.function_package[index]].path == "std/log"
                    && self.res.functions[index].name.text == name
                    && self.res.functions[index].receiver.is_some()
            })
            .expect("the bundled log package declares its helpers");
        FunctionId(index as u32)
    }

    fn level_type(&self, method: FunctionId) -> TypeId {
        self.signatures[method.0 as usize]
            .as_ref()
            .expect("the bundled log package is checked")
            .params[1]
    }

    fn level_constant(&mut self, name: &str, span: Span) -> Option<hir::Expr> {
        let index = (0..self.res.consts.len()).find(|&index| {
            self.res.packages[self.res.const_package[index]].path == "std/log"
                && self.res.consts[index].name.text == name
        })?;
        match self.eval_const(ConstId(index as u32))? {
            ConstValue::Typed(ty, value) => Some(typed(ExprKind::Const(value), ty, span)),
            ConstValue::Untyped(_) => None,
        }
    }

    fn check_keys(&mut self, name: &str, keys: &[(String, Span)]) -> bool {
        let mut ok = true;
        for (index, (key, span)) in keys.iter().enumerate() {
            if RESERVED_KEYS.contains(&key.as_str()) {
                self.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("`{name}` cannot use the key `{key}`"),
                        *span,
                    )
                    .note("`time`, `level`, and `msg` are written by every record"),
                );
                ok = false;
            } else if keys[..index].iter().any(|(earlier, _)| earlier == key) {
                self.error(format!("`{name}` uses the key `{key}` twice"), *span);
                ok = false;
            }
        }
        ok
    }

    pub(super) fn log_call(
        &mut self,
        kind: LogKind,
        method: FunctionId,
        receiver: hir::Expr,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let name = &self.res.functions[method.0 as usize].name.text;
        let name = format!("log.Logger.{name}");
        let mut level = None;
        let mut values = Vec::new();
        let mut ok = true;
        for (index, arg) in args.iter().enumerate() {
            let value = if kind == LogKind::Log && index == 0 {
                let level_ty = self.level_type(method);
                self.expr(arg, Some(level_ty))
                    .and_then(|value| self.coerce(value, level_ty))
            } else {
                self.expr(arg, None)
                    .and_then(|value| self.with_default_type(value))
            };
            match value {
                Some(expr) if kind == LogKind::Log && index == 0 => level = Some(expr),
                Some(expr) => values.push(expr),
                None => ok = false,
            }
        }
        if !ok {
            return None;
        }
        if args.len() <= usize::from(kind == LogKind::Log) {
            let wanted = if kind == LogKind::With {
                "a key and a value"
            } else {
                "a message template"
            };
            self.error(format!("`{name}` needs {wanted}"), span);
            return None;
        }
        if kind == LogKind::With {
            return self.log_with(&name, receiver, values, span);
        }
        let template = values.remove(0);
        let template_text = self.log_constant(&name, "template", &template)?;
        let holes = match parse_holes(&template_text) {
            Ok(holes) => holes,
            Err(message) => {
                self.error(format!("`{name}` template: {message}"), template.span);
                return None;
            }
        };
        let keys: Vec<_> = holes
            .iter()
            .map(|hole| (hole.clone(), template.span))
            .collect();
        if !self.check_keys(&name, &keys) {
            return None;
        }
        if holes.len() != values.len() {
            let message = format!(
                "`{name}` template has {} hole{} but {} value{} {} given",
                holes.len(),
                if holes.len() == 1 { "" } else { "s" },
                values.len(),
                if values.len() == 1 { "" } else { "s" },
                if values.len() == 1 { "was" } else { "were" },
            );
            self.diagnostics.push(
                Diagnostic::new(Severity::Error, message, span)
                    .note("each `{name}` hole in the template takes the next value"),
            );
            return None;
        }
        let texts = self.log_texts(&name, values, span)?;
        let level = match (kind, level) {
            (LogKind::Level(constant), _) => self.level_constant(constant, span)?,
            (_, Some(level)) => level,
            _ => unreachable!("`Log` checked its level above"),
        };
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::Call {
                function: self.log_helper("emit"),
                args: vec![
                    receiver,
                    level,
                    plain_text(template_text, template.span),
                    texts,
                ],
            },
            types: Vec::new(),
            span,
        }))
    }

    fn log_with(
        &mut self,
        name: &str,
        receiver: hir::Expr,
        values: Vec<hir::Expr>,
        span: Span,
    ) -> Option<Value> {
        if !values.len().is_multiple_of(2) {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("`{name}` takes keys and values in pairs"),
                    span,
                )
                .note("write `logger.With(\"key\", value, \"other\", value)`"),
            );
            return None;
        }
        let mut texts = Vec::new();
        let mut keys = Vec::new();
        let mut ok = true;
        for (index, value) in values.into_iter().enumerate() {
            if index % 2 == 1 {
                match self.log_text(name, value) {
                    Some(text) => texts.push(text),
                    None => ok = false,
                }
                continue;
            }
            match self.log_constant(name, "key", &value) {
                Some(key) if valid_key(&key) => {
                    keys.push((key.clone(), value.span));
                    texts.push(plain_text(key, value.span));
                }
                Some(key) => {
                    self.error(
                        format!(
                            "`{key}` is not a log key; use a letter or `_` followed by letters, digits, `_`, `.`, or `-`"
                        ),
                        value.span,
                    );
                    ok = false;
                }
                None => ok = false,
            }
        }
        if !ok || !self.check_keys(name, &keys) {
            return None;
        }
        let array = self.string_array(texts, span);
        let function = self.log_helper("withPairs");
        let logger = receiver.ty();
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::Call {
                function,
                args: vec![receiver, array],
            },
            types: vec![logger],
            span,
        }))
    }

    fn log_constant(&mut self, name: &str, what: &str, value: &hir::Expr) -> Option<String> {
        if let ExprKind::Const(Const::String(text)) = &value.kind
            && self.types.base(value.ty()) == TypeStore::STRING
        {
            return Some(text.clone());
        }
        self.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("the {what} of `{name}` must be a constant string"),
                value.span,
            )
            .note("the compiler checks each log call against its values; pass changing text as a value"),
        );
        None
    }

    fn log_texts(&mut self, name: &str, values: Vec<hir::Expr>, span: Span) -> Option<hir::Expr> {
        let mut texts = Vec::new();
        let mut ok = true;
        for value in values {
            match self.log_text(name, value) {
                Some(text) => texts.push(text),
                None => ok = false,
            }
        }
        ok.then(|| self.string_array(texts, span))
    }

    fn log_text(&mut self, name: &str, value: hir::Expr) -> Option<hir::Expr> {
        let shape = self.shape_for(
            value.ty(),
            value.span,
            name,
            "log values are bool, numbers, rune, string, error, and types with a `String() string` method",
        )?;
        Some(self.plain_value_text(value, shape))
    }

    fn string_array(&mut self, elements: Vec<hir::Expr>, span: Span) -> hir::Expr {
        let ty = self.types.dyn_array_type(TypeStore::STRING);
        typed(
            ExprKind::ArrayLit {
                element: TypeStore::STRING,
                elements,
            },
            ty,
            span,
        )
    }
}
