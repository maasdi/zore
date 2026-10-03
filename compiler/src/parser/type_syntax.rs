//! Type syntax: named, fixed-array, slice, and dynamic-array types.

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
            TokenKind::Keyword(Keyword::Map) => Err(self.unsupported("map types", "M22")),
            TokenKind::Keyword(Keyword::Channel) => Err(self.unsupported("channel types", "M30")),
            TokenKind::Keyword(Keyword::Func) => Err(self.unsupported("function types", "M24")),
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

    /// `Name<...>` after `Name`: only the predeclared `Array` takes a type
    /// argument here; `Array` cannot be shadowed (§3.18).
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

    /// Consumes the `>` closing a type argument list, splitting it off a
    /// `>>`, `>=`, or `>>=` token so `Array<Array<int>>` closes both lists.
    pub(super) fn close_type_arguments(&mut self) -> PResult<()> {
        let rest = match self.peek() {
            TokenKind::Punct(Punct::Gt) => {
                self.bump();
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

    /// Whether the tokens `ahead` positions from here begin `[]`.
    pub(super) fn at_slice_type_after(&self, ahead: usize) -> bool {
        *self.peek_at(ahead) == TokenKind::Punct(Punct::LBracket)
            && *self.peek_at(ahead + 1) == TokenKind::Punct(Punct::RBracket)
    }

    /// `[element; size]` or the shared slice type `[]element`.
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
