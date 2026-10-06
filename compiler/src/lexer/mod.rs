#[allow(clippy::module_inception)]
mod lexer;
mod token;
mod token_kind;

pub use lexer::{Lexed, lex};
pub use token::Token;
pub use token_kind::{IntBase, Keyword, Punct, ReservedWord, Separator, TokenKind};
