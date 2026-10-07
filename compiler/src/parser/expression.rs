use super::parser::{PResult, Parser};
use crate::ast::*;
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::{Keyword, Punct, TokenKind};

impl Parser<'_> {
    pub(super) fn expr_list(&mut self) -> PResult<Vec<Expr>> {
        let mut exprs = vec![self.expr()?];
        while self.eat(Punct::Comma) {
            exprs.push(self.expr()?);
        }
        Ok(exprs)
    }

    pub(super) fn expr(&mut self) -> PResult<Expr> {
        self.binary(1)
    }

    pub(super) fn binary(&mut self, min: u8) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        let mut compared = false;
        while let Some(op) = binary_op(self.peek()).filter(|op| op.precedence() >= min) {
            if op.is_comparison() && compared {
                let span = self.current_span();
                return Err(self.report(
                    Diagnostic::new(
                        Severity::Error,
                        "comparison operators cannot be chained",
                        span,
                    )
                    .note("combine comparisons with `&&`, or add parentheses"),
                ));
            }
            compared = op.is_comparison();
            self.bump();
            let rhs = self.binary(op.precedence() + 1)?;
            let span = self.span_from(lhs.span);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    pub(super) fn unary(&mut self) -> PResult<Expr> {
        let start = self.current_span();
        let op = match self.peek() {
            TokenKind::Punct(Punct::Plus) => UnaryOp::Plus,
            TokenKind::Punct(Punct::Minus) => UnaryOp::Neg,
            TokenKind::Punct(Punct::Not) => UnaryOp::Not,
            TokenKind::Punct(Punct::Caret) => UnaryOp::Complement,
            TokenKind::Keyword(Keyword::Await) => return self.await_expr(),
            _ => return self.postfix(),
        };
        self.bump();
        let operand = Box::new(self.unary()?);
        Ok(Expr {
            kind: ExprKind::Unary { op, operand },
            span: self.span_from(start),
        })
    }

    pub(super) fn await_expr(&mut self) -> PResult<Expr> {
        // `await x?` groups as `(await x)?`.
        let start = self.bump().span;
        let operand = if is_prefix(self.peek()) {
            self.unary()?
        } else {
            self.access()?
        };
        let expr = Expr {
            kind: ExprKind::Await(Box::new(operand)),
            span: self.span_from(start),
        };
        self.propagations(expr)
    }

    pub(super) fn postfix(&mut self) -> PResult<Expr> {
        let expr = self.access()?;
        self.propagations(expr)
    }

    pub(super) fn propagations(&mut self, mut expr: Expr) -> PResult<Expr> {
        while self.at(Punct::Question) {
            self.bump();
            expr = Expr {
                span: self.span_from(expr.span),
                kind: ExprKind::Try(Box::new(expr)),
            };
            if self.at(Punct::Dot) || self.at(Punct::LParen) {
                let span = self.current_span();
                return Err(self.report(
                    Diagnostic::new(
                        Severity::Error,
                        "`?` binds more loosely than calls and field access",
                        span,
                    )
                    .note("add parentheses, as in `(value?).field`"),
                ));
            }
        }
        Ok(expr)
    }

    pub(super) fn access(&mut self) -> PResult<Expr> {
        let mut expr = self.primary()?;
        let start = expr.span;
        loop {
            if self.at(Punct::LParen) {
                self.bump();
                let args = self.with_struct_literals(true, |p| {
                    p.comma_list(Punct::RParen, "argument", true, Self::expr)
                })?;
                expr = Expr {
                    span: self.span_from(expr.span),
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                };
            } else if self.at(Punct::Dot) {
                self.bump();
                let name = self.name("a field or method name")?;
                expr = Expr {
                    span: self.span_from(expr.span),
                    kind: ExprKind::Field {
                        base: Box::new(expr),
                        name,
                    },
                };
            } else if self.at(Punct::LBracket) {
                self.bump();
                let kind = self.with_struct_literals(true, |p| p.index_or_slice(expr))?;
                expr = Expr {
                    span: self.span_from(start),
                    kind,
                };
            } else {
                return Ok(expr);
            }
        }
    }

    fn index_or_slice(&mut self, base: Expr) -> PResult<ExprKind> {
        let base = Box::new(base);
        let low = if self.at(Punct::Colon) {
            None
        } else {
            let index = self.expr()?;
            if self.eat(Punct::RBracket) {
                return Ok(ExprKind::Index {
                    base,
                    index: Box::new(index),
                });
            }
            Some(Box::new(index))
        };
        self.expect(Punct::Colon)?;
        let high = if self.at(Punct::RBracket) || self.at(Punct::Colon) {
            None
        } else {
            Some(Box::new(self.expr()?))
        };
        if self.at(Punct::Colon) {
            let span = self.current_span();
            return Err(self.error(
                "a slice expression takes at most two bounds; Zore has no capacity bound or stride",
                span,
            ));
        }
        self.expect(Punct::RBracket)?;
        Ok(ExprKind::Slice { base, low, high })
    }

    pub(super) fn primary(&mut self) -> PResult<Expr> {
        let span = self.current_span();
        let kind = match self.peek().clone() {
            TokenKind::Ident
                if self.current_text() == "Array"
                    && *self.peek_at(1) == TokenKind::Punct(Punct::Lt) =>
            {
                return self.dyn_array_literal();
            }
            TokenKind::Ident => {
                let name = self.name("an expression")?;
                if self.struct_literals_allowed && self.at(Punct::LBrace) {
                    return self.struct_literal(None, name);
                }
                if self.struct_literals_allowed
                    && self.at(Punct::Dot)
                    && *self.peek_at(1) == TokenKind::Ident
                    && *self.peek_at(2) == TokenKind::Punct(Punct::LBrace)
                {
                    self.bump();
                    let member = self.name("a type name")?;
                    return self.struct_literal(Some(name), member);
                }
                return Ok(Expr {
                    kind: ExprKind::Name(name.text),
                    span,
                });
            }
            TokenKind::Int(base) => ExprKind::Int(base),
            TokenKind::Float => ExprKind::Float,
            TokenKind::String(value) => ExprKind::String(value),
            TokenKind::Rune(value) => ExprKind::Rune(value),
            TokenKind::Keyword(Keyword::True) => ExprKind::Bool(true),
            TokenKind::Keyword(Keyword::False) => ExprKind::Bool(false),
            TokenKind::Keyword(Keyword::Nil) => ExprKind::Nil,
            TokenKind::MalformedLiteral => ExprKind::Malformed,
            TokenKind::Punct(Punct::LParen) => {
                self.bump();
                let inner = self.with_struct_literals(true, Self::expr)?;
                self.expect(Punct::RParen)?;
                return Ok(Expr {
                    kind: ExprKind::Paren(Box::new(inner)),
                    span: self.span_from(span),
                });
            }
            TokenKind::Underscore => {
                return Err(self.error("`_` cannot be used as a value", span));
            }
            TokenKind::Keyword(Keyword::Func) => {
                return self.closure();
            }
            TokenKind::Keyword(Keyword::Go) => {
                return Err(self.unsupported("`go` task-creation expressions"));
            }
            TokenKind::Keyword(Keyword::Map) => {
                return self.map_literal();
            }
            TokenKind::Punct(Punct::LBracket) => {
                return self.array_literal();
            }
            TokenKind::Keyword(Keyword::Channel) => {
                return Err(self.unsupported("channel expressions"));
            }
            TokenKind::Reserved(word) => {
                let message = format!(
                    "`{}` is reserved for possible future use; the feature is not available",
                    word.as_str()
                );
                return Err(self.error(message, span));
            }
            _ => return Err(self.unexpected("an expression")),
        };
        self.bump();
        Ok(Expr { kind, span })
    }

    fn closure(&mut self) -> PResult<Expr> {
        let start = self.bump().span;
        if *self.peek() == TokenKind::Ident {
            let span = self.current_span();
            return Err(self.error(
                "a function literal has no name; declare named functions at package level",
                span,
            ));
        }
        self.expect(Punct::LParen)?;
        let params = self.comma_list(Punct::RParen, "parameter", true, |p| {
            p.param("a parameter name")
        })?;
        let results = self.results()?;
        let body = self.with_struct_literals(true, |p| {
            p.body_block(
                "function literal signature",
                "function literals require a body",
            )
        })?;
        Ok(Expr {
            kind: ExprKind::Closure(Box::new(Closure {
                params,
                results,
                body,
            })),
            span: self.span_from(start),
        })
    }

    pub(super) fn struct_literal(&mut self, package: Option<Name>, ty: Name) -> PResult<Expr> {
        let start = package.as_ref().map_or(ty.span, |package| package.span);
        self.bump();
        let fields = self.with_struct_literals(true, |p| {
            p.comma_list(Punct::RBrace, "field", true, Self::field_init)
        })?;
        Ok(Expr {
            span: self.span_from(start),
            kind: ExprKind::StructLit {
                package,
                ty,
                fields,
            },
        })
    }

    pub(super) fn array_literal(&mut self) -> PResult<Expr> {
        let span = self.current_span();
        let ty = self.bracket_type()?;
        if let Type::Slice { span, .. } = ty {
            return Err(self.error(
                "slice literals are not part of Zore; slice existing storage instead, e.g. `data[:]`",
                span,
            ));
        }
        self.expect(Punct::LBrace)?;
        let elements = self.with_struct_literals(true, |p| {
            p.comma_list(Punct::RBrace, "element", true, Self::expr)
        })?;
        Ok(Expr {
            span: self.span_from(span),
            kind: ExprKind::ArrayLit { ty, elements },
        })
    }

    fn map_literal(&mut self) -> PResult<Expr> {
        let span = self.current_span();
        let ty = self.map_type()?;
        self.expect(Punct::LBrace)?;
        let entries = self.with_struct_literals(true, |p| {
            p.comma_list(Punct::RBrace, "map entry", true, Self::map_entry)
        })?;
        Ok(Expr {
            span: self.span_from(span),
            kind: ExprKind::MapLit { ty, entries },
        })
    }

    fn map_entry(&mut self) -> PResult<MapEntry> {
        let key = self.expr()?;
        self.expect(Punct::Colon)?;
        let value = self.expr()?;
        Ok(MapEntry {
            span: self.span_from(key.span),
            key,
            value,
        })
    }

    /// Recognized by the predeclared name `Array`.
    fn dyn_array_literal(&mut self) -> PResult<Expr> {
        let span = self.current_span();
        let ty = self.ty()?;
        if !self.at(Punct::LBrace) {
            let at = self.current_span();
            return Err(self.error(
                "expected `{` after `Array<T>`; dynamic arrays are built with a typed literal such as `Array<int>{}`",
                at,
            ));
        }
        self.bump();
        let elements = self.with_struct_literals(true, |p| {
            p.comma_list(Punct::RBrace, "element", true, Self::expr)
        })?;
        Ok(Expr {
            span: self.span_from(span),
            kind: ExprKind::ArrayLit { ty, elements },
        })
    }

    pub(super) fn field_init(&mut self) -> PResult<FieldInit> {
        if !(*self.peek() == TokenKind::Ident && self.peek_at(1) == &TokenKind::Punct(Punct::Colon))
        {
            if matches!(self.peek(), TokenKind::Keyword(_) | TokenKind::Reserved(_)) {
                self.name("a field name")?;
            }
            let span = self.current_span();
            return Err(self.report(
                Diagnostic::new(Severity::Error, "struct literal fields must be named", span)
                    .note("write each field as `Name: value`"),
            ));
        }
        let name = self.name("a field name")?;
        self.bump();
        let value = self.expr()?;
        Ok(FieldInit {
            span: self.span_from(name.span),
            name,
            value,
        })
    }
}

