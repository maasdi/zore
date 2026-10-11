use super::{Checker, Value, typed};
use crate::ast::{self, BinaryOp, ParamMode};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{self, Const, ExprKind};
use crate::resolve::FunctionId;
use crate::source::Span;
use crate::types::{TypeId, TypeKind, TypeStore};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum FormatKind {
    Print,
    Println,
    Printf,
    Sprint,
    Sprintln,
    Sprintf,
    Errorf,
}

impl FormatKind {
    fn name(self) -> &'static str {
        match self {
            Self::Print => "fmt.Print",
            Self::Println => "fmt.Println",
            Self::Printf => "fmt.Printf",
            Self::Sprint => "fmt.Sprint",
            Self::Sprintln => "fmt.Sprintln",
            Self::Sprintf => "fmt.Sprintf",
            Self::Errorf => "fmt.Errorf",
        }
    }

    fn has_format(self) -> bool {
        matches!(self, Self::Printf | Self::Sprintf | Self::Errorf)
    }
}

enum Piece {
    Text(String),
    Verb(Verb),
}

#[derive(Clone, Copy)]
struct Verb {
    letter: char,
    width: Option<i128>,
    precision: Option<i128>,
    left: bool,
    zero: bool,
}

/// How a value of one type is turned into text.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Stringer(FunctionId),
    Bool,
    Signed,
    Unsigned,
    Rune,
    Float(u8),
    Text,
    Error,
}

fn parse_format(text: &str) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            literal.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            literal.push('%');
            continue;
        }
        let mut verb = Verb {
            letter: ' ',
            width: None,
            precision: None,
            left: false,
            zero: false,
        };
        while let Some(&flag) = chars.peek() {
            match flag {
                '-' => verb.left = true,
                '0' => verb.zero = true,
                _ => break,
            }
            chars.next();
        }
        let number = |chars: &mut std::iter::Peekable<std::str::Chars>| {
            let mut value: Option<i128> = None;
            while let Some(digit) = chars.peek().and_then(|c| c.to_digit(10)) {
                value = Some(value.unwrap_or(0).saturating_mul(10) + i128::from(digit));
                chars.next();
            }
            value
        };
        verb.width = number(&mut chars);
        if chars.peek() == Some(&'.') {
            chars.next();
            verb.precision = Some(number(&mut chars).unwrap_or(0));
        }
        let Some(letter) = chars.next() else {
            return Err("the format ends in the middle of a `%` directive".into());
        };
        if !"vdxXobcqstfeEgGw".contains(letter) {
            return Err(format!("`%{letter}` is not a formatting verb"));
        }
        verb.letter = letter;
        if !literal.is_empty() {
            pieces.push(Piece::Text(std::mem::take(&mut literal)));
        }
        pieces.push(Piece::Verb(verb));
    }
    if !literal.is_empty() {
        pieces.push(Piece::Text(literal));
    }
    Ok(pieces)
}

fn allowed(shape: Shape, letter: char) -> bool {
    match shape {
        Shape::Stringer(_) => "vsq".contains(letter),
        Shape::Bool => "vt".contains(letter),
        Shape::Signed | Shape::Unsigned => "vdxXobc".contains(letter),
        Shape::Rune => "vcqdxXob".contains(letter),
        Shape::Float(_) => "vfeEgG".contains(letter),
        Shape::Text => "vsq".contains(letter),
        Shape::Error => "vsw".contains(letter),
    }
}

