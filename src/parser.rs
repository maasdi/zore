//! Tokens to syntax tree for the M3–M4 subset (spec §3.2–3.3, §5, §7–9).
//!
//! Supported: package clause, imports, functions and methods, struct types,
//! `let`/`var`/`const`, assignments, calls, `return`, `if`, `for`,
//! `break`/`continue`, blocks, and the §7.6 operators. Syntax owned by later
//! milestones (collections, closures, `go`, generic type arguments) is
//! diagnosed as unsupported rather than guessed.
//!
//! Errors are recovered at statement and declaration boundaries. A file is
//! syntactically valid only when no diagnostics are reported.

use crate::ast::*;
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::lex;
use crate::source::{SourceFile, Span};
use crate::token::{Keyword, Punct, Separator, Token, TokenKind};

#[derive(Debug)]
pub struct Parsed {
    pub file: File,
    /// Lexical diagnostics followed by syntax diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(file: &SourceFile) -> Parsed {
    let lexed = lex(file);
    let mut parser = Parser {
        file,
        tokens: lexed.tokens,
        pos: 0,
        last_end: 0,
        open: 0,
        struct_literals: true,
        diagnostics: lexed.diagnostics,
    };
    let ast = parser.file_ast();
    Parsed {
        file: ast,
        diagnostics: parser.diagnostics,
    }
}

/// Marker for an error already reported; the caller resynchronizes.
struct Recover;

type PResult<T> = Result<T, Recover>;

/// A simple statement before the caller decides how a bare expression is used.
enum Simple {
    Stmt(Stmt),
    Expr(Expr),
}

struct Parser<'a> {
    file: &'a SourceFile,
    tokens: Vec<Token>,
    pos: usize,
    /// End offset of the last consumed source token (not inserted separators).
    last_end: u32,
    /// Currently open `(`, `[`, and `{` tokens, used to resynchronize.
    open: usize,
    /// False in `if`/`for` headers, where `Name {` starts the body (§8.4).
    struct_literals: bool,
    diagnostics: Vec<Diagnostic>,
}

