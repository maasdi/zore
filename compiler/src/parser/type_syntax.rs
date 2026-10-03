//! Type syntax: named, fixed-array, and slice types.

use super::parser::{PResult, Parser};
use crate::ast::*;
use crate::lexer::{Keyword, Punct, Separator, TokenKind};

impl Parser<'_> {
    pub(super) fn ty(&mut self) -> PResult<Type> {
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