impl Checker<'_> {
    pub(super) fn format_kind(&self, id: FunctionId) -> Option<FormatKind> {
        let index = id.0 as usize;
        if index >= self.res.functions.len()
            || self.res.packages[self.res.function_package[index]].path != "zore/fmt"
        {
            return None;
        }
        Some(match self.res.functions[index].name.text.as_str() {
            "Print" => FormatKind::Print,
            "Println" => FormatKind::Println,
            "Printf" => FormatKind::Printf,
            "Sprint" => FormatKind::Sprint,
            "Sprintln" => FormatKind::Sprintln,
            "Sprintf" => FormatKind::Sprintf,
            "Errorf" => FormatKind::Errorf,
            _ => return None,
        })
    }

    fn fmt_helper(&self, name: &str) -> FunctionId {
        let index = (0..self.res.functions.len())
            .find(|&index| {
                self.res.packages[self.res.function_package[index]].path == "zore/fmt"
                    && self.res.functions[index].name.text == name
            })
            .expect("the bundled fmt package declares its helpers");
        FunctionId(index as u32)
    }

    fn helper_call(
        &self,
        name: &str,
        args: Vec<hir::Expr>,
        result: TypeId,
        span: Span,
    ) -> hir::Expr {
        hir::Expr {
            kind: ExprKind::Call {
                function: self.fmt_helper(name),
                args,
            },
            types: vec![result],
            span,
        }
    }

    /// A call of `fmt.Print`, `Printf`, and the others, checked against its format and turned into text.
    pub(super) fn format_call(
        &mut self,
        kind: FormatKind,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        let mut values = Vec::new();
        let mut ok = true;
        for arg in args {
            match self
                .expr(arg, None)
                .and_then(|value| self.with_default_type(value))
            {
                Some(expr) => values.push(expr),
                None => ok = false,
            }
        }
        if !ok {
            return None;
        }
        let text = if kind.has_format() {
            self.formatted_text(kind, values, span)?
        } else {
            self.plain_text(kind, values, span)?
        };
        Some(Value::Typed(match kind {
            FormatKind::Print | FormatKind::Println | FormatKind::Printf => hir::Expr {
                kind: ExprKind::Call {
                    function: self.fmt_helper("writeOut"),
                    args: vec![text],
                },
                types: Vec::new(),
                span,
            },
            FormatKind::Sprint | FormatKind::Sprintln | FormatKind::Sprintf => text,
            FormatKind::Errorf => text,
        }))
    }

    fn plain_text(
        &mut self,
        kind: FormatKind,
        values: Vec<hir::Expr>,
        span: Span,
    ) -> Option<hir::Expr> {
        let mut text: Option<hir::Expr> = None;
        let mut previous: Option<Shape> = None;
        let lines = matches!(kind, FormatKind::Println | FormatKind::Sprintln);
        for value in values {
            let shape = self.shape(value.ty(), value.span, kind)?;
            let spaced = match previous {
                None => false,
                Some(_) if lines => true,
                Some(before) => before != Shape::Text && shape != Shape::Text,
            };
            if spaced {
                text = Some(self.join(text, self.text_const(" ", span), span));
            }
            let piece = self.value_text(
                value,
                shape,
                Verb {
                    letter: 'v',
                    width: None,
                    precision: None,
                    left: false,
                    zero: false,
                },
            );
            text = Some(self.join(text, piece, span));
            previous = Some(shape);
        }
        if lines {
            text = Some(self.join(text, self.text_const("\n", span), span));
        }
        Some(text.unwrap_or_else(|| self.text_const("", span)))
    }

    fn formatted_text(
        &mut self,
        kind: FormatKind,
        mut values: Vec<hir::Expr>,
        span: Span,
    ) -> Option<hir::Expr> {
        if values.is_empty() {
            self.error(format!("`{}` needs a format string", kind.name()), span);
            return None;
        }
        let format = values.remove(0);
        let ExprKind::Const(Const::String(format_text)) = &format.kind else {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("the format of `{}` must be a constant string", kind.name()),
                    format.span,
                )
                .note("the compiler checks each `%` directive against its argument"),
            );
            return None;
        };
        if self.types.base(format.ty()) != TypeStore::STRING {
            self.error(
                format!("the format of `{}` must be a string", kind.name()),
                format.span,
            );
            return None;
        }
        let pieces = match parse_format(format_text) {
            Ok(pieces) => pieces,
            Err(message) => {
                self.error(format!("`{}` format: {message}", kind.name()), format.span);
                return None;
            }
        };
        let verbs = pieces
            .iter()
            .filter(|p| matches!(p, Piece::Verb(_)))
            .count();
        if verbs != values.len() {
            let message = format!(
                "`{}` format has {verbs} directive{} but {} argument{} {} given",
                kind.name(),
                if verbs == 1 { "" } else { "s" },
                values.len(),
                if values.len() == 1 { "" } else { "s" },
                if values.len() == 1 { "was" } else { "were" },
            );
            self.error(message, span);
            return None;
        }
        let mut text: Option<hir::Expr> = None;
        let mut cause: Option<(Option<hir::Expr>, hir::Expr)> = None;
        let mut values = values.into_iter();
        let mut ok = true;
        for piece in pieces {
            match piece {
                Piece::Text(literal) => {
                    text = Some(self.join(text, self.text_const(&literal, span), span));
                }
                Piece::Verb(verb) => {
                    let value = values.next().expect("counted above");
                    let Some(shape) = self.shape(value.ty(), value.span, kind) else {
                        ok = false;
                        continue;
                    };
                    if !allowed(shape, verb.letter)
                        || (verb.letter == 'w' && kind != FormatKind::Errorf)
                    {
                        let message = format!(
                            "`{}` directive `%{}` cannot format a value of type `{}`",
                            kind.name(),
                            verb.letter,
                            self.name(value.ty())
                        );
                        let note = if verb.letter == 'w' {
                            "`%w` wraps an `error`, and only `fmt.Errorf` takes it"
                        } else {
                            "`%v` formats any value fmt accepts"
                        };
                        self.diagnostics
                            .push(Diagnostic::new(Severity::Error, message, value.span).note(note));
                        ok = false;
                        continue;
                    }
                    if verb.precision.is_some() && !matches!(shape, Shape::Float(_)) {
                        let message = format!(
                            "`{}` directive `%{}` takes no precision for a value of type `{}`",
                            kind.name(),
                            verb.letter,
                            self.name(value.ty())
                        );
                        self.error(message, value.span);
                        ok = false;
                        continue;
                    }
                    if verb.letter == 'w' {
                        if cause.is_some() {
                            self.error("`fmt.Errorf` takes at most one `%w` directive", value.span);
                            ok = false;
                            continue;
                        }
                        cause = Some((text.take(), value));
                        continue;
                    }
                    let piece = self.value_text(value, shape, verb);
                    text = Some(self.join(text, piece, span));
                }
            }
        }
        if !ok {
            return None;
        }
        if kind != FormatKind::Errorf {
            return Some(text.unwrap_or_else(|| self.text_const("", span)));
        }
        Some(match cause {
            Some((before, cause)) => {
                let before = before.unwrap_or_else(|| self.text_const("", span));
                let after = text.unwrap_or_else(|| self.text_const("", span));
                self.helper_call(
                    "wrapFormatted",
                    vec![before, cause, after],
                    TypeStore::ERROR,
                    span,
                )
            }
            None => typed(
                ExprKind::Error(Box::new(text.unwrap_or_else(|| self.text_const("", span)))),
                TypeStore::ERROR,
                span,
            ),
        })
    }

    fn shape(&mut self, ty: TypeId, span: Span, kind: FormatKind) -> Option<Shape> {
        if let Some(method) = self.method_of(ty, "String")
            && self.is_string_method(method)
        {
            return Some(Shape::Stringer(method));
        }
        let shape = match self.types.kind(ty) {
            TypeKind::Bool => Shape::Bool,
            TypeKind::Int(int) if int.signed => Shape::Signed,
            TypeKind::Int(_) => Shape::Unsigned,
            TypeKind::Rune => Shape::Rune,
            TypeKind::Float(float) => Shape::Float(float.bits),
            TypeKind::String => Shape::Text,
            TypeKind::Error => Shape::Error,
            _ => {
                let message = format!(
                    "`{}` cannot format a value of type `{}`",
                    kind.name(),
                    self.name(ty)
                );
                self.diagnostics.push(Diagnostic::new(Severity::Error, message, span).note(
                    "fmt formats bool, numbers, rune, string, error, and types with a `String() string` method",
                ));
                return None;
            }
        };
        Some(shape)
    }

    fn is_string_method(&self, method: FunctionId) -> bool {
        let declaration = self.res.functions[method.0 as usize];
        let Some(signature) = &self.signatures[method.0 as usize] else {
            return false;
        };
        !declaration.is_async
            && signature.params.len() == 1
            && signature.results == [TypeStore::STRING]
            && declaration
                .receiver
                .as_ref()
                .is_some_and(|receiver| receiver.mode == ParamMode::Borrow)
    }

    fn value_text(&mut self, value: hir::Expr, shape: Shape, verb: Verb) -> hir::Expr {
        let span = value.span;
        let letter = typed(
            ExprKind::Const(Const::Rune(verb.letter)),
            TypeStore::RUNE,
            span,
        );
        let text = match shape {
            Shape::Stringer(method) => {
                let call = self.string_method_call(method, value);
                if verb.letter == 'q' {
                    self.helper_call("formatString", vec![call, letter], TypeStore::STRING, span)
                } else {
                    call
                }
            }
            Shape::Bool => {
                let value = self.retyped(value, TypeStore::BOOL);
                self.helper_call("formatBool", vec![value], TypeStore::STRING, span)
            }
            Shape::Signed => {
                let value = self.retyped(value, TypeStore::INT64);
                self.helper_call("formatInt", vec![value, letter], TypeStore::STRING, span)
            }
            Shape::Unsigned => {
                let value = self.retyped(value, TypeStore::UINT64);
                self.helper_call("formatUint", vec![value, letter], TypeStore::STRING, span)
            }
            Shape::Rune => {
                let value = self.retyped(value, TypeStore::RUNE);
                self.helper_call("formatRune", vec![value, letter], TypeStore::STRING, span)
            }
            Shape::Float(bits) => {
                let value = self.retyped(value, TypeStore::FLOAT64);
                let precision = typed(
                    ExprKind::Const(Const::Int(verb.precision.unwrap_or(-1))),
                    TypeStore::INT64,
                    span,
                );
                let bits = typed(
                    ExprKind::Const(Const::Int(i128::from(bits))),
                    TypeStore::INT64,
                    span,
                );
                self.helper_call(
                    "formatFloat",
                    vec![value, letter, precision, bits],
                    TypeStore::STRING,
                    span,
                )
            }
            Shape::Text => {
                let value = self.retyped(value, TypeStore::STRING);
                if verb.letter == 'q' {
                    self.helper_call("formatString", vec![value, letter], TypeStore::STRING, span)
                } else {
                    value
                }
            }
            Shape::Error => self.helper_call("formatError", vec![value], TypeStore::STRING, span),
        };
        let Some(width) = verb.width else {
            return text;
        };
        let width = typed(ExprKind::Const(Const::Int(width)), TypeStore::INT64, span);
        let left = typed(
            ExprKind::Const(Const::Bool(verb.left)),
            TypeStore::BOOL,
            span,
        );
        let zero = typed(
            ExprKind::Const(Const::Bool(verb.zero)),
            TypeStore::BOOL,
            span,
        );
        self.helper_call(
            "pad",
            vec![text, width, left, zero],
            TypeStore::STRING,
            span,
        )
    }

    fn string_method_call(&mut self, method: FunctionId, receiver: hir::Expr) -> hir::Expr {
        let span = receiver.span;
        let kind = if self.is_generic(method) {
            let receiver_param = self.signatures[method.0 as usize]
                .as_ref()
                .expect("checked above")
                .params[0];
            let bound = self.receiver_type_arguments(method, &receiver_param, receiver.ty());
            let type_args = self.res.type_params[&method]
                .iter()
                .map(|param| bound.get(param).copied().unwrap_or(*param))
                .collect();
            ExprKind::CallGeneric {
                function: method,
                type_args,
                args: vec![receiver],
            }
        } else {
            ExprKind::Call {
                function: method,
                args: vec![receiver],
            }
        };
        hir::Expr {
            kind,
            types: vec![TypeStore::STRING],
            span,
        }
    }

    /// The value with the predeclared type its formatting helper takes.
    fn retyped(&self, value: hir::Expr, target: TypeId) -> hir::Expr {
        if value.ty() == target {
            return value;
        }
        let span = value.span;
        match value.kind {
            ExprKind::Const(c) if self.types.base(value.types[0]) == self.types.base(target) => {
                typed(ExprKind::Const(c), target, span)
            }
            _ => typed(ExprKind::Convert(Box::new(value)), target, span),
        }
    }

    fn text_const(&self, text: &str, span: Span) -> hir::Expr {
        typed(
            ExprKind::Const(Const::String(text.to_string())),
            TypeStore::STRING,
            span,
        )
    }

    fn join(&self, left: Option<hir::Expr>, right: hir::Expr, span: Span) -> hir::Expr {
        match left {
            None => right,
            Some(left) => typed(
                ExprKind::Binary {
                    op: BinaryOp::Add,
                    lhs: Box::new(left),
                    rhs: Box::new(right),
                },
                TypeStore::STRING,
                span,
            ),
        }
    }
}
