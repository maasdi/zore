use std::sync::Arc;

pub(super) const MAX_PATTERN: usize = 4096;
pub(super) const MAX_CAPTURES: usize = 16;

#[derive(Clone, Debug)]
pub(super) enum ClassItem {
    Range(char, char),
    Ascii(char),
}

#[derive(Clone, Debug)]
pub(super) struct Class {
    pub negated: bool,
    pub items: Vec<ClassItem>,
}

#[derive(Clone, Debug)]
pub(super) enum Matcher {
    Literal(char),
    Dot,
    Class(Arc<Class>),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Assertion {
    Start,
    End,
    Word(bool),
}

#[derive(Debug)]
pub(super) enum Node {
    Empty,
    Consume(Matcher),
    Assert(Assertion),
    Sequence(Vec<Node>),
    Alternate(Vec<Node>),
    Group(Option<usize>, Box<Node>),
    Repeat {
        body: Box<Node>,
        min: usize,
        max: Option<usize>,
        greedy: bool,
    },
}

enum Escape {
    Rune(char),
    Class(char),
    Boundary(bool),
}

struct Parser<'a> {
    pattern: &'a str,
    at: usize,
    depth: usize,
    captures: usize,
}

pub(super) fn parse(pattern: &str) -> Result<(Node, usize), String> {
    if pattern.len() > MAX_PATTERN {
        return Err("pattern byte limit exceeded".into());
    }
    let mut parser = Parser {
        pattern,
        at: 0,
        depth: 0,
        captures: 0,
    };
    let node = parser.alternate()?;
    if parser.peek().is_some() {
        return parser.error("unexpected closing parenthesis");
    }
    Ok((node, parser.captures))
}

