#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident,
    Underscore,
    Keyword(Keyword),
    Reserved(ReservedWord),
    /// Decoded from the span during typing.
    Int(IntBase),
    /// Decoded from the span during typing.
    Float,
    String(String),
    Rune(char),
    /// A literal whose spelling was diagnosed.
    MalformedLiteral,
    /// An unrecognized character or sequence; always diagnosed.
    Unknown,
    Punct(Punct),
    Semicolon(Separator),
    Eof,
}

impl TokenKind {
    /// Whether a following newline or end of file inserts a semicolon.
    pub fn ends_statement(&self) -> bool {
        match self {
            Self::Ident
            | Self::Underscore
            | Self::Int(_)
            | Self::Float
            | Self::String(_)
            | Self::Rune(_)
            | Self::MalformedLiteral
            // Diagnosed tokens end statements so recovery stays line-based.
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
    Explicit,
    Newline,
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
    In = "in",
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

// Longest spellings first: the first matching prefix is the maximal munch.
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
