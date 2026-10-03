//! Tokens produced by the lexer.

use super::token_kind::TokenKind;
use crate::source::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