impl Parser<'_> {
    fn error<T>(&self, reason: &str) -> Result<T, String> {
        Err(format!("at byte {}: {reason}", self.at))
    }

    fn peek(&self) -> Option<char> {
        self.pattern[self.at..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.at += ch.len_utf8();
        Some(ch)
    }

    fn take(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn alternate(&mut self) -> Result<Node, String> {
        let mut arms = vec![self.sequence()?];
        while self.take('|') {
            arms.push(self.sequence()?);
        }
        Ok(if arms.len() == 1 {
            arms.pop().unwrap()
        } else {
            Node::Alternate(arms)
        })
    }

    fn sequence(&mut self) -> Result<Node, String> {
        let mut nodes = Vec::new();
        while !matches!(self.peek(), None | Some('|' | ')')) {
            nodes.push(self.repeated()?);
        }
        Ok(match nodes.len() {
            0 => Node::Empty,
            1 => nodes.pop().unwrap(),
            _ => Node::Sequence(nodes),
        })
    }

    fn repeated(&mut self) -> Result<Node, String> {
        let node = self.atom()?;
        let bounds = match self.peek() {
            Some('*') => {
                self.bump();
                Some((0, None))
            }
            Some('+') => {
                self.bump();
                Some((1, None))
            }
            Some('?') => {
                self.bump();
                Some((0, Some(1)))
            }
            Some('{') => Some(self.bounds()?),
            _ => None,
        };
        let Some((min, max)) = bounds else {
            return Ok(node);
        };
        if matches!(node, Node::Assert(_)) {
            return self.error("cannot repeat an assertion");
        }
        let greedy = !self.take('?');
        if matches!(self.peek(), Some('*' | '+' | '?' | '{')) {
            return self.error("stacked quantifiers are not supported");
        }
        Ok(Node::Repeat {
            body: Box::new(node),
            min,
            max,
            greedy,
        })
    }

    fn count(&mut self) -> Result<usize, String> {
        let start = self.at;
        let mut count = 0;
        while let Some(ch @ '0'..='9') = self.peek() {
            self.bump();
            count = count * 10 + ch as usize - '0' as usize;
            if count > 1000 {
                return self.error("repeat count exceeds 1000");
            }
        }
        if self.at == start {
            return self.error("expected repeat count");
        }
        Ok(count)
    }

    fn bounds(&mut self) -> Result<(usize, Option<usize>), String> {
        self.bump();
        let min = self.count()?;
        if self.take('}') {
            return Ok((min, Some(min)));
        }
        if !self.take(',') {
            return self.error("expected comma or closing brace");
        }
        let max = if self.peek() == Some('}') {
            None
        } else {
            Some(self.count()?)
        };
        if !self.take('}') {
            return self.error("unterminated repeat count");
        }
        if max.is_some_and(|max| max < min) {
            return self.error("reversed repeat bounds");
        }
        Ok((min, max))
    }

    fn atom(&mut self) -> Result<Node, String> {
        Ok(match self.bump().unwrap() {
            '(' => {
                self.depth += 1;
                if self.depth > 64 {
                    return self.error("group nesting limit exceeded");
                }
                let capture = if self.take('?') {
                    if !self.take(':') {
                        return self.error("only noncapturing (?:...) groups are supported");
                    }
                    None
                } else {
                    self.captures += 1;
                    if self.captures > MAX_CAPTURES {
                        return self.error("capture limit exceeded");
                    }
                    Some(self.captures)
                };
                let body = self.alternate()?;
                if !self.take(')') {
                    return self.error("unterminated group");
                }
                self.depth -= 1;
                Node::Group(capture, Box::new(body))
            }
            '[' => Node::Consume(Matcher::Class(Arc::new(self.class()?))),
            '.' => Node::Consume(Matcher::Dot),
            '^' => Node::Assert(Assertion::Start),
            '$' => Node::Assert(Assertion::End),
            '\\' => match self.escape()? {
                Escape::Rune(ch) => Node::Consume(Matcher::Literal(ch)),
                Escape::Class(kind) => Node::Consume(Matcher::Class(Arc::new(Class {
                    negated: false,
                    items: vec![ClassItem::Ascii(kind)],
                }))),
                Escape::Boundary(word) => Node::Assert(Assertion::Word(word)),
            },
            '*' | '+' | '?' | '{' | '}' | ']' => {
                return self.error("unexpected metacharacter");
            }
            ch => Node::Consume(Matcher::Literal(ch)),
        })
    }

    fn class_item(&mut self) -> Result<ClassItem, String> {
        match self.bump() {
            Some('\\') => match self.escape()? {
                Escape::Rune(ch) => Ok(ClassItem::Range(ch, ch)),
                Escape::Class(kind) => Ok(ClassItem::Ascii(kind)),
                Escape::Boundary(_) => self.error("word boundary is not a class item"),
            },
            Some('[') => self.error("nested and POSIX classes are not supported"),
            Some(ch) => Ok(ClassItem::Range(ch, ch)),
            None => self.error("unterminated class"),
        }
    }

    fn class(&mut self) -> Result<Class, String> {
        let negated = self.take('^');
        let mut items = Vec::new();
        while self.peek() != Some(']') {
            if self.peek().is_none() {
                return self.error("unterminated class");
            }
            let raw_hyphen = self.peek() == Some('-');
            let item = self.class_item()?;
            if raw_hyphen && !items.is_empty() && self.peek() != Some(']') {
                return self.error("a middle hyphen must be escaped");
            }
            let range = !raw_hyphen
                && self.peek() == Some('-')
                && !self.pattern[self.at..].starts_with("-]");
            if range {
                self.bump();
                let end = self.class_item()?;
                match (item, end) {
                    (ClassItem::Range(start, _), ClassItem::Range(end, _)) if start <= end => {
                        items.push(ClassItem::Range(start, end));
                    }
                    _ => return self.error("invalid or reversed class range"),
                }
            } else {
                items.push(item);
            }
        }
        self.bump();
        if items.is_empty() {
            return self.error("empty class");
        }
        Ok(Class { negated, items })
    }

    fn hex(&mut self, digits: usize) -> Result<u32, String> {
        let mut value = 0;
        for _ in 0..digits {
            let Some(digit) = self.bump().and_then(|ch| ch.to_digit(16)) else {
                return self.error("expected hexadecimal digit");
            };
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn escape(&mut self) -> Result<Escape, String> {
        Ok(match self.bump() {
            Some('n') => Escape::Rune('\n'),
            Some('r') => Escape::Rune('\r'),
            Some('t') => Escape::Rune('\t'),
            Some('f') => Escape::Rune('\u{c}'),
            Some('v') => Escape::Rune('\u{b}'),
            Some(ch @ ('d' | 'D' | 's' | 'S' | 'w' | 'W')) => Escape::Class(ch),
            Some('b') => Escape::Boundary(true),
            Some('B') => Escape::Boundary(false),
            Some('x') => Escape::Rune(char::from_u32(self.hex(2)?).unwrap()),
            Some('u') => {
                if !self.take('{') {
                    return self.error("expected braced Unicode scalar");
                }
                let start = self.at;
                let mut digits = 0;
                let mut value = 0;
                while self.peek() != Some('}') && digits < 6 {
                    value = value * 16 + self.hex(1)?;
                    digits += 1;
                }
                if self.at == start || !self.take('}') {
                    return self.error("expected one to six Unicode hex digits");
                }
                let Some(ch) = char::from_u32(value) else {
                    return self.error("invalid Unicode scalar");
                };
                Escape::Rune(ch)
            }
            Some(
                ch @ ('\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^'
                | '$' | '-'),
            ) => Escape::Rune(ch),
            Some(_) => return self.error("unsupported escape"),
            None => return self.error("trailing backslash"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_and_unsupported_syntax_is_rejected_with_byte_positions() {
        for pattern in [
            "(",
            ")",
            "[",
            "[]",
            "[^]",
            "[z-a]",
            "[a-b-c]",
            "[a-\\d]",
            "[[:alpha:]]",
            "*a",
            "a**",
            "a*+",
            "a???",
            "^*",
            "a{",
            "a{,2}",
            "a{2,1}",
            "a{1001}",
            "a{2",
            "a}",
            "a]",
            "\\",
            "\\q",
            "\\1",
            "\\0",
            "\\p{L}",
            "\\xG0",
            "\\x0",
            "\\u{}",
            "\\u{D800}",
            "\\u{110000}",
            "\\u1234",
            "(?i)a",
            "(?=a)",
            "(?P<name>a)",
            "(?!a)",
            "(?<=a)",
            "[\\b]",
        ] {
            let error = parse(pattern).unwrap_err();
            assert!(error.starts_with("at byte "), "{pattern:?}: {error}");
        }
    }

    #[test]
    fn pattern_capture_depth_and_repeat_limits_are_checked_before_expansion() {
        assert!(parse(&"a".repeat(MAX_PATTERN)).is_ok());
        assert!(
            parse(&"a".repeat(MAX_PATTERN + 1))
                .unwrap_err()
                .contains("pattern byte limit")
        );
        assert!(parse(&"()".repeat(MAX_CAPTURES)).is_ok());
        assert!(
            parse(&"()".repeat(MAX_CAPTURES + 1))
                .unwrap_err()
                .contains("capture limit")
        );
        assert!(parse(&("(?:".repeat(64) + "a" + &")".repeat(64))).is_ok());
        assert!(
            parse(&("(?:".repeat(65) + "a" + &")".repeat(65)))
                .unwrap_err()
                .contains("nesting limit")
        );
        assert!(parse("a{1000,1000}").is_ok());
    }
}