fn is_prefix(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Punct(Punct::Plus | Punct::Minus | Punct::Not | Punct::Caret)
            | TokenKind::Keyword(Keyword::Await)
    )
}

fn binary_op(kind: &TokenKind) -> Option<BinaryOp> {
    let TokenKind::Punct(punct) = kind else {
        return None;
    };
    Some(match punct {
        Punct::Star => BinaryOp::Mul,
        Punct::Slash => BinaryOp::Div,
        Punct::Percent => BinaryOp::Rem,
        Punct::Shl => BinaryOp::Shl,
        Punct::Shr => BinaryOp::Shr,
        Punct::Amp => BinaryOp::BitAnd,
        Punct::Plus => BinaryOp::Add,
        Punct::Minus => BinaryOp::Sub,
        Punct::Pipe => BinaryOp::BitOr,
        Punct::Caret => BinaryOp::BitXor,
        Punct::EqEq => BinaryOp::Eq,
        Punct::NotEq => BinaryOp::NotEq,
        Punct::Lt => BinaryOp::Lt,
        Punct::LtEq => BinaryOp::LtEq,
        Punct::Gt => BinaryOp::Gt,
        Punct::GtEq => BinaryOp::GtEq,
        Punct::AndAnd => BinaryOp::And,
        Punct::OrOr => BinaryOp::Or,
        _ => return None,
    })
}
