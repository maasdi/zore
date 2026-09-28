//! Source text to tokens, with automatic semicolon insertion.

pub mod token;

use crate::diagnostic::{Diagnostic, Severity};
use crate::source::{SourceFile, Span};
use crate::lexer::token::{IntBase, Keyword, Punct, ReservedWord, Separator, Token, TokenKind};

#[derive(Debug)]
pub struct Lexed {
    /// Ends with exactly one `Eof` token.
    pub tokens: Vec<Token>,
    /// Empty exactly when the file is lexically valid.
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(file: &SourceFile) -> Lexed {
    let mut lexer = Lexer {
        file,
        text: file.text(),
        pos: 0,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
        last_token_ends_statement: false,
    };
    lexer.run();
    Lexed {
        tokens: lexer.tokens,
        diagnostics: lexer.diagnostics,
    }
}

struct Lexer<'a> {
    file: &'a SourceFile,
    text: &'a str,
    pos: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
    last_token_ends_statement: bool,
}

impl Lexer<'_> {
    fn run(&mut self) {
        while let Some(ch) = self.peek() {
            let start = self.pos;
            match ch {
                ' ' | '\t' | '\r' => self.pos += 1,
                '\n' => {
                    self.insert_at_line_end(start);
                    self.pos += 1;
                }
                '/' if self.peek_at(1) == Some('/') => self.line_comment(),
                '/' if self.peek_at(1) == Some('*') => self.block_comment(),
                '"' => self.string_or_rune('"'),
                '\'' => self.string_or_rune('\''),
                '`' => self.raw_string(),
                ';' => {
                    self.pos += 1;
                    self.push(TokenKind::Semicolon(Separator::Explicit), start);
                }
                '0'..='9' => self.number(),
                c if is_ident_char(c) => self.identifier(),
                _ => self.punct(),
            }
        }
        let end = self.text.len();
        if self.last_token_ends_statement {
            self.push_at(TokenKind::Semicolon(Separator::Eof), end, end);
        }
        self.push_at(TokenKind::Eof, end, end);
    }

    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn peek_at(&self, ahead: usize) -> Option<char> {
        self.text[self.pos..].chars().nth(ahead)
    }

    fn span(&self, start: usize, end: usize) -> Span {
        self.file
            .span(start as u32, end as u32)
            .expect("lexer offsets are UTF-8 boundaries within the file")
    }

    fn error(&self, message: impl Into<String>, start: usize, end: usize) -> Diagnostic {
        Diagnostic::new(Severity::Error, message, self.span(start, end))
    }

    fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.push_at(kind, start, self.pos);
    }

    fn push_at(&mut self, kind: TokenKind, start: usize, end: usize) {
        self.last_token_ends_statement = kind.ends_statement();
        let span = self.span(start, end);
        self.tokens.push(Token { kind, span });
    }

    fn insert_at_line_end(&mut self, lf: usize) {
        if self.last_token_ends_statement {
            let at = if lf > 0 && self.text.as_bytes()[lf - 1] == b'\r' {
                lf - 1
            } else {
                lf
            };
            self.push_at(TokenKind::Semicolon(Separator::Newline), at, at);
        }
    }

    fn line_comment(&mut self) {
        // Keep the newline visible so it can trigger semicolon insertion.
        self.pos = self.text[self.pos..]
            .find('\n')
            .map_or(self.text.len(), |offset| self.pos + offset);
    }

    fn block_comment(&mut self) {
        let start = self.pos;
        let body = start + 2;
        let (end, closed) = match self.text[body..].find("*/") {
            Some(offset) => (body + offset + 2, true),
            None => (self.text.len(), false),
        };
        // A multiline block comment acts as one newline.
        if let Some(offset) = self.text[body..end].find('\n') {
            self.insert_at_line_end(body + offset);
        }
        if !closed {
            self.report(
                self.error("unterminated block comment", start, body)
                    .primary_message("comment starts here")
                    .note("block comments end at the first `*/` and do not nest"),
            );
        }
        self.pos = end;
    }

    fn identifier(&mut self) {
        let start = self.pos;
        let mut non_ascii = None;
        while let Some(ch) = self
            .peek()
            .filter(|&c| is_ident_char(c) || c.is_ascii_digit())
        {
            if !ch.is_ascii() && non_ascii.is_none() {
                non_ascii = Some((self.pos, ch));
            }
            self.pos += ch.len_utf8();
        }
        let text = &self.text[start..self.pos];
        if let Some((at, ch)) = non_ascii {
            // One token keeps recovery from splitting the name.
            self.report(
                self.error(
                    format!("identifier `{text}` contains non-ASCII character {ch:?}"),
                    at,
                    at + ch.len_utf8(),
                )
                .note("identifiers may contain only ASCII letters, digits, and `_`"),
            );
            self.push(TokenKind::Ident, start);
            return;
        }
        let kind = if text == "_" {
            TokenKind::Underscore
        } else if let Some(keyword) = Keyword::lookup(text) {
            TokenKind::Keyword(keyword)
        } else if let Some(word) = ReservedWord::lookup(text) {
            TokenKind::Reserved(word)
        } else {
            TokenKind::Ident
        };
        self.push(kind, start);
    }

    fn number(&mut self) {
        let start = self.pos;
        let bytes = self.text.as_bytes();
        let prefixed = bytes[start] == b'0'
            && matches!(
                bytes.get(start + 1),
                Some(b'b' | b'B' | b'o' | b'O' | b'x' | b'X')
            );
        let mut seen_point = false;
        let mut seen_exponent = false;
        // Scan the whole run so suffixes are diagnosed rather than split off.
        while let Some(ch) = self.peek() {
            if ch == '_' || ch.is_alphanumeric() {
                self.pos += ch.len_utf8();
                if !prefixed && matches!(ch, 'e' | 'E') {
                    seen_exponent = true;
                    let sign = matches!(self.peek(), Some('+' | '-'));
                    let after = self.peek_at(1);
                    if sign && after.is_some_and(|c| c == '_' || c.is_alphanumeric()) {
                        self.pos += 1;
                    }
                }
            } else if ch == '.'
                && !prefixed
                && !seen_point
                && !seen_exponent
                && self.peek_at(1).is_some_and(|c| c.is_ascii_digit())
            {
                seen_point = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        match validate_number(&self.text[start..self.pos]) {
            Ok(kind) => self.push(kind, start),
            Err(error) => {
                self.report(
                    self.error(error.message, start + error.start, start + error.end)
                        .note(error.note),
                );
                self.push(TokenKind::MalformedLiteral, start);
            }
        }
    }

    fn string_or_rune(&mut self, quote: char) {
        let start = self.pos;
        let rune = quote == '\'';
        let what = if rune { "rune" } else { "string" };
        self.pos += 1;
        let mut value = String::new();
        let mut valid = true;
        let mut closed = false;
        let mut continuation = false;
        while let Some(ch) = self.peek() {
            if ch == quote {
                self.pos += 1;
                closed = true;
                break;
            }
            if ch == '\n' || (ch == '\r' && self.peek_at(1) == Some('\n')) {
                break;
            }
            if ch != '\\' {
                value.push(ch);
                self.pos += ch.len_utf8();
                continue;
            }
            let backslash = self.pos;
            self.pos += 1;
            match self.peek() {
                None => break,
                Some('\n') => {
                    continuation = true;
                    break;
                }
                Some('\r') if self.peek_at(1) == Some('\n') => {
                    continuation = true;
                    break;
                }
                Some(c) => match self.escape_sequence(c, rune, backslash) {
                    Some(decoded) => value.push(decoded),
                    None => valid = false,
                },
            }
        }
        if !closed {
            let opening = if rune { "'" } else { "\"" };
            let note = if continuation {
                "a backslash does not continue a literal onto the next line"
            } else if rune {
                "rune literals must close on the same line"
            } else {
                "use `\\n` or a backtick raw string for multiline text"
            };
            self.report(
                self.error(format!("unterminated {what} literal"), start, start + 1)
                    .primary_message(format!("`{opening}` opens a {what} literal here"))
                    .note(note),
            );
            self.push(TokenKind::MalformedLiteral, start);
            return;
        }
        if !valid {
            self.push(TokenKind::MalformedLiteral, start);
            return;
        }
        if !rune {
            self.push(TokenKind::String(value), start);
            return;
        }
        let mut scalars = value.chars();
        match (scalars.next(), scalars.next()) {
            (Some(scalar), None) => self.push(TokenKind::Rune(scalar), start),
            (first, _) => {
                let message = if first.is_none() {
                    "empty rune literal"
                } else {
                    "rune literal contains more than one character"
                };
                self.report(
                    self.error(message, start, self.pos)
                        .note("a rune literal holds exactly one Unicode scalar value"),
                );
                self.push(TokenKind::MalformedLiteral, start);
            }
        }
    }

    fn escape_sequence(&mut self, c: char, rune: bool, backslash: usize) -> Option<char> {
        self.pos += c.len_utf8();
        let simple = match c {
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            '\\' => Some('\\'),
            '"' => Some('"'),
            '\'' if rune => Some('\''),
            _ => None,
        };
        if simple.is_some() {
            return simple;
        }
        let digits = match c {
            'u' => 4,
            'U' => 8,
            _ => {
                let note = if c == '\'' {
                    "write a single quote directly inside a string"
                } else if rune {
                    "supported escapes are \\n \\r \\t \\\\ \\\" \\' \\uXXXX \\UXXXXXXXX"
                } else {
                    "supported escapes are \\n \\r \\t \\\\ \\\" \\uXXXX \\UXXXXXXXX"
                };
                self.report(
                    self.error(
                        format!("unknown escape sequence `\\{c}`"),
                        backslash,
                        self.pos,
                    )
                    .note(note),
                );
                return None;
            }
        };
        let mut value = 0u32;
        let mut count = 0;
        while count < digits {
            let Some(digit) = self.peek().and_then(|ch| ch.to_digit(16)) else {
                break;
            };
            value = value * 16 + digit;
            count += 1;
            self.pos += 1;
        }
        let end = self.pos;
        if count < digits {
            self.report(
                self.error(
                    format!("`\\{c}` escape requires exactly {digits} hexadecimal digits"),
                    backslash,
                    end,
                )
                .note("braces and digit separators are not allowed in Unicode escapes"),
            );
            return None;
        }
        let scalar = char::from_u32(value);
        if scalar.is_none() {
            self.report(
                self.error(
                    format!(
                        "`{}` is not a Unicode scalar value",
                        &self.text[backslash..end]
                    ),
                    backslash,
                    end,
                )
                .note("scalar values are U+0000–U+10FFFF, excluding surrogates U+D800–U+DFFF"),
            );
        }
        scalar
    }

    fn raw_string(&mut self) {
        let start = self.pos;
        let body = start + 1;
        match self.text[body..].find('`') {
            Some(offset) => {
                let value = self.text[body..body + offset].to_owned();
                self.pos = body + offset + 1;
                self.push(TokenKind::String(value), start);
            }
            None => {
                self.pos = self.text.len();
                self.report(
                    self.error("unterminated raw string literal", start, body)
                        .primary_message("`` ` `` opens a raw string literal here"),
                );
                self.push(TokenKind::MalformedLiteral, start);
            }
        }
    }

    fn punct(&mut self) {
        let start = self.pos;
        let rest = &self.text[start..];
        for unsupported in ["++", "--"] {
            if rest.starts_with(unsupported) {
                self.pos += 2;
                self.report(
                    self.error(
                        format!("`{unsupported}` is not a Zore operator"),
                        start,
                        self.pos,
                    )
                    .note(
                        "increment and decrement are not supported; \
                             separate adjacent signs with a space or parentheses",
                    ),
                );
                self.push(TokenKind::Unknown, start);
                return;
            }
        }
        if let Some(&punct) = Punct::ALL.iter().find(|p| rest.starts_with(p.as_str())) {
            self.pos += punct.as_str().len();
            self.push(TokenKind::Punct(punct), start);
            return;
        }
        let ch = self.peek().expect("punct is called before EOF");
        self.pos += ch.len_utf8();
        self.report(self.error(format!("unexpected character {ch:?}"), start, self.pos));
        self.push(TokenKind::Unknown, start);
    }
}

// Non-ASCII letters are accepted only to diagnose them within one identifier.
fn is_ident_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic() || (!ch.is_ascii() && ch.is_alphanumeric())
}

