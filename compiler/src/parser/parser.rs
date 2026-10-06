use crate::ast::*;
use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::{Keyword, Punct, Separator, Token, TokenKind};
use crate::source::{SourceFile, Span};

pub(super) struct Reported;

pub(super) type PResult<T> = Result<T, Reported>;

pub(super) enum StatementOrExpr {
    Stmt(Stmt),
    Expr(Expr),
}

pub(super) struct Parser<'a> {
    pub(super) file: &'a SourceFile,
    pub(super) tokens: Vec<Token>,
    pub(super) pos: usize,
    pub(super) last_token_end: u32,
    pub(super) open_delimiters: usize,
    // Off in `if`/`for` headers, where `Name {` starts the body.
    pub(super) struct_literals_allowed: bool,
    pub(super) diagnostics: Vec<Diagnostic>,
}

impl Parser<'_> {
    pub(super) fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    pub(super) fn peek_at(&self, ahead: usize) -> &TokenKind {
        let index = (self.pos + ahead).min(self.tokens.len() - 1);
        &self.tokens[index].kind
    }

    pub(super) fn current_text(&self) -> &str {
        let span = self.current_span();
        &self.file.text()[span.start() as usize..span.end() as usize]
    }

    pub(super) fn current_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    pub(super) fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        match token.kind {
            TokenKind::Eof => return token,
            TokenKind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace) => {
                self.open_delimiters += 1
            }
            TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => {
                self.open_delimiters = self.open_delimiters.saturating_sub(1);
            }
            _ => {}
        }
        if !token.span.is_empty() {
            self.last_token_end = token.span.end();
        }
        self.pos += 1;
        token
    }

    pub(super) fn at(&self, punct: Punct) -> bool {
        *self.peek() == TokenKind::Punct(punct)
    }

    pub(super) fn at_keyword(&self, keyword: Keyword) -> bool {
        *self.peek() == TokenKind::Keyword(keyword)
    }

    pub(super) fn at_separator(&self) -> bool {
        matches!(self.peek(), TokenKind::Semicolon(_))
    }

    pub(super) fn at_newline_before(&self, next: &TokenKind) -> bool {
        *self.peek() == TokenKind::Semicolon(Separator::Newline) && self.peek_at(1) == next
    }

    pub(super) fn eat(&mut self, punct: Punct) -> bool {
        let found = self.at(punct);
        if found {
            self.bump();
        }
        found
    }

    pub(super) fn span_from(&self, start: Span) -> Span {
        let end = self.last_token_end.max(start.end());
        self.file
            .span(start.start(), end)
            .expect("parser spans cover consumed tokens")
    }

    pub(super) fn report(&mut self, diagnostic: Diagnostic) -> Reported {
        self.diagnostics.push(diagnostic);
        Reported
    }

    pub(super) fn error(&mut self, message: impl Into<String>, span: Span) -> Reported {
        self.report(Diagnostic::new(Severity::Error, message, span))
    }

    pub(super) fn unexpected(&mut self, expected: &str) -> Reported {
        // The lexer has already reported these tokens.
        if matches!(
            self.peek(),
            TokenKind::Unknown | TokenKind::MalformedLiteral
        ) {
            return Reported;
        }
        let found = self.describe_current();
        let span = self.current_span();
        self.error(format!("expected {expected}, found {found}"), span)
    }

    pub(super) fn describe_current(&self) -> String {
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

    pub(super) fn unsupported(&mut self, what: &str, milestone: &str) -> Reported {
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

    pub(super) fn expect(&mut self, punct: Punct) -> PResult<Span> {
        if self.at(punct) {
            return Ok(self.bump().span);
        }
        Err(self.unexpected(&format!("`{}`", punct.as_str())))
    }

    pub(super) fn name(&mut self, what: &str) -> PResult<Name> {
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

    pub(super) fn synchronize(&mut self, depth: usize, stop_at_brace: bool) {
        let mut local = self.open_delimiters.saturating_sub(depth);
        let start = self.pos;
        loop {
            let line_start = self.pos > start
                && matches!(self.tokens[self.pos - 1].kind, TokenKind::Semicolon(_));
            match self.peek() {
                TokenKind::Eof => break,
                // An unclosed delimiter must not hide later declarations.
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
        self.open_delimiters = depth;
    }

    pub(super) fn with_struct_literals<T>(
        &mut self,
        allowed: bool,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved = std::mem::replace(&mut self.struct_literals_allowed, allowed);
        let result = f(self);
        self.struct_literals_allowed = saved;
        result
    }

    pub(super) fn comma_list<T>(
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
                // The newline ended the list, so the trailing comma is required.
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
