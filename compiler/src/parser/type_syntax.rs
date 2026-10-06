use super::parser::{PResult, Parser};
use crate::ast::*;
use crate::lexer::{Keyword, Punct, Separator, Token, TokenKind};

impl Parser<'_> {
    pub(super) fn ty(&mut self) -> PResult<Type> {
        match self.peek() {
            TokenKind::Ident => {
                let name = self.name("a type")?;
                if self.at(Punct::Lt) {
                    return self.type_arguments(name);
                }
                if self.at(Punct::Dot) && *self.peek_at(1) == TokenKind::Ident {
                    return Err(self.unsupported("package-qualified type names", "M23"));
                }
                Ok(Type::Named(name))
            }
            TokenKind::Punct(Punct::LBracket) => self.bracket_type(),
            TokenKind::Keyword(Keyword::Map) => self.map_type(),
            TokenKind::Keyword(Keyword::Channel) => Err(self.unsupported("channel types", "M30")),
            TokenKind::Keyword(Keyword::Func) => self.func_type(),
            TokenKind::Keyword(Keyword::Mut) => {
                let start = self.current_span();
                if !self.at_slice_type_after(1) {
                    return Err(self.error(
                        "`mut` in a type only forms a mutable slice type `mut []T` (§12.2)",
                        start,
                    ));
                }
                self.bump();
                let Type::Slice { element, .. } = self.bracket_type()? else {
                    unreachable!("checked: `[]` follows")
                };
                Ok(Type::Slice {
                    element,
                    mutable: true,
                    span: self.span_from(start),
                })
            }
            _ => Err(self.unexpected("a type")),
        }
    }

    /// Only the predeclared `Array` takes a type argument, and it cannot be shadowed.
    fn type_arguments(&mut self, name: Name) -> PResult<Type> {
        match name.text.as_str() {
            "Array" => {
                self.bump();
                let element = self.ty()?;
                self.close_type_arguments()?;
                Ok(Type::DynArray {
                    element: Box::new(element),
                    span: self.span_from(name.span),
                })
            }
            "Task" => Err(self.unsupported("`Task<...>` types", "M25–M29")),
            _ => Err(self.error(
                format!(
                    "`{}` does not take type arguments; user-defined generics are not part of the MVP (§22.1)",
                    name.text
                ),
                name.span,
            )),
        }
    }

    /// Splits `>>`, `>=`, or `>>=` so `Array<Array<int>>` closes both lists.
    pub(super) fn close_type_arguments(&mut self) -> PResult<()> {
        let rest = match self.peek() {
            TokenKind::Punct(Punct::Gt) => {
                self.bump();
                self.insert_semicolon_after_type_arguments();
                return Ok(());
            }
            TokenKind::Punct(Punct::Shr) => Punct::Gt,
            TokenKind::Punct(Punct::GtEq) => Punct::Eq,
            TokenKind::Punct(Punct::ShrEq) => Punct::GtEq,
            _ => return Err(self.unexpected("`>`")),
        };
        let span = self.current_span();
        let split = self
            .file
            .span(span.start() + 1, span.end())
            .expect("a `>` token is one byte");
        self.tokens[self.pos] = Token {
            kind: TokenKind::Punct(rest),
            span: split,
        };
        self.last_token_end = span.start() + 1;
        Ok(())
    }

    /// Only the parser can tell a closing `>` from a comparison, so it inserts the semicolon the lexer could not.
    fn insert_semicolon_after_type_arguments(&mut self) {
        if matches!(self.peek(), TokenKind::Semicolon(_)) {
            return;
        }
        let gap_start = self.last_token_end as usize;
        let gap_end = match self.peek() {
            TokenKind::Eof => self.file.text().len(),
            _ => self.current_span().start() as usize,
        };
        let gap = &self.file.text()[gap_start..gap_end];
        let at = match gap.find('\n') {
            Some(lf) if lf > 0 && gap.as_bytes()[lf - 1] == b'\r' => gap_start + lf - 1,
            Some(lf) => gap_start + lf,
            None if *self.peek() == TokenKind::Eof => gap_end,
            None => return,
        };
        let at = at as u32;
        let span = self.file.span(at, at).expect("a position inside the file");
        self.tokens.insert(
            self.pos,
            Token {
                kind: TokenKind::Semicolon(Separator::Newline),
                span,
            },
        );
    }

    fn func_type(&mut self) -> PResult<Type> {
        let start = self.bump().span;
        self.expect(Punct::LParen)?;
        let params =
            self.comma_list(Punct::RParen, "parameter type", true, Self::func_type_param)?;
        let results = if self.at_type_start() {
            self.results()?
        } else {
            Vec::new()
        };
        Ok(Type::Func {
            params,
            results,
            span: self.span_from(start),
        })
    }

    fn func_type_param(&mut self) -> PResult<FuncTypeParam> {
        if *self.peek() == TokenKind::Ident
            && matches!(
                self.peek_at(1),
                TokenKind::Ident
                    | TokenKind::Punct(Punct::LBracket)
                    | TokenKind::Keyword(
                        Keyword::Mut | Keyword::Own | Keyword::Func | Keyword::Map
                    )
            )
        {
            let span = self.current_span();
            return Err(self.error(
                "function type parameters have no names; write only the type, as in `func(int)` (§16.2)",
                span,
            ));
        }
        let mode = if self.at_keyword(Keyword::Mut) && !self.at_slice_type_after(1) {
            self.bump();
            ParamMode::Mut
        } else if self.at_keyword(Keyword::Own) {
            self.bump();
            ParamMode::Own
        } else {
            ParamMode::Borrow
        };
        let ty = self.ty()?;
        Ok(FuncTypeParam { mode, ty })
    }

    fn at_type_start(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Ident
                | TokenKind::Punct(Punct::LBracket | Punct::LParen)
                | TokenKind::Keyword(
                    Keyword::Map | Keyword::Func | Keyword::Channel | Keyword::Mut
                )
        )
    }

    pub(super) fn map_type(&mut self) -> PResult<Type> {
        let start = self.bump().span;
        self.expect(Punct::LBracket)?;
        let key = self.ty()?;
        self.expect(Punct::RBracket)?;
        let value = self.ty()?;
        Ok(Type::Map {
            key: Box::new(key),
            value: Box::new(value),
            span: self.span_from(start),
        })
    }

    pub(super) fn at_slice_type_after(&self, ahead: usize) -> bool {
        *self.peek_at(ahead) == TokenKind::Punct(Punct::LBracket)
            && *self.peek_at(ahead + 1) == TokenKind::Punct(Punct::RBracket)
    }

    pub(super) fn bracket_type(&mut self) -> PResult<Type> {
        let start = self.current_span();
        self.bump();
        if self.eat(Punct::RBracket) {
            let element = self.ty()?;
            return Ok(Type::Slice {
                element: Box::new(element),
                mutable: false,
                span: self.span_from(start),
            });
        }
        let element = self.ty()?;
        if !matches!(self.peek(), TokenKind::Semicolon(Separator::Explicit)) {
            return Err(self.unexpected("`;`"));
        }
        self.bump();
        let size = self.with_struct_literals(true, Self::expr)?;
        self.expect(Punct::RBracket)?;
        Ok(Type::Array {
            element: Box::new(element),
            size: Box::new(size),
            span: self.span_from(start),
        })
    }
}