struct NumberError {
    start: usize,
    end: usize,
    message: String,
    note: &'static str,
}

impl NumberError {
    fn new(start: usize, end: usize, message: String, note: &'static str) -> Self {
        Self {
            start,
            end,
            message,
            note,
        }
    }

    fn invalid_digit(text: &str, at: usize, base: IntBase) -> Self {
        Self::new(
            at,
            next_char(text, at),
            format!(
                "invalid digit {:?} in {} literal",
                char_at(text, at),
                base_name(base)
            ),
            "digits must be valid for the literal's base",
        )
    }

    fn suffix(text: &str, at: usize) -> Self {
        Self::new(
            at,
            text.len(),
            format!("invalid suffix `{}` on numeric literal", &text[at..]),
            "numeric literals do not accept suffixes",
        )
    }
}

const SEPARATOR_NOTE: &str = "a single `_` may appear only between two digits";

fn validate_number(text: &str) -> Result<TokenKind, NumberError> {
    let bytes = text.as_bytes();
    let base = match bytes.get(..2) {
        Some(b"0b" | b"0B") => IntBase::Binary,
        Some(b"0o" | b"0O") => IntBase::Octal,
        Some(b"0x" | b"0X") => IntBase::Hexadecimal,
        _ => IntBase::Decimal,
    };
    let mut kind = TokenKind::Int(base);
    let mut i = 0;
    if base != IntBase::Decimal {
        i = 2;
        let digits = format!("{} digits after `{}`", base_name(base), &text[..2]);
        digit_sequence(text, &mut i, base, &digits)?;
    } else {
        digit_sequence(text, &mut i, base, "digits")?;
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            kind = TokenKind::Float;
            digit_sequence(text, &mut i, base, "fractional digits")?;
        }
        if matches!(bytes.get(i), Some(b'e' | b'E')) {
            i += 1;
            kind = TokenKind::Float;
            if matches!(bytes.get(i), Some(b'+' | b'-')) {
                i += 1;
            }
            digit_sequence(text, &mut i, base, "exponent digits")?;
        }
    }
    match bytes.get(i) {
        None => Ok(kind),
        Some(b) if b.is_ascii_digit() => Err(NumberError::invalid_digit(text, i, base)),
        Some(_) => Err(NumberError::suffix(text, i)),
    }
}

