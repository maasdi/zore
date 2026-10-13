use super::layout::Layout;
use crate::lexer::{Keyword, Punct, Separator, Token, TokenKind};
use crate::source::SourceFile;

struct Comment<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

pub(super) fn write(file: &SourceFile, tokens: &[Token], layout: &Layout) -> String {
    let text = file.text();
    let comments = comments(text, tokens);
    let mut writer = Writer {
        output: String::new(),
        indent: 0,
        previous: None,
        blank_pending: false,
    };
    let mut delimiters = Vec::new();
    let mut comment_index = 0;
    let mut source_end = 0;
    let mut closing = vec![0; tokens.len()];
    let mut openings = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::Punct(Punct::LBrace | Punct::LParen | Punct::LBracket) => {
                openings.push(index)
            }
            TokenKind::Punct(Punct::RBrace | Punct::RParen | Punct::RBracket) => {
                if let Some(open) = openings.pop() {
                    closing[open] = index;
                }
            }
            _ => {}
        }
    }
    for (index, token) in tokens.iter().enumerate() {
        let start = token.span.start() as usize;
        while let Some(comment) = comments.get(comment_index).filter(|c| c.start <= start) {
            writer.gap(&text[source_end.min(comment.start)..comment.start]);
            if writer.blank_pending && writer.at_line_start() {
                writer.blank_line();
                writer.blank_pending = false;
            }
            if !writer.at_line_start() {
                writer.space();
            }
            writer.text(comment.text);
            if comment.text.starts_with("//") {
                writer.newline();
            }
            source_end = source_end.max(comment.end);
            comment_index += 1;
        }
        writer.gap(&text[source_end.min(start)..start]);
        source_end = source_end.max(token.span.end() as usize);
        match token.kind {
            TokenKind::Eof => break,
            TokenKind::Semicolon(separator) => {
                if separator == Separator::Explicit {
                    writer.trim_space();
                    writer.text(";");
                    if layout.inline_separators.contains(&token.span.start()) {
                        writer.space();
                        continue;
                    }
                }
                writer.newline();
                writer.previous = None;
                continue;
            }
            _ => {}
        }
        let spelling = &text[start..token.span.end() as usize];
        let mut multiline_open = false;
        match token.kind {
            TokenKind::Punct(Punct::RBrace | Punct::RParen | Punct::RBracket) => {
                if delimiters.pop() == Some(true) {
                    writer.indent = writer.indent.saturating_sub(1);
                    writer.newline();
                }
            }
            TokenKind::Punct(Punct::LBrace | Punct::LParen | Punct::LBracket) => {
                let close = closing[index];
                let end = tokens[close].span.start() as usize;
                let empty =
                    index + 1 == close && !text[token.span.end() as usize..end].contains('/');
                let next = tokens[index + 1].span.start() as usize;
                let previous = tokens[..close]
                    .iter()
                    .rfind(|t| !t.span.is_empty())
                    .map_or(token.span.end() as usize, |t| t.span.end() as usize);
                multiline_open = !empty
                    && (layout.blocks.contains(&token.span.start())
                        || text[token.span.end() as usize..next].contains('\n')
                        || text[previous..end].contains('\n'));
                delimiters.push(multiline_open);
            }
            _ => {}
        }
        if writer.blank_pending && writer.at_line_start() {
            writer.blank_line();
            writer.blank_pending = false;
        }
        if let Some(previous) = writer.previous {
            if needs_space(text, previous, token, layout) {
                writer.space();
            } else {
                writer.trim_space();
            }
        }
        writer.text(spelling);
        writer.previous = Some(token);
        writer.blank_pending |= layout.blank_after.contains(&token.span.end());
        if multiline_open {
            writer.indent += 1;
            writer.newline();
        }
    }
    while let Some(comment) = comments.get(comment_index) {
        writer.gap(&text[source_end.min(comment.start)..comment.start]);
        if !writer.at_line_start() {
            writer.space();
        }
        writer.text(comment.text);
        writer.newline();
        source_end = comment.end;
        comment_index += 1;
    }
    while writer.output.ends_with([' ', '\n']) {
        writer.output.pop();
    }
    writer.output.push('\n');
    writer.output
}