impl Parser<'_> {
    // ----- token access -----

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at(&self, ahead: usize) -> &TokenKind {
        let index = (self.pos + ahead).min(self.tokens.len() - 1);
        &self.tokens[index].kind
    }

    fn current_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        match token.kind {
            TokenKind::Eof => return token,
            TokenKind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace) => self.open += 1,
            TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => {
                self.open = self.open.saturating_sub(1);
            }
            _ => {}
        }
        if !token.span.is_empty() {
            self.last_end = token.span.end();
        }
        self.pos += 1;
        token
    }

    fn at(&self, punct: Punct) -> bool {
        *self.peek() == TokenKind::Punct(punct)
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        *self.peek() == TokenKind::Keyword(keyword)
    }

    fn at_separator(&self) -> bool {
        matches!(self.peek(), TokenKind::Semicolon(_))
    }

    fn at_newline_before(&self, next: &TokenKind) -> bool {
        *self.peek() == TokenKind::Semicolon(Separator::Newline) && self.peek_at(1) == next
    }

    fn eat(&mut self, punct: Punct) -> bool {
        let found = self.at(punct);
        if found {
            self.bump();
        }
        found
    }

    fn span_from(&self, start: Span) -> Span {
        let end = self.last_end.max(start.end());
        self.file
            .span(start.start(), end)
            .expect("parser spans cover consumed tokens")
    }

    // ----- diagnostics -----

    fn report(&mut self, diagnostic: Diagnostic) -> Recover {
        self.diagnostics.push(diagnostic);
        Recover
    }

    fn error(&mut self, message: impl Into<String>, span: Span) -> Recover {
        self.report(Diagnostic::new(Severity::Error, message, span))
    }

    /// Report an unexpected current token unless the lexer already did.
    fn unexpected(&mut self, expected: &str) -> Recover {
        if matches!(
            self.peek(),
            TokenKind::Unknown | TokenKind::MalformedLiteral
        ) {
            return Recover;
        }
        let found = self.describe_current();
        let span = self.current_span();
        self.error(format!("expected {expected}, found {found}"), span)
    }

    fn describe_current(&self) -> String {
        let token = &self.tokens[self.pos];
        let text = self.file.text();
        let source = &text[token.span.start() as usize..token.span.end() as usize];
        match &token.kind {
            TokenKind::Ident => format!("identifier `{source}`"),
            TokenKind::Underscore => "`_`".into(),
            TokenKind::Keyword(k) => format!("keyword `{}`", k.as_str()),
            TokenKind::Reserved(w) => format!("reserved word `{}`", w.as_str()),
            TokenKind::Int(_) | TokenKind::Float => format!("number `{source}`"),
            TokenKind::String(_) => "string literal".into(),
            TokenKind::Rune(_) => "rune literal".into(),
            TokenKind::MalformedLiteral => "malformed literal".into(),
            TokenKind::Unknown => format!("`{source}`"),
            TokenKind::Punct(p) => format!("`{}`", p.as_str()),
            TokenKind::Semicolon(Separator::Explicit) => "`;`".into(),
            TokenKind::Semicolon(Separator::Newline) => "newline".into(),
            TokenKind::Semicolon(Separator::Eof) | TokenKind::Eof => "end of file".into(),
        }
    }

    fn unsupported(&mut self, what: &str, milestone: &str) -> Recover {
        let span = self.current_span();
        self.report(
            Diagnostic::new(
                Severity::Error,
                format!("{what} are not supported by this compiler yet"),
                span,
            )
            .note(format!("planned for roadmap milestone {milestone}")),
        )
    }

    fn expect(&mut self, punct: Punct) -> PResult<Span> {
        if self.at(punct) {
            return Ok(self.bump().span);
        }
        Err(self.unexpected(&format!("`{}`", punct.as_str())))
    }

    /// A name in a declaration position (§3.5, §3.15–3.17, §5.5).
    fn name(&mut self, what: &str) -> PResult<Name> {
        let span = self.current_span();
        let message = match self.peek().clone() {
            TokenKind::Ident => {
                let text = self.file.text()[span.start() as usize..span.end() as usize].to_owned();
                self.bump();
                return Ok(Name { text, span });
            }
            TokenKind::Keyword(k) => {
                format!("`{}` is a keyword and cannot be used as {what}", k.as_str())
            }
            TokenKind::Reserved(w) => format!(
                "`{}` is reserved for possible future use and cannot be used as {what}",
                w.as_str()
            ),
            TokenKind::Underscore => format!("`_` cannot be used as {what}"),
            _ => return Err(self.unexpected(what)),
        };
        Err(self.error(message, span))
    }

    /// Skip to the end of the construct that started at nesting `depth`:
    /// past a separator at that depth, or before a `}` that closes it. Between
    /// declarations (`stop_at_brace` unset), also stop before a declaration
    /// keyword that starts a line, so an unclosed delimiter in one declaration
    /// does not hide the rest of the file. At least one token is consumed
    /// before stopping at such a keyword, so recovery always makes progress.
    fn synchronize(&mut self, depth: usize, stop_at_brace: bool) {
        let mut local = self.open.saturating_sub(depth);
        let start = self.pos;
        loop {
            let line_start = self.pos > start
                && matches!(self.tokens[self.pos - 1].kind, TokenKind::Semicolon(_));
            match self.peek() {
                TokenKind::Eof => break,
                TokenKind::Keyword(
                    Keyword::Func | Keyword::Async | Keyword::Type | Keyword::Import,
                ) if !stop_at_brace && line_start => break,
                TokenKind::Semicolon(_) if local == 0 => {
                    self.bump();
                    break;
                }
                TokenKind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace) => local += 1,
                TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => {
                    if local > 0 {
                        local -= 1;
                    } else if stop_at_brace && self.at(Punct::RBrace) {
                        break;
                    }
                }
                _ => {}
            }
            self.bump();
        }
        self.open = depth;
    }

    // ----- declarations -----

    fn file_ast(&mut self) -> File {
        let package = self.package_clause();
        let mut imports = Vec::new();
        let mut items = Vec::new();
        loop {
            let before = self.pos;
            let result = match self.peek() {
                TokenKind::Eof => break,
                TokenKind::Keyword(Keyword::Import) if items.is_empty() => {
                    self.import().map(|import| imports.push(import))
                }
                TokenKind::Keyword(Keyword::Import) => {
                    let span = self.current_span();
                    Err(self.error("imports must come before other declarations", span))
                }
                _ => self.item().map(|item| items.push(item)),
            };
            if result.is_err() || !self.declaration_end() {
                self.synchronize(0, false);
            }
            debug_assert!(self.pos > before || *self.peek() == TokenKind::Eof);
        }
        File {
            package,
            imports,
            items,
        }
    }

    fn declaration_end(&mut self) -> bool {
        match self.peek() {
            TokenKind::Semicolon(_) => {
                self.bump();
                true
            }
            TokenKind::Eof => true,
            _ => {
                self.unexpected("newline or `;` after declaration");
                false
            }
        }
    }

    fn package_clause(&mut self) -> Option<Name> {
        if !self.at_keyword(Keyword::Package) {
            let span = self.current_span();
            self.error("expected `package` clause at the start of the file", span);
            return None;
        }
        self.bump();
        let name = self.name("a package name").ok();
        if name.is_none() || !self.declaration_end() {
            self.synchronize(0, false);
        }
        name
    }

    fn import(&mut self) -> PResult<Import> {
        let start = self.bump().span;
        let span = self.current_span();
        match self.peek().clone() {
            TokenKind::String(path)
                if self.file.text().as_bytes()[span.start() as usize] == b'"' =>
            {
                self.bump();
                Ok(Import {
                    path,
                    path_span: span,
                    span: self.span_from(start),
                })
            }
            TokenKind::Punct(Punct::LParen) => Err(self.error(
                "grouped imports are not supported; write one `import` per line",
                span,
            )),
            _ => Err(self.unexpected("a double-quoted import path")),
        }
    }

    fn item(&mut self) -> PResult<Item> {
        match self.peek() {
            TokenKind::Keyword(Keyword::Func | Keyword::Async) => self.func().map(Item::Func),
            TokenKind::Keyword(Keyword::Type) => self.struct_decl().map(Item::Struct),
            TokenKind::Keyword(Keyword::Let | Keyword::Var | Keyword::Const) => {
                self.binding().map(Item::Binding)
            }
            TokenKind::Reserved(word) => {
                let message = format!(
                    "`{}` is reserved for possible future use; the feature is not available",
                    word.as_str()
                );
                let span = self.current_span();
                Err(self.error(message, span))
            }
            _ => Err(self.unexpected("a declaration")),
        }
    }

    fn func(&mut self) -> PResult<FuncDecl> {
        let start = self.current_span();
        let is_async = self.at_keyword(Keyword::Async);
        if is_async {
            self.bump();
            if !self.at_keyword(Keyword::Func) {
                return Err(self.unexpected("`func` after `async`"));
            }
        }
        self.bump();
        let receiver = if self.at(Punct::LParen) {
            self.bump();
            let receiver = self.param("a receiver name")?;
            if self.at(Punct::Comma) {
                let span = self.current_span();
                return Err(self.error("a method has exactly one receiver", span));
            }
            self.expect(Punct::RParen)?;
            Some(receiver)
        } else {
            None
        };
        let name = self.name("a function name")?;
        self.expect(Punct::LParen)?;
        let params = self.comma_list(Punct::RParen, "parameter", true, |p| {
            p.param("a parameter name")
        })?;
        let results = self.results()?;
        let body = self.body_block("function signature", "function declarations require a body")?;
        Ok(FuncDecl {
            is_async,
            receiver,
            name,
            params,
            results,
            body,
            span: self.span_from(start),
        })
    }

    fn param(&mut self, what: &str) -> PResult<Param> {
        let name = self.name(what)?;
        if self.at(Punct::Comma) || self.at(Punct::RParen) {
            let message = format!(
                "`{}` needs its own type; grouped `a, b T` parameters are not supported",
                name.text
            );
            return Err(self.error(message, name.span));
        }
        let mode = if self.at_keyword(Keyword::Mut) {
            self.bump();
            ParamMode::Mut
        } else if self.at_keyword(Keyword::Own) {
            self.bump();
            ParamMode::Own
        } else {
            ParamMode::Borrow
        };
        let ty = self.ty()?;
        if self.at(Punct::Eq) {
            let span = self.current_span();
            return Err(self.error("default parameter values are not supported", span));
        }
        Ok(Param {
            span: self.span_from(name.span),
            name,
            mode,
            ty,
        })
    }

    /// Result list: none, one bare type, or two or more parenthesized types.
    fn results(&mut self) -> PResult<Vec<Type>> {
        if self.at(Punct::LParen) {
            let open = self.bump().span;
            let results = self.comma_list(Punct::RParen, "result type", false, Self::ty)?;
            if results.len() < 2 {
                let span = self.span_from(open);
                let message = if results.is_empty() {
                    "an empty result list is written by omitting it"
                } else {
                    "a single result type is written without parentheses"
                };
                return Err(self.error(message, span));
            }
            return Ok(results);
        }
        if matches!(
            self.peek(),
            TokenKind::Punct(Punct::LBrace) | TokenKind::Semicolon(_)
        ) {
            return Ok(Vec::new());
        }
        Ok(vec![self.ty()?])
    }

    fn ty(&mut self) -> PResult<Type> {
        match self.peek() {
            TokenKind::Ident => {
                let name = self.name("a type")?;
                if self.at(Punct::Lt) {
                    return Err(
                        self.unsupported("generic type arguments such as `Array<T>`", "M20–M25")
                    );
                }
                if self.at(Punct::Dot) && *self.peek_at(1) == TokenKind::Ident {
                    return Err(self.unsupported("package-qualified type names", "M23"));
                }
                Ok(Type { name })
            }
            TokenKind::Punct(Punct::LBracket) => {
                Err(self.unsupported("array and slice types", "M20–M21"))
            }
            TokenKind::Keyword(Keyword::Map) => Err(self.unsupported("map types", "M22")),
            TokenKind::Keyword(Keyword::Channel) => Err(self.unsupported("channel types", "M30")),
            TokenKind::Keyword(Keyword::Func) => Err(self.unsupported("function types", "M24")),
            TokenKind::Keyword(Keyword::Mut) => Err(self.unsupported("`mut` slice types", "M21")),
            _ => Err(self.unexpected("a type")),
        }
    }

    fn struct_decl(&mut self) -> PResult<StructDecl> {
        let start = self.bump().span;
        let name = self.name("a type name")?;
        if !self.at_keyword(Keyword::Struct) {
            return Err(self.unexpected("`struct`; only struct type declarations are supported"));
        }
        self.bump();
        let open = self.body_open("struct type name")?;
        let mut fields = Vec::new();
        loop {
            if self.at(Punct::RBrace) {
                self.bump();
                break;
            }
            if *self.peek() == TokenKind::Eof {
                return Err(self.unclosed(open));
            }
            let depth = self.open;
            let field = self.field_decl();
            let ok = match field {
                Ok(field) => {
                    fields.push(field);
                    self.statement_end("field")
                }
                Err(Recover) => false,
            };
            if !ok {
                self.synchronize(depth, true);
            }
        }
        Ok(StructDecl {
            name,
            fields,
            span: self.span_from(start),
        })
    }

    fn field_decl(&mut self) -> PResult<FieldDecl> {
        let name = self.name("a field name")?;
        let ty = self.ty()?;
        Ok(FieldDecl {
            span: self.span_from(name.span),
            name,
            ty,
        })
    }

    fn binding(&mut self) -> PResult<Binding> {
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

    fn binding_target(&mut self, kind: BindingKind) -> PResult<BindingTarget> {
        if *self.peek() == TokenKind::Underscore && kind != BindingKind::Const {
            return Ok(BindingTarget::Discard(self.bump().span));
        }
        self.name("a binding name").map(BindingTarget::Name)
    }

    // ----- statements -----

    /// `{` of a body that must stay on the line of the preceding header (§3.7).
    fn body_open(&mut self, header: &str) -> PResult<Span> {
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

    fn body_block(&mut self, header: &str, missing: &str) -> PResult<Block> {
        if self.at_separator() && !self.at_newline_before(&TokenKind::Punct(Punct::LBrace))
            || *self.peek() == TokenKind::Eof
        {
            let span = self.current_span();
            return Err(self.error(missing, span));
        }
        let open = self.body_open(header)?;
        Ok(self.block_rest(open))
    }

    fn block(&mut self) -> PResult<Block> {
        let open = self.expect(Punct::LBrace)?;
        Ok(self.block_rest(open))
    }

    fn unclosed(&mut self, open: Span) -> Recover {
        let span = self.current_span();
        self.report(
            Diagnostic::new(Severity::Error, "unclosed `{`", span)
                .primary_message("file ends here")
                .related(open, "opened here"),
        )
    }

    /// Statements after an opening brace, through the matching `}`.
    fn block_rest(&mut self, open: Span) -> Block {
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
            let (depth, before) = (self.open, self.pos);
            let ok = match self.stmt() {
                Ok(stmt) => {
                    stmts.push(stmt);
                    self.statement_end("statement")
                }
                Err(Recover) => false,
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

    /// A statement ends at a separator or may omit it before `}` (§3.7).
    fn statement_end(&mut self, what: &str) -> bool {
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

    fn stmt(&mut self) -> PResult<Stmt> {
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
            _ => return self.simple().and_then(|simple| self.statement(simple)),
        };
        Ok(Stmt {
            kind,
            span: self.span_from(start),
        })
    }

    /// Accept a bare expression as a statement only when it is call-based.
    fn statement(&mut self, simple: Simple) -> PResult<Stmt> {
        match simple {
            Simple::Stmt(stmt) => Ok(stmt),
            Simple::Expr(expr) if is_call_based(&expr) => Ok(Stmt {
                span: expr.span,
                kind: StmtKind::Expr(expr),
            }),
            Simple::Expr(expr) => Err(self.report(
                Diagnostic::new(Severity::Error, "expression is not a statement", expr.span).note(
                    "only calls can be statements; discard a value explicitly with `_ = value`",
                ),
            )),
        }
    }

    /// Assignment or bare expression (§5.6, §7.8).
    fn simple(&mut self) -> PResult<Simple> {
        let start = self.current_span();
        let mut targets = vec![self.assign_target()?];
        while self.eat(Punct::Comma) {
            targets.push(self.assign_target()?);
        }
        let Some(op) = assign_op(self.peek()) else {
            return match targets.pop() {
                Some(AssignTarget::Place(expr)) if targets.is_empty() => Ok(Simple::Expr(expr)),
                _ => Err(self.unexpected("`=` after assignment targets")),
            };
        };
        let op_span = self.bump().span;
        for target in &targets {
            if let AssignTarget::Place(expr) = target
                && !matches!(expr.kind, ExprKind::Name(_) | ExprKind::Field { .. })
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
        Ok(Simple::Stmt(Stmt {
            kind: StmtKind::Assign {
                targets,
                op,
                values,
            },
            span: self.span_from(start),
        }))
    }

    fn assign_target(&mut self) -> PResult<AssignTarget> {
        if *self.peek() == TokenKind::Underscore {
            return Ok(AssignTarget::Discard(self.bump().span));
        }
        self.expr().map(AssignTarget::Place)
    }

    fn if_stmt(&mut self) -> PResult<If> {
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

    fn for_stmt(&mut self) -> PResult<For> {
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
            let update = self.with_struct_literals(false, |p| p.simple())?;
            let update = Box::new(self.statement(update)?);
            ForHeader::Counting {
                init,
                condition,
                update,
            }
        } else {
            match first {
                Simple::Expr(condition) => ForHeader::Condition(condition),
                Simple::Stmt(_) => return Err(self.unexpected("`;` after the loop initializer")),
            }
        };
        let body = self.body_block("`for` header", "expected `{` after `for` header")?;
        Ok(For { header, body })
    }

    /// The initializer of a counting loop or the condition of a conditional
    /// loop. Struct literals are allowed in an initializer, so an assignment
    /// parsed without them is re-parsed when it is followed by `{`.
    fn for_first_clause(&mut self) -> PResult<Simple> {
        if matches!(self.peek(), TokenKind::Keyword(Keyword::Let | Keyword::Var)) {
            let start = self.current_span();
            let binding = self.binding()?;
            return Ok(Simple::Stmt(Stmt {
                kind: StmtKind::Binding(binding),
                span: self.span_from(start),
            }));
        }
        let (pos, open, last_end, reported) =
            (self.pos, self.open, self.last_end, self.diagnostics.len());
        let first = self.with_struct_literals(false, |p| p.simple());
        if matches!(first, Ok(Simple::Stmt(_))) && self.at(Punct::LBrace) {
            (self.pos, self.open, self.last_end) = (pos, open, last_end);
            self.diagnostics.truncate(reported);
            return self.simple();
        }
        first
    }

    fn header_separator(&mut self) -> PResult<()> {
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

    fn header_expr(&mut self) -> PResult<Expr> {
        self.with_struct_literals(false, Self::expr)
    }

    fn with_struct_literals<T>(&mut self, allowed: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = std::mem::replace(&mut self.struct_literals, allowed);
        let result = f(self);
        self.struct_literals = saved;
        result
    }

    // ----- expressions -----

    fn expr_list(&mut self) -> PResult<Vec<Expr>> {
        let mut exprs = vec![self.expr()?];
        while self.eat(Punct::Comma) {
            exprs.push(self.expr()?);
        }
        Ok(exprs)
    }

    fn expr(&mut self) -> PResult<Expr> {
        self.binary(1)
    }

    /// Precedence climbing over §7.6 levels 4–8; comparisons do not chain.
    fn binary(&mut self, min: u8) -> PResult<Expr> {
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

    fn unary(&mut self) -> PResult<Expr> {
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

    /// `await operand?` groups as `(await operand)?` (§7.6).
    fn await_expr(&mut self) -> PResult<Expr> {
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

    fn postfix(&mut self) -> PResult<Expr> {
        let expr = self.access()?;
        self.propagations(expr)
    }

    fn propagations(&mut self, mut expr: Expr) -> PResult<Expr> {
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

    /// Primary expression followed by calls and field access (§7.6 level 1).
    fn access(&mut self) -> PResult<Expr> {
        let mut expr = self.primary()?;
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
                return Err(self.unsupported("indexing and slicing expressions", "M20–M21"));
            } else {
                return Ok(expr);
            }
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let span = self.current_span();
        let kind = match self.peek().clone() {
            TokenKind::Ident => {
                let name = self.name("an expression")?;
                if self.struct_literals && self.at(Punct::LBrace) {
                    return self.struct_literal(name);
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
                return Err(self.unsupported("function literals (closures)", "M24"));
            }
            TokenKind::Keyword(Keyword::Go) => {
                return Err(self.unsupported("`go` task-creation expressions", "M25–M29"));
            }
            TokenKind::Keyword(Keyword::Map) | TokenKind::Punct(Punct::LBracket) => {
                return Err(self.unsupported("array and map literals", "M20–M22"));
            }
            TokenKind::Keyword(Keyword::Channel) => {
                return Err(self.unsupported("channel expressions", "M30"));
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

    /// `Name{field: value, ...}` with every field named (§8.2, §8.4).
    fn struct_literal(&mut self, ty: Name) -> PResult<Expr> {
        self.bump();
        let fields = self.with_struct_literals(true, |p| {
            p.comma_list(Punct::RBrace, "field", true, Self::field_init)
        })?;
        Ok(Expr {
            span: self.span_from(ty.span),
            kind: ExprKind::StructLit { ty, fields },
        })
    }

    fn field_init(&mut self) -> PResult<FieldInit> {
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

    /// Comma-separated items through `close`. A trailing comma is optional
    /// where `trailing` is set and required before a newline that precedes
    /// `close`, because the newline would otherwise insert a semicolon (§3.7).
    fn comma_list<T>(
        &mut self,
        close: Punct,
        what: &str,
        trailing: bool,
        mut item: impl FnMut(&mut Self) -> PResult<T>,
    ) -> PResult<Vec<T>> {
        let mut items = Vec::new();
        loop {
            if self.eat(close) {
                return Ok(items);
            }
            items.push(item(self)?);
            if self.at(Punct::Comma) {
                let comma = self.bump().span;
                if !trailing && self.at(close) {
                    let message = format!("a trailing comma is not allowed after the last {what}");
                    return Err(self.error(message, comma));
                }
            } else if self.at_newline_before(&TokenKind::Punct(close)) {
                let span = self.current_span();
                return Err(self.report(
                    Diagnostic::new(
                        Severity::Error,
                        format!("missing trailing comma after the last {what}"),
                        span,
                    )
                    .note(format!(
                        "add `,` when the closing `{}` is on the next line",
                        close.as_str()
                    )),
                ));
            } else if !self.at(close) {
                return Err(self.unexpected(&format!("`,` or `{}`", close.as_str())));
            }
        }
    }
}

fn is_call_based(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Call { .. } => true,
        ExprKind::Await(inner) | ExprKind::Try(inner) => is_call_based(inner),
        _ => false,
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