fn digit_sequence(text: &str, i: &mut usize, base: IntBase, what: &str) -> Result<(), NumberError> {
    let bytes = text.as_bytes();
    match bytes.get(*i) {
        Some(&b) if is_digit(b, base) => *i += 1,
        Some(b'_') => {
            let message = "digit separator must be between two digits".into();
            return Err(NumberError::new(*i, *i + 1, message, SEPARATOR_NOTE));
        }
        Some(_) => {
            let message = format!("expected {what}, found {:?}", char_at(text, *i));
            let note = "the literal is incomplete";
            return Err(NumberError::new(*i, next_char(text, *i), message, note));
        }
        None => {
            // Point at the last character; the end of the run has no width.
            let last = text.len() - 1;
            let message = format!("expected {what}");
            return Err(NumberError::new(
                last,
                text.len(),
                message,
                "the literal is incomplete",
            ));
        }
    }
    loop {
        match (bytes.get(*i), bytes.get(*i + 1)) {
            (Some(&b), _) if is_digit(b, base) => *i += 1,
            (Some(b'_'), Some(&b)) if is_digit(b, base) => *i += 2,
            (Some(b'_'), Some(b)) if b.is_ascii_digit() => {
                return Err(NumberError::invalid_digit(text, *i + 1, base));
            }
            (Some(b'_'), Some(b)) if b.is_ascii_alphabetic() && !matches!(b, b'e' | b'E') => {
                return Err(NumberError::suffix(text, *i));
            }
            (Some(b'_'), _) => {
                let message = "digit separator must be between two digits".into();
                return Err(NumberError::new(*i, *i + 1, message, SEPARATOR_NOTE));
            }
            _ => return Ok(()),
        }
    }
}

fn is_digit(byte: u8, base: IntBase) -> bool {
    (byte as char).is_digit(base.radix())
}

fn base_name(base: IntBase) -> &'static str {
    match base {
        IntBase::Binary => "binary",
        IntBase::Octal => "octal",
        IntBase::Decimal => "decimal",
        IntBase::Hexadecimal => "hexadecimal",
    }
}

fn char_at(text: &str, i: usize) -> char {
    text[i..].chars().next().unwrap_or(' ')
}

fn next_char(text: &str, i: usize) -> usize {
    text.get(i..)
        .and_then(|rest| rest.chars().next())
        .map_or(text.len(), |ch| i + ch.len_utf8())
}