fn comments<'a>(text: &'a str, tokens: &[Token]) -> Vec<Comment<'a>> {
    let mut comments = Vec::new();
    let mut end = 0;
    for token in tokens
        .iter()
        .filter(|t| !t.span.is_empty() || t.kind == TokenKind::Eof)
    {
        let start = token.span.start() as usize;
        let gap = &text[end..start];
        let mut pos = 0;
        while pos < gap.len() {
            if gap[pos..].starts_with("//") {
                let length = gap[pos..].find('\n').unwrap_or(gap.len() - pos);
                let comment_end = end + pos + length;
                let comment_start = end + pos;
                comments.push(Comment {
                    start: comment_start,
                    end: comment_end,
                    text: &text[comment_start..comment_end],
                });
                pos += length;
            } else if gap[pos..].starts_with("/*") {
                let length = gap[pos + 2..].find("*/").expect("valid block comment") + 4;
                let comment_start = end + pos;
                let comment_end = comment_start + length;
                comments.push(Comment {
                    start: comment_start,
                    end: comment_end,
                    text: &text[comment_start..comment_end],
                });
                pos += length;
            } else {
                pos += gap[pos..].chars().next().expect("gap character").len_utf8();
            }
        }
        end = token.span.end() as usize;
    }
    comments
}

fn needs_space(text: &str, previous: &Token, current: &Token, layout: &Layout) -> bool {
    let before = &text[previous.span.start() as usize..previous.span.end() as usize];
    let after = &text[current.span.start() as usize..current.span.end() as usize];
    if matches!(previous.kind, TokenKind::Punct(_))
        && matches!(current.kind, TokenKind::Punct(_))
        && (Punct::ALL
            .iter()
            .any(|p| p.as_str().strip_prefix(before) == Some(after))
            || matches!((before, after), ("+", "+") | ("-", "-") | ("/", "/" | "*")))
    {
        return true;
    }
    if layout.spaced_before.contains(&current.span.start()) {
        return true;
    }
    if layout.tight_before.contains(&current.span.start())
        || layout.tight_after.contains(&previous.span.end())
    {
        return false;
    }
    match (&previous.kind, &current.kind) {
        (
            _,
            TokenKind::Punct(
                Punct::RParen
                | Punct::RBracket
                | Punct::Comma
                | Punct::Colon
                | Punct::Dot
                | Punct::Question,
            ),
        )
        | (TokenKind::Punct(Punct::LParen | Punct::LBracket | Punct::Dot), _) => false,
        (_, TokenKind::Punct(Punct::RBrace)) => false,
        (_, TokenKind::Punct(Punct::LBrace)) => layout.blocks.contains(&current.span.start()),
        (_, TokenKind::Punct(Punct::LParen)) => !matches!(
            previous.kind,
            TokenKind::Ident
                | TokenKind::Keyword(Keyword::Func | Keyword::Channel)
                | TokenKind::Punct(Punct::Gt | Punct::Shr)
        ),
        (_, TokenKind::Punct(Punct::LBracket)) => !matches!(
            previous.kind,
            TokenKind::Ident
                | TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace)
                | TokenKind::Keyword(Keyword::Map | Keyword::Mut)
        ),
        (TokenKind::Punct(Punct::LBrace), _) => false,
        (_, TokenKind::Punct(Punct::Not | Punct::Plus | Punct::Minus | Punct::Caret))
            if layout.unary.contains(&current.span.start()) =>
        {
            !matches!(
                previous.kind,
                TokenKind::Punct(Punct::LParen | Punct::LBracket)
            )
        }
        _ => true,
    }
}

struct Writer<'a> {
    output: String,
    indent: usize,
    previous: Option<&'a Token>,
    blank_pending: bool,
}

impl Writer<'_> {
    fn at_line_start(&self) -> bool {
        self.output.is_empty() || self.output.ends_with('\n')
    }
    fn trim_space(&mut self) {
        while self.output.ends_with(' ') {
            self.output.pop();
        }
    }
    fn space(&mut self) {
        if !self.at_line_start() && !self.output.ends_with(' ') {
            self.output.push(' ');
        }
    }
    fn newline(&mut self) {
        self.trim_space();
        if !self.at_line_start() {
            self.output.push('\n');
        }
    }
    fn blank_line(&mut self) {
        if !self.output.is_empty() && !self.output.ends_with("\n\n") {
            self.newline();
            self.output.push('\n');
        }
    }
    fn text(&mut self, text: &str) {
        if self.at_line_start() {
            for _ in 0..self.indent {
                self.output.push_str("    ");
            }
        }
        self.output.push_str(text);
    }
    fn gap(&mut self, gap: &str) {
        let newlines = gap.bytes().filter(|&b| b == b'\n').count();
        if newlines > 0 {
            self.newline();
        }
        if newlines > 1 {
            self.blank_line();
        }
    }
}
