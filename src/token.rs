//! Token kinds produced by the lexer (spec §3.5–3.17, §5.6, §7.6).

use crate::source::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident,
    /// Standalone `_`: identifier-shaped but reserved for discards (§3.5, §5.5).
    Underscore,
    Keyword(Keyword),
    /// Future-reserved word (§3.16); never enables its associated feature.
    Reserved(ReservedWord),
    /// Integer spelling; the value is decoded by later stages from the span
    /// because literal typing and range rules belong to the type model.
    Int(IntBase),
    /// Decimal float spelling; decoding is deferred for the same reason.
    Float,
    /// Decoded value of a double-quoted or raw string literal.
    String(String),
    /// Decoded scalar of a rune literal.
    Rune(char),
    /// A literal whose spelling was diagnosed. It still ends statements so that
    /// recovery keeps the surrounding structure.
    MalformedLiteral,
    /// A character or sequence with no token meaning; always diagnosed. Like
    /// `MalformedLiteral`, it ends a statement to aid recovery.
    Unknown,
    Punct(Punct),
    Semicolon(Separator),
    Eof,
}

impl TokenKind {
    /// Whether a following newline or EOF inserts a semicolon (§3.7).
    pub fn ends_statement(&self) -> bool {
        match self {
            Self::Ident
            | Self::Underscore
            | Self::Int(_)
            | Self::Float
            | Self::String(_)
            | Self::Rune(_)
            | Self::MalformedLiteral
            // Already diagnosed; ending the statement keeps recovery line-based.
            | Self::Unknown => true,
            Self::Keyword(keyword) => keyword.ends_statement(),
            Self::Punct(punct) => matches!(
                punct,
                Punct::RParen | Punct::RBracket | Punct::RBrace | Punct::Question
            ),
            Self::Reserved(_) | Self::Semicolon(_) | Self::Eof => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntBase {
    Binary,
    Octal,
    Decimal,
    Hexadecimal,
}

impl IntBase {
    pub fn radix(self) -> u32 {
        match self {
            Self::Binary => 2,
            Self::Octal => 8,
            Self::Decimal => 10,
            Self::Hexadecimal => 16,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Separator {
    /// A `;` written in the source.
    Explicit,
    /// Inserted at a newline, including the first newline of a block comment.
    Newline,
    /// Inserted at end of input.
    Eof,
}

macro_rules! words {
    ($name:ident { $($variant:ident = $text:literal,)* }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name {
            $($variant,)*
        }

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)*
                }
            }

            /// Exact, case-sensitive whole-word lookup.
            pub fn lookup(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

words!(Keyword {
    Package = "package",
    Import = "import",
    Func = "func",
    Type = "type",
    Struct = "struct",
    Let = "let",
    Var = "var",
    Const = "const",
    Mut = "mut",
    Own = "own",
    If = "if",
    Else = "else",
    For = "for",
    Break = "break",
    Continue = "continue",
    Return = "return",
    Async = "async",
    Await = "await",
    Go = "go",
    Map = "map",
    Channel = "channel",
    True = "true",
    False = "false",
    Nil = "nil",
});

impl Keyword {
    fn ends_statement(self) -> bool {
        matches!(
            self,
            Self::Break | Self::Continue | Self::Return | Self::True | Self::False | Self::Nil
        )
    }
}

words!(ReservedWord {
    Interface = "interface",
    Trait = "trait",
    Impl = "impl",
    Enum = "enum",
    Match = "match",
    Unsafe = "unsafe",
    Macro = "macro",
    Defer = "defer",
});

// `ALL` is ordered longest spelling first so the lexer can take the maximal
// munch by trying each spelling in turn. `++`, `--`, `:=`, and `&^` are
// deliberately absent (§7.6, §5.4).
words!(Punct {
    ShlEq = "<<=",
    ShrEq = ">>=",
    Shl = "<<",
    Shr = ">>",
    LtEq = "<=",
    GtEq = ">=",
    EqEq = "==",
    NotEq = "!=",
    AndAnd = "&&",
    OrOr = "||",
    PlusEq = "+=",
    MinusEq = "-=",
    StarEq = "*=",
    SlashEq = "/=",
    PercentEq = "%=",
    AmpEq = "&=",
    PipeEq = "|=",
    CaretEq = "^=",
    Plus = "+",
    Minus = "-",
    Star = "*",
    Slash = "/",
    Percent = "%",
    Amp = "&",
    Pipe = "|",
    Caret = "^",
    Not = "!",
    Lt = "<",
    Gt = ">",
    Eq = "=",
    Question = "?",
    Dot = ".",
    Comma = ",",
    Colon = ":",
    LParen = "(",
    RParen = ")",
    LBracket = "[",
    RBracket = "]",
    LBrace = "{",
    RBrace = "}",
});
