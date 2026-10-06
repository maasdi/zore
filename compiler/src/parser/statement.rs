use super::parser::{PResult, Parser, Reported, StatementOrExpr};
use crate::ast::*;
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::{Keyword, Punct, Separator, TokenKind};
use crate::source::Span;

impl Parser<'_> {
    pub(super) fn binding(&mut self) -> PResult<Binding> {
        let start = self.current_span();
        let kind = match self.bump().kind {
            TokenKind::Keyword(Keyword::Let) => BindingKind::Let,
            TokenKind::Keyword(Keyword::Var) => BindingKind::Var,
            _ => BindingKind::Const,
        };
        let mut targets = vec![self.binding_target(kind)?];
        while self.eat(Punct::Comma) {
            targets.push(self.binding_target(kind)?);
        }
        if kind == BindingKind::Const && targets.len() > 1 {
            let span = self.span_from(start);
            return Err(self.error("a constant declaration has exactly one name", span));
        }
        let ty = if self.at(Punct::Eq) || self.at_separator() {
            None
        } else if self.at(Punct::Colon) && *self.peek_at(1) == TokenKind::Punct(Punct::Eq) {
            let span = self.current_span();
            return Err(self.report(
                Diagnostic::new(Severity::Error, "`:=` is not Zore syntax", span)
                    .note("declare with `let name = value` or `var name = value`"),
            ));
        } else if self.at(Punct::Colon) {
            let span = self.current_span();
            return Err(self.report(
                Diagnostic::new(
                    Severity::Error,
                    "type annotations are not written with `:`",
                    span,
                )
                .note("write the type after the name, as in `let count int64 = 0`"),
            ));
        } else if targets.len() > 1 {
            let span = self.current_span();
            return Err(self.error(
                "typed multiple bindings are not supported; use separate declarations",
                span,
            ));
        } else {
            Some(self.ty()?)
        };
        if !self.at(Punct::Eq) {
            if self.at_separator() || self.at(Punct::RBrace) || *self.peek() == TokenKind::Eof {
                let span = self.span_from(start);
                return Err(self.error("declarations require an initializer (`= value`)", span));
            }
            return Err(self.unexpected("`=`"));
        }
        self.bump();
        let value = self.expr()?;
        if self.at(Punct::Comma) {
            let span = self.current_span();
            return Err(self.error(
                "a declaration takes exactly one initializer expression",
                span,
            ));
        }
        Ok(Binding {
            kind,
            targets,
            ty,
            value,
            span: self.span_from(start),
        })
    }

    pub(super) fn binding_target(&mut self, kind: BindingKind) -> PResult<BindingTarget> {
        if *self.peek() == TokenKind::Underscore && kind != BindingKind::Const {
            return Ok(BindingTarget::Discard(self.bump().span));
        }
        self.name("a binding name").map(BindingTarget::Name)
    }

    pub(super) fn body_open(&mut self, header: &str) -> PResult<Span> {
        if self.at_newline_before(&TokenKind::Punct(Punct::LBrace)) {
            let span = self.current_span();
            return Err(self.report(
                Diagnostic::new(
                    Severity::Error,
                    format!("`{{` must be on the same line as the {header}"),
                    span,
                )
                .note("a newline here ends the statement (automatic semicolon insertion)"),
            ));
        }
        self.expect(Punct::LBrace)
    }

    pub(super) fn body_block(&mut self, header: &str, missing: &str) -> PResult<Block> {
        if self.at_separator() && !self.at_newline_before(&TokenKind::Punct(Punct::LBrace))
            || *self.peek() == TokenKind::Eof
        {
            let span = self.current_span();
            return Err(self.error(missing, span));
        }
        let open = self.body_open(header)?;
        Ok(self.block_rest(open))
    }

    pub(super) fn block(&mut self) -> PResult<Block> {
        let open = self.expect(Punct::LBrace)?;
        Ok(self.block_rest(open))
    }

    pub(super) fn unclosed(&mut self, open: Span) -> Reported {
        let span = self.current_span();
        self.report(
            Diagnostic::new(Severity::Error, "unclosed `{`", span)
                .primary_message("file ends here")
                .related(open, "opened here"),
        )
    }

    pub(super) fn block_rest(&mut self, open: Span) -> Block {
        let mut stmts = Vec::new();
        loop {
            match self.peek() {
                TokenKind::Punct(Punct::RBrace) => {
                    self.bump();
                    break;
                }
                TokenKind::Eof => {
                    self.unclosed(open);
                    break;
                }
                _ => {}
            }
            let (depth, before) = (self.open_delimiters, self.pos);
            let ok = match self.stmt() {
                Ok(stmt) => {
                    stmts.push(stmt);
                    self.statement_end("statement")
                }
                Err(Reported) => false,
            };
            if !ok {
                self.synchronize(depth, true);
            }
            debug_assert!(self.pos > before, "statement recovery must make progress");
        }
        Block {
            stmts,
            span: self.span_from(open),
        }
    }

    pub(super) fn statement_end(&mut self, what: &str) -> bool {
        match self.peek() {
            TokenKind::Semicolon(_) => {
                self.bump();
                true
            }
            TokenKind::Punct(Punct::RBrace) => true,
            _ => {
                self.unexpected(&format!("newline or `;` after {what}"));
                false
            }
        }
    }

    pub(super) fn stmt(&mut self) -> PResult<Stmt> {
        let start = self.current_span();
        let kind = match self.peek() {
            TokenKind::Keyword(Keyword::Let | Keyword::Var | Keyword::Const) => {
                StmtKind::Binding(self.binding()?)
            }
            TokenKind::Keyword(Keyword::Return) => {
                self.bump();
                if self.at_separator() || self.at(Punct::RBrace) || *self.peek() == TokenKind::Eof {
                    StmtKind::Return(Vec::new())
                } else {
                    StmtKind::Return(self.expr_list()?)
                }
            }
            TokenKind::Keyword(Keyword::Break) => {
                self.bump();
                StmtKind::Break
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.bump();
                StmtKind::Continue
            }
            TokenKind::Keyword(Keyword::If) => StmtKind::If(self.if_stmt()?),
            TokenKind::Keyword(Keyword::For) => StmtKind::For(self.for_stmt()?),
            TokenKind::Punct(Punct::LBrace) => StmtKind::Block(self.block()?),
            TokenKind::Semicolon(Separator::Explicit) => {
                return Err(self.error("empty statement", start));
            }
            _ => {
                return self
                    .simple_statement()
                    .and_then(|simple| self.statement(simple));
            }
        };
        Ok(Stmt {
            kind,
            span: self.span_from(start),
        })
    }

    pub(super) fn statement(&mut self, simple: StatementOrExpr) -> PResult<Stmt> {
        match simple {
            StatementOrExpr::Stmt(stmt) => Ok(stmt),
            StatementOrExpr::Expr(expr) if is_call_based(&expr) => Ok(Stmt {
                span: expr.span,
                kind: StmtKind::Expr(expr),
            }),
            StatementOrExpr::Expr(expr) => Err(self.report(
                Diagnostic::new(Severity::Error, "expression is not a statement", expr.span).note(
                    "only calls can be statements; discard a value explicitly with `_ = value`",
                ),
            )),
        }
    }

    pub(super) fn simple_statement(&mut self) -> PResult<StatementOrExpr> {
        let start = self.current_span();
        let mut targets = vec![self.assign_target()?];
        while self.eat(Punct::Comma) {
            targets.push(self.assign_target()?);
        }
        let Some(op) = assign_op(self.peek()) else {
            return match targets.pop() {
                Some(AssignTarget::Place(expr)) if targets.is_empty() => {
                    Ok(StatementOrExpr::Expr(expr))
                }
                _ => Err(self.unexpected("`=` after assignment targets")),
            };
        };
        let op_span = self.bump().span;
        for target in &targets {
            if let AssignTarget::Place(expr) = target
                && !matches!(
                    expr.kind,
                    ExprKind::Name(_) | ExprKind::Field { .. } | ExprKind::Index { .. }
                )
            {
                return Err(self.error("invalid assignment target", expr.span));
            }
        }
        if let AssignOp::Compound(_) = op {
            if targets.len() > 1 {
                return Err(self.error("compound assignment takes exactly one target", op_span));
            }
            if let AssignTarget::Discard(span) = targets[0] {
                return Err(self.error("`_` cannot be a compound assignment target", span));
            }
        }
        let values = self.expr_list()?;
        if matches!(op, AssignOp::Compound(_)) && values.len() > 1 {
            return Err(self.error(
                "compound assignment takes exactly one value",
                values[1].span,
            ));
        }
        Ok(StatementOrExpr::Stmt(Stmt {
            kind: StmtKind::Assign {
                targets,
                op,
                values,
            },
            span: self.span_from(start),
        }))
    }

    pub(super) fn assign_target(&mut self) -> PResult<AssignTarget> {
        if *self.peek() == TokenKind::Underscore {
            return Ok(AssignTarget::Discard(self.bump().span));
        }
        self.expr().map(AssignTarget::Place)
    }

    pub(super) fn if_stmt(&mut self) -> PResult<If> {
        let start = self.bump().span;
        let condition = self.header_expr()?;
        let then_block = self.body_block("`if` condition", "expected `{` after `if` condition")?;
        if self.at_newline_before(&TokenKind::Keyword(Keyword::Else)) {
            let span = self.current_span();
            return Err(self.report(
                Diagnostic::new(
                    Severity::Error,
                    "`else` must be on the same line as the preceding `}`",
                    span,
                )
                .note("a newline here ends the `if` statement (automatic semicolon insertion)"),
            ));
        }
        let else_branch = if self.at_keyword(Keyword::Else) {
            self.bump();
            if self.at_keyword(Keyword::If) {
                Some(Else::If(Box::new(self.if_stmt()?)))
            } else {
                Some(Else::Block(
                    self.body_block("`else`", "expected `{` after `else`")?,
                ))
            }
        } else {
            None
        };
        Ok(If {
            condition,
            then_block,
            else_branch,
            span: self.span_from(start),
        })
    }

    pub(super) fn for_stmt(&mut self) -> PResult<For> {
        self.bump();
        if self.at(Punct::LBrace) {
            let open = self.bump().span;
            return Ok(For {
                header: ForHeader::Infinite,
                body: self.block_rest(open),
            });
        }
        if self.at_separator() {
            let span = self.current_span();
            return Err(self.error("counting loop clauses cannot be empty", span));
        }
        if self.at_each_header() {
            let first = self.binding_target(BindingKind::Let)?;
            let second = if self.eat(Punct::Comma) {
                Some(self.binding_target(BindingKind::Let)?)
            } else {
                None
            };
            self.bump();
            let collection = self.header_expr()?;
            let body = self.body_block("`for` header", "expected `{` after `for` header")?;
            return Ok(For {
                header: ForHeader::Each {
                    first,
                    second,
                    collection,
                },
                body,
            });
        }
        let first = self.for_first_clause()?;
        let header = if *self.peek() == TokenKind::Semicolon(Separator::Explicit) {
            self.bump();
            let init = Box::new(self.statement(first)?);
            let condition = self.header_expr()?;
            self.header_separator()?;
            if matches!(
                self.peek(),
                TokenKind::Keyword(Keyword::Let | Keyword::Var | Keyword::Const)
            ) {
                let span = self.current_span();
                return Err(self.error("the counting loop update cannot be a declaration", span));
            }
            let update = self.with_struct_literals(false, |p| p.simple_statement())?;
            let update = Box::new(self.statement(update)?);
            ForHeader::Counting {
                init,
                condition,
                update,
            }
        } else {
            match first {
                StatementOrExpr::Expr(condition) => ForHeader::Condition(condition),
                StatementOrExpr::Stmt(_) => {
                    return Err(self.unexpected("`;` after the loop initializer"));
                }
            }
        };
        let body = self.body_block("`for` header", "expected `{` after `for` header")?;
        Ok(For { header, body })
    }

    fn at_each_header(&self) -> bool {
        let is_name = |kind: &TokenKind| matches!(kind, TokenKind::Ident | TokenKind::Underscore);
        let is_in = |kind: &TokenKind| *kind == TokenKind::Keyword(Keyword::In);
        is_name(self.peek())
            && (is_in(self.peek_at(1))
                || *self.peek_at(1) == TokenKind::Punct(Punct::Comma)
                    && is_name(self.peek_at(2))
                    && is_in(self.peek_at(3)))
    }

    pub(super) fn for_first_clause(&mut self) -> PResult<StatementOrExpr> {
        if matches!(self.peek(), TokenKind::Keyword(Keyword::Let | Keyword::Var)) {
            let start = self.current_span();
            let binding = self.binding()?;
            return Ok(StatementOrExpr::Stmt(Stmt {
                kind: StmtKind::Binding(binding),
                span: self.span_from(start),
            }));
        }
        let (pos, open_delimiters, last_token_end, reported) = (
            self.pos,
            self.open_delimiters,
            self.last_token_end,
            self.diagnostics.len(),
        );
        // An initializer may contain struct literals; retry with them allowed.
        let first = self.with_struct_literals(false, |p| p.simple_statement());
        if matches!(first, Ok(StatementOrExpr::Stmt(_))) && self.at(Punct::LBrace) {
            (self.pos, self.open_delimiters, self.last_token_end) =
                (pos, open_delimiters, last_token_end);
            self.diagnostics.truncate(reported);
            return self.simple_statement();
        }
        first
    }

    pub(super) fn header_separator(&mut self) -> PResult<()> {
        match self.peek() {
            TokenKind::Semicolon(Separator::Explicit) => {
                self.bump();
                Ok(())
            }
            TokenKind::Semicolon(_) => {
                let span = self.current_span();
                Err(self.error("counting loop headers must be written on one line", span))
            }
            _ => Err(self.unexpected("`;` in counting loop header")),
        }
    }

    pub(super) fn header_expr(&mut self) -> PResult<Expr> {
        self.with_struct_literals(false, Self::expr)
    }
}

fn is_call_based(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Call { .. } => true,
        ExprKind::Await(inner) | ExprKind::Try(inner) => is_call_based(inner),
        _ => false,
    }
}

fn assign_op(kind: &TokenKind) -> Option<AssignOp> {
    let TokenKind::Punct(punct) = kind else {
        return None;
    };
    let op = match punct {
        Punct::Eq => return Some(AssignOp::Assign),
        Punct::PlusEq => BinaryOp::Add,
        Punct::MinusEq => BinaryOp::Sub,
        Punct::StarEq => BinaryOp::Mul,
        Punct::SlashEq => BinaryOp::Div,
        Punct::PercentEq => BinaryOp::Rem,
        Punct::AmpEq => BinaryOp::BitAnd,
        Punct::PipeEq => BinaryOp::BitOr,
        Punct::CaretEq => BinaryOp::BitXor,
        Punct::ShlEq => BinaryOp::Shl,
        Punct::ShrEq => BinaryOp::Shr,
        _ => return None,
    };
    Some(AssignOp::Compound(op))
}
