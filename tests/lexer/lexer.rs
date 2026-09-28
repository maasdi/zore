//! Lexer tests for tokenization, literals, comments, and recovery.

use zore::lexer::{Lexed, lex};
use zore::source::SourceMap;
use zore::token::{IntBase, Keyword, Punct, ReservedWord, Separator, TokenKind};

struct Case {
    sources: SourceMap,
    lexed: Lexed,
}

impl Case {
    fn new(text: &str) -> Self {
        let mut sources = SourceMap::new();
        let id = sources.add("test.ore", text.into()).unwrap();
        let lexed = lex(sources.file(id).unwrap());
        Self { sources, lexed }
    }

    /// Token kinds with source text, excluding the final `Eof`.
    fn tokens(&self) -> Vec<(TokenKind, &str)> {
        let (eof, rest) = self.lexed.tokens.split_last().unwrap();
        assert_eq!(eof.kind, TokenKind::Eof);
        rest.iter()
            .map(|token| (token.kind.clone(), self.sources.slice(token.span).unwrap()))
            .collect()
    }

    fn kinds(&self) -> Vec<TokenKind> {
        self.tokens().into_iter().map(|(kind, _)| kind).collect()
    }

    /// Diagnostic messages with the source text their primary span covers.
    fn errors(&self) -> Vec<(&str, &str)> {
        self.lexed
            .diagnostics
            .iter()
            .map(|d| (d.message(), self.sources.slice(d.span()).unwrap()))
            .collect()
    }

    fn assert_clean(&self) {
        assert!(self.lexed.diagnostics.is_empty(), "{:?}", self.errors());
    }

    fn render(&self) -> String {
        self.lexed
            .diagnostics
            .iter()
            .map(|d| d.render(&self.sources).unwrap())
            .collect()
    }
}

fn ident() -> TokenKind {
    TokenKind::Ident
}

fn semi(separator: Separator) -> TokenKind {
    TokenKind::Semicolon(separator)
}

fn punct(punct: Punct) -> TokenKind {
    TokenKind::Punct(punct)
}

fn single(text: &str) -> TokenKind {
    let case = Case::new(text);
    case.assert_clean();
    let kinds = case.kinds();
    assert_eq!(kinds.len(), 2, "{text:?}: {kinds:?}");
    assert_eq!(kinds[1], semi(Separator::Eof), "{text:?}");
    kinds[0].clone()
}

fn string_value(text: &str) -> String {
    match single(text) {
        TokenKind::String(value) => value,
        other => panic!("{text:?} lexed as {other:?}"),
    }
}

fn rune_value(text: &str) -> char {
    match single(text) {
        TokenKind::Rune(value) => value,
        other => panic!("{text:?} lexed as {other:?}"),
    }
}

/// Lex a malformed spelling and return the primary diagnostic message and the
/// source text it covers. The literal must remain one recovery token.
fn malformed(text: &str) -> (String, String) {
    let case = Case::new(text);
    let errors = case.errors();
    assert!(!errors.is_empty(), "{text:?} was accepted");
    let tokens = case.tokens();
    assert_eq!(tokens[0], (TokenKind::MalformedLiteral, text), "{text:?}");
    (errors[0].0.to_owned(), errors[0].1.to_owned())
}

#[test]
fn empty_whitespace_and_comment_only_files_have_no_separators() {
    for text in [
        "",
        "   \t\r\n\n",
        "// note",
        "/* a\nb */\n",
        "// a\n/* b */\n",
    ] {
        let case = Case::new(text);
        case.assert_clean();
        assert!(case.kinds().is_empty(), "{text:?}");
        let eof = case.lexed.tokens.last().unwrap();
        assert_eq!(eof.span.start() as usize, text.len());
        assert!(eof.span.is_empty());
    }
}

#[test]
fn ascii_identifiers_keywords_and_reserved_words() {
    for name in [
        "user",
        "user2",
        "user_name",
        "User",
        "_internal",
        "_User",
        "a0",
        "Z9",
        "__",
    ] {
        assert_eq!(single(name), ident(), "{name}");
    }
    assert_eq!(single("_"), TokenKind::Underscore);
    for &keyword in Keyword::ALL {
        let case = Case::new(keyword.as_str());
        case.assert_clean();
        assert_eq!(case.kinds()[0], TokenKind::Keyword(keyword));
    }
    for &word in ReservedWord::ALL {
        let case = Case::new(word.as_str());
        case.assert_clean();
        assert_eq!(
            case.kinds(),
            [TokenKind::Reserved(word)],
            "no insertion after {word:?}"
        );
    }
    // Case-sensitive whole-word matching.
    for name in [
        "functionName",
        "Func",
        "True",
        "_return",
        "Interface",
        "TRAIT",
        "_unsafe",
        "deferred",
        "myenum",
        "matchValue",
    ] {
        assert_eq!(single(name), ident(), "{name}");
    }
    // Predeclared names are identifiers, not keywords.
    for name in [
        "int", "string", "bool", "rune", "Array", "Task", "error", "println", "clone", "drop",
    ] {
        assert_eq!(single(name), ident(), "{name}");
    }
}

#[test]
fn non_ascii_identifier_characters_are_diagnosed_in_one_token() {
    for (text, bad) in [("café", "é"), ("用户", "用"), ("α", "α"), ("user١", "١")] {
        let case = Case::new(text);
        assert_eq!(case.tokens()[0], (ident(), text));
        let errors = case.errors();
        assert_eq!(errors.len(), 1, "{text}");
        assert!(errors[0].0.contains("non-ASCII"), "{errors:?}");
        assert_eq!(errors[0].1, bad);
    }
    // `2user` is a malformed number, never an identifier.
    let (message, span) = malformed("2user");
    assert!(message.contains("suffix"), "{message}");
    assert_eq!(span, "user");
}

#[test]
fn comments_are_skipped_without_nesting() {
    for text in [
        "// text\n",
        "// text",
        "//",
        "/**/",
        "/* first\nsecond */",
        "/* outer /* inner */",
        "/* // text */",
        "// /* text",
        "/* \" */",
        "// café 用户",
        "/* café 用户 */",
    ] {
        let case = Case::new(text);
        case.assert_clean();
        assert!(case.kinds().is_empty(), "{text:?}");
    }
    let case = Case::new("/* outer /* inner */ tail");
    assert_eq!(case.tokens()[0], (ident(), "tail"));

    let case = Case::new("user/* note */Name");
    let tokens = case.tokens();
    assert_eq!(tokens[0], (ident(), "user"));
    assert_eq!(tokens[1], (ident(), "Name"));

    for text in [r#""https://example.test""#, r#""/* text */""#] {
        assert_eq!(
            single(text),
            TokenKind::String(text[1..text.len() - 1].into())
        );
    }
}

#[test]
fn unterminated_block_comment_identifies_opening_delimiter() {
    for text in ["/* text", "/*", "x /* a\nb"] {
        let case = Case::new(text);
        assert_eq!(
            case.errors(),
            [("unterminated block comment", "/*")],
            "{text:?}"
        );
    }
    let rendered = Case::new("let a = 1\n  /* open\n").render();
    assert!(rendered.contains("test.ore:2:3"), "{rendered}");
    assert!(rendered.contains("^^ comment starts here"), "{rendered}");
}

#[test]
fn tokens_after_multiline_unicode_comment_keep_byte_spans() {
    let text = "/* café\n用户 */ name";
    let case = Case::new(text);
    let token = case
        .lexed
        .tokens
        .iter()
        .find(|t| t.kind == ident())
        .unwrap();
    assert_eq!(token.span.start() as usize, text.find("name").unwrap());
    let file = case.sources.file(token.span.file()).unwrap();
    let location = file.location(token.span.start()).unwrap();
    assert_eq!((location.line, location.column), (2, 7));
}

#[test]
fn semicolons_are_inserted_after_eligible_tokens() {
    for ending in [
        "name", "1", "1.5", "\"text\"", "`raw`", "'r'", "return", "break", "continue", "true",
        "false", "nil", ")", "]", "}", "?", "_",
    ] {
        let at_newline = Case::new(&format!("{ending}\n"));
        let tokens = at_newline.tokens();
        assert_eq!(
            tokens.last().unwrap().0,
            semi(Separator::Newline),
            "{ending}"
        );
        let at_eof = Case::new(ending);
        assert_eq!(
            at_eof.kinds().last().unwrap(),
            &semi(Separator::Eof),
            "{ending}"
        );
    }
    for ending in [
        "package", "import", "func", "type", "struct", "let", "var", "const", "mut", "own", "if",
        "else", "for", "async", "await", "go", "map", "channel", "(", "[", "{", ",", "+", "=", ".",
        ":", "!",
    ] {
        for suffix in ["\n", ""] {
            let case = Case::new(&format!("{ending}{suffix}"));
            let kinds = case.kinds();
            assert_eq!(kinds.len(), 1, "{ending:?}{suffix:?}: {kinds:?}");
        }
    }
}

#[test]
fn inserted_semicolons_are_located_at_the_triggering_newline() {
    let case = Case::new("name\r\nother");
    let tokens = &case.lexed.tokens;
    assert_eq!(tokens[1].kind, semi(Separator::Newline));
    assert_eq!((tokens[1].span.start(), tokens[1].span.end()), (4, 4));
    assert_eq!(tokens[3].kind, semi(Separator::Eof));
    assert_eq!(tokens[3].span.start(), 11);

    let case = Case::new("name /* first\nsecond\nthird */\n");
    let separators: Vec<_> = case
        .lexed
        .tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Semicolon(_)))
        .collect();
    assert_eq!(separators.len(), 1);
    assert_eq!(separators[0].span.start(), 13);
}

#[test]
fn semicolon_insertion_follows_statement_boundary_rules() {
    use Separator::*;
    let cases: &[(&str, &[&str])] = &[
        ("name\r other", &["name", "other", "<eof>"]),
        ("name;\n", &["name", ";"]),
        ("name\n\n", &["name", "<nl>"]),
        (
            "let total = first +\nsecond",
            &["let", "total", "=", "first", "+", "second", "<eof>"],
        ),
        (
            "total = first\n+ second",
            &["total", "=", "first", "<nl>", "+", "second", "<eof>"],
        ),
        (
            "read(file)?\nnext()",
            &[
                "read", "(", "file", ")", "?", "<nl>", "next", "(", ")", "<eof>",
            ],
        ),
        ("return\nvalue", &["return", "<nl>", "value", "<eof>"]),
        (
            "await\noperation()",
            &["await", "operation", "(", ")", "<eof>"],
        ),
        ("go\nwork()", &["go", "work", "(", ")", "<eof>"]),
        ("name // note\nother", &["name", "<nl>", "other", "<eof>"]),
        ("name // note", &["name", "<eof>"]),
        ("name /* note */ other", &["name", "other", "<eof>"]),
        (
            "name /* first\nsecond */ other",
            &["name", "<nl>", "other", "<eof>"],
        ),
        (
            "greet(\nuser,\n)",
            &["greet", "(", "user", ",", ")", "<eof>"],
        ),
        (
            "greet(\nuser\n)",
            &["greet", "(", "user", "<nl>", ")", "<eof>"],
        ),
        (
            "func main()\n{}",
            &["func", "main", "(", ")", "<nl>", "{", "}", "<eof>"],
        ),
        (
            "let a = 1; let b = 2",
            &["let", "a", "=", "1", ";", "let", "b", "=", "2", "<eof>"],
        ),
        ("`first\nsecond`\n", &["`first\nsecond`", "<nl>"]),
        ("\"\\n\"\n", &["\"\\n\"", "<nl>"]),
    ];
    for (text, expected) in cases {
        let case = Case::new(text);
        case.assert_clean();
        let actual: Vec<&str> = case
            .tokens()
            .into_iter()
            .map(|(kind, text)| match kind {
                TokenKind::Semicolon(Newline) => "<nl>",
                TokenKind::Semicolon(Eof) => "<eof>",
                _ => text,
            })
            .collect();
        assert_eq!(&actual, expected, "{text:?}");
    }
}

#[test]
fn double_quoted_strings_decode_the_locked_escape_set() {
    assert_eq!(string_value(r#""Hello""#), "Hello");
    assert_eq!(string_value(r#""""#), "");
    assert_eq!(string_value(r#""café 用户""#), "café 用户");
    assert_eq!(string_value(r#""${name} {name}""#), "${name} {name}");
    assert_eq!(
        string_value(r#""/* text */ // text""#),
        "/* text */ // text"
    );
    assert_eq!(string_value(r#""\n\r\t""#), "\n\r\t");
    assert_eq!(string_value(r#""\\""#), "\\");
    assert_eq!(string_value(r#""\"""#), "\"");
    assert_eq!(string_value(r#""'""#), "'");
    for text in [r#""\u0041""#, r#""\U00000041""#, r#""A""#] {
        assert_eq!(string_value(text), "A");
    }
    for text in [r#""\u00e9""#, r#""\u00E9""#, r#""é""#] {
        assert_eq!(string_value(text), "é");
    }
    assert_eq!(string_value(r#""\U0001F600""#), "😀");
    assert_eq!(string_value(r#""\u0041B""#), "AB");
    assert_eq!(string_value(r#""\U00000041B""#), "AB");
    assert_eq!(string_value(r#""\u0000""#), "\0");
    assert_eq!(string_value(r#""\uD7FF""#), "\u{D7FF}");
    assert_eq!(string_value(r#""\uE000""#), "\u{E000}");
    assert_eq!(string_value(r#""\U0010FFFF""#), "\u{10FFFF}");
    // Escapes decode once.
    assert_eq!(string_value(r#""\\n""#), "\\n");
    assert_eq!(string_value(r#""\u005Cn""#), "\\n");
}

#[test]
fn malformed_string_escapes_are_diagnosed_at_their_spelling() {
    let cases = [
        (r#""\uD800""#, r"\uD800", "not a Unicode scalar"),
        (r#""\uDFFF""#, r"\uDFFF", "not a Unicode scalar"),
        (r#""\U0000D800""#, r"\U0000D800", "not a Unicode scalar"),
        (r#""\U00110000""#, r"\U00110000", "not a Unicode scalar"),
        (r#""\UFFFFFFFF""#, r"\UFFFFFFFF", "not a Unicode scalar"),
        (r#""\u123""#, r"\u123", "exactly 4"),
        (r#""\U0000041""#, r"\U0000041", "exactly 8"),
        (r#""\u12G4""#, r"\u12", "exactly 4"),
        (r#""\u{0041}""#, r"\u", "exactly 4"),
        (r#""\u0_41""#, r"\u0", "exactly 4"),
        (r#""\q""#, r"\q", "unknown escape"),
        (r#""\a""#, r"\a", "unknown escape"),
        (r#""\b""#, r"\b", "unknown escape"),
        (r#""\f""#, r"\f", "unknown escape"),
        (r#""\v""#, r"\v", "unknown escape"),
        (r#""\x41""#, r"\x", "unknown escape"),
        (r#""\101""#, r"\1", "unknown escape"),
        (r#""\0""#, r"\0", "unknown escape"),
        (r#""\'""#, r"\'", "unknown escape"),
    ];
    for (text, span, message) in cases {
        let (actual, actual_span) = malformed(text);
        assert!(actual.contains(message), "{text}: {actual}");
        assert_eq!(actual_span, span, "{text}");
    }
    // Both halves of a surrogate pair are rejected.
    assert_eq!(Case::new(r#""\uD83D\uDE00""#).errors().len(), 2);
}

#[test]
fn unterminated_and_multiline_quoted_strings_are_rejected() {
    for text in [
        "\"abc",
        "\"abc\ndef",
        "\"abc\r\ndef",
        "\"abc\\\ndef",
        "\"abc\\",
    ] {
        let case = Case::new(text);
        let errors = case.errors();
        assert_eq!(errors.len(), 1, "{text:?}: {errors:?}");
        assert_eq!(errors[0], ("unterminated string literal", "\""), "{text:?}");
    }
    let rendered = Case::new("\"abc\\\n\"").render();
    assert!(
        rendered.contains("does not continue a literal"),
        "{rendered}"
    );
    // The newline stays outside the literal and still inserts a separator.
    let case = Case::new("\"abc\nnext");
    let kinds = case.kinds();
    assert_eq!(
        kinds[..3],
        [
            TokenKind::MalformedLiteral,
            semi(Separator::Newline),
            ident()
        ]
    );
}

#[test]
fn raw_strings_preserve_contents() {
    assert_eq!(string_value(r"`C:\projects\zore`"), r"C:\projects\zore");
    assert_eq!(string_value("`first\n    second`"), "first\n    second");
    assert_eq!(string_value("`a\r\nb\rc`"), "a\r\nb\rc");
    assert_eq!(
        string_value("`\"quoted\" /* c */ // d`"),
        "\"quoted\" /* c */ // d"
    );
    assert_eq!(string_value("`café 用户`"), "café 用户");
    assert_eq!(string_value("`${name}`"), "${name}");
    assert_eq!(string_value("``"), "");
    assert_eq!(string_value("`a\\`"), "a\\");
    assert_eq!(string_value(r"`\q \uD800 \n`"), r"\q \uD800 \n");
    assert_eq!(string_value("\"`\""), "`");

    let case = Case::new("`open\nmore");
    assert_eq!(case.errors(), [("unterminated raw string literal", "`")]);
    assert_eq!(
        case.kinds(),
        [TokenKind::MalformedLiteral, semi(Separator::Eof)]
    );
}

#[test]
fn string_spans_cover_unicode_and_multiline_contents() {
    let text = "`é\n😀` next";
    let case = Case::new(text);
    let tokens = case.tokens();
    assert_eq!(tokens[0].1, "`é\n😀`");
    assert_eq!(tokens[1], (ident(), "next"));
}

#[test]
fn rune_literals_hold_exactly_one_scalar() {
    assert_eq!(rune_value("'A'"), 'A');
    assert_eq!(rune_value("'é'"), 'é');
    assert_eq!(rune_value("'😀'"), '😀');
    assert_eq!(rune_value(r"'\n'"), '\n');
    assert_eq!(rune_value(r"'\r'"), '\r');
    assert_eq!(rune_value(r"'\t'"), '\t');
    assert_eq!(rune_value(r"'\\'"), '\\');
    assert_eq!(rune_value(r"'\u005C'"), '\\');
    assert_eq!(rune_value(r"'\''"), '\'');
    assert_eq!(rune_value("'\"'"), '"');
    assert_eq!(rune_value(r#"'\"'"#), '"');
    assert_eq!(rune_value(r"'\u0041'"), 'A');
    assert_eq!(rune_value(r"'\U00000041'"), 'A');
    assert_eq!(rune_value(r"'\u00e9'"), 'é');
    assert_eq!(rune_value(r"'\u00E9'"), 'é');
    assert_eq!(rune_value(r"'\U0001F600'"), '😀');
    assert_eq!(rune_value(r"'\u0000'"), '\0');
    assert_eq!(rune_value(r"'\uD7FF'"), '\u{D7FF}');
    assert_eq!(rune_value(r"'\uE000'"), '\u{E000}');
    assert_eq!(rune_value(r"'\U0010FFFF'"), '\u{10FFFF}');
    assert_eq!(rune_value("'/'"), '/');
    assert_eq!(rune_value("'*'"), '*');
}

#[test]
fn malformed_rune_literals_are_rejected() {
    for (text, message) in [
        ("''", "empty rune"),
        ("'ab'", "more than one"),
        (r"'\u0041B'", "more than one"),
        ("'e\\u0301'", "more than one"),
        (r"'\\n'", "more than one"),
        (r"'\u005Cn'", "more than one"),
        (r"'\uD800'", "not a Unicode scalar"),
        (r"'\uDFFF'", "not a Unicode scalar"),
        (r"'\U0000D800'", "not a Unicode scalar"),
        (r"'\U00110000'", "not a Unicode scalar"),
        (r"'\UFFFFFFFF'", "not a Unicode scalar"),
        (r"'\u123'", "exactly 4"),
        (r"'\U0000041'", "exactly 8"),
        (r"'\u12G4'", "exactly 4"),
        (r"'\u{0041}'", "exactly 4"),
        (r"'\u0_41'", "exactly 4"),
        (r"'\q'", "unknown escape"),
        (r"'\x41'", "unknown escape"),
        (r"'\101'", "unknown escape"),
        (r"'\0'", "unknown escape"),
        (r"'\a'", "unknown escape"),
        (r"'\b'", "unknown escape"),
        (r"'\f'", "unknown escape"),
        (r"'\v'", "unknown escape"),
    ] {
        let (actual, _) = malformed(text);
        assert!(actual.contains(message), "{text}: {actual}");
    }
    let (_, span) = malformed("'ab'");
    assert_eq!(span, "'ab'");
    assert_eq!(Case::new(r"'\uD83D\uDE00'").errors().len(), 2);
    for text in ["'a\nb", "'a\r\nb", "'\\\nb", "'a"] {
        let case = Case::new(text);
        assert_eq!(
            case.errors(),
            [("unterminated rune literal", "'")],
            "{text:?}"
        );
    }
}

#[test]
fn integer_bases_and_separators() {
    use IntBase::*;
    let valid = [
        ("0", Decimal),
        ("00", Decimal),
        ("042", Decimal),
        ("0755", Decimal),
        ("08", Decimal),
        ("09", Decimal),
        ("0b101010", Binary),
        ("0B101010", Binary),
        ("0o755", Octal),
        ("0O755", Octal),
        ("0xFF", Hexadecimal),
        ("0Xff", Hexadecimal),
        ("0xFf", Hexadecimal),
        ("0b0", Binary),
        ("0B0", Binary),
        ("0o0", Octal),
        ("0O0", Octal),
        ("0x0", Hexadecimal),
        ("0X0", Hexadecimal),
        ("0b0010", Binary),
        ("0o007", Octal),
        ("0x00A", Hexadecimal),
        ("1_000", Decimal),
        ("10_00", Decimal),
        ("1_0_0_0", Decimal),
        ("0xFF_FF", Hexadecimal),
        ("0Xff_ff", Hexadecimal),
        ("0b1010_0101", Binary),
        ("0B1010_0101", Binary),
        ("0o7_55", Octal),
        ("0O7_55", Octal),
        ("0_755", Decimal),
        ("0_0", Decimal),
        ("0b0_0", Binary),
        ("0o0_0", Octal),
        ("0x0_0", Hexadecimal),
        ("0xF32", Hexadecimal),
        ("0xdeadBEEF", Hexadecimal),
    ];
    for (text, base) in valid {
        assert_eq!(single(text), TokenKind::Int(base), "{text}");
    }
    assert_eq!(single("_1000"), ident());
}

#[test]
fn malformed_integers_are_rejected_as_one_literal() {
    let cases = [
        ("0b", "expected binary digits", "b"),
        ("0B", "expected binary digits", "B"),
        ("0o", "expected octal digits", "o"),
        ("0O", "expected octal digits", "O"),
        ("0x", "expected hexadecimal digits", "x"),
        ("0X", "expected hexadecimal digits", "X"),
        ("0b2", "expected binary digits", "2"),
        ("0b102", "invalid digit '2'", "2"),
        ("0o8", "expected octal digits", "8"),
        ("0o79", "invalid digit '9'", "9"),
        ("0xG", "expected hexadecimal digits", "G"),
        ("1000_", "separator", "_"),
        ("0b10_", "separator", "_"),
        ("0o75_", "separator", "_"),
        ("0xFF_", "separator", "_"),
        ("1__000", "separator", "_"),
        ("0b1__0", "separator", "_"),
        ("0o7__5", "separator", "_"),
        ("0xF__F", "separator", "_"),
        ("0b_10", "separator", "_"),
        ("0B_10", "separator", "_"),
        ("0o_755", "separator", "_"),
        ("0O_755", "separator", "_"),
        ("0x_FF", "separator", "_"),
        ("0X_FF", "separator", "_"),
        ("0_b10", "suffix", "_b10"),
        ("0_o755", "suffix", "_o755"),
        ("0_xFF", "suffix", "_xFF"),
        ("0b1_2", "invalid digit '2'", "2"),
        ("0o7_8", "invalid digit '8'", "8"),
        ("0xF_G", "suffix", "_G"),
        ("42u8", "suffix", "u8"),
        ("42int64", "suffix", "int64"),
        ("42i64", "suffix", "i64"),
        ("42f32", "suffix", "f32"),
        ("42n", "suffix", "n"),
        ("42_name", "suffix", "_name"),
        ("0b10u8", "suffix", "u8"),
        ("0o755int64", "suffix", "int64"),
        ("0xFFu8", "suffix", "u8"),
        ("1_000u64", "suffix", "u64"),
        ("4２", "suffix", "２"),
    ];
    for (text, message, span) in cases {
        let (actual, actual_span) = malformed(text);
        assert!(actual.contains(message), "{text}: {actual}");
        assert_eq!(actual_span, span, "{text}");
    }
}

#[test]
fn decimal_floats_and_exponents() {
    for text in [
        "0.5",
        "1.0",
        "1e6",
        "1.5e-3",
        "2E+8",
        "1e0",
        "1.0e0",
        "1.0E+0",
        "1.0e-0",
        "01.50",
        "00e2",
        "1_000.25",
        "1_000.2_5",
        "1_0e+0_2",
        "1_000.2_5e1_0",
        "1e-3",
        "1e3",
        "1E+3",
    ] {
        assert_eq!(single(text), TokenKind::Float, "{text}");
    }
    assert_eq!(single("1"), TokenKind::Int(IntBase::Decimal));

    // A dot joins a float only when a digit follows.
    let case = Case::new("1.field");
    case.assert_clean();
    assert_eq!(
        case.kinds()[..3],
        [TokenKind::Int(IntBase::Decimal), punct(Punct::Dot), ident()]
    );
    for (text, expected) in [
        (
            ".5",
            vec![punct(Punct::Dot), TokenKind::Int(IntBase::Decimal)],
        ),
        (
            "1.",
            vec![TokenKind::Int(IntBase::Decimal), punct(Punct::Dot)],
        ),
        (
            "1.e2",
            vec![TokenKind::Int(IntBase::Decimal), punct(Punct::Dot), ident()],
        ),
        (
            "0b1.1",
            vec![TokenKind::Int(IntBase::Binary), punct(Punct::Dot)],
        ),
    ] {
        let case = Case::new(text);
        assert_eq!(case.kinds()[..expected.len()], expected, "{text}");
    }
}

#[test]
fn malformed_floats_are_rejected() {
    let cases = [
        ("1e", "expected exponent digits", "e"),
        ("1E", "expected exponent digits", "E"),
        ("1e+", "expected exponent digits", "e"),
        ("1.0e-", "expected exponent digits", "e"),
        ("1e++2", "expected exponent digits", "e"),
        ("1e+-2", "expected exponent digits", "e"),
        ("1_.0", "separator", "_"),
        ("1.0_", "separator", "_"),
        ("1__0.0", "separator", "_"),
        ("1_e2", "separator", "_"),
        ("1e_2", "separator", "_"),
        ("1e+_2", "separator", "_"),
        ("1e2_", "separator", "_"),
        ("1e2__0", "separator", "_"),
        ("1.0_e2", "separator", "_"),
        ("1.5f32", "suffix", "f32"),
        ("1.5float32", "suffix", "float32"),
        ("1e3f64", "suffix", "f64"),
        ("1.0i", "suffix", "i"),
        ("1.5_name", "suffix", "_name"),
        ("1_000.25f64", "suffix", "f64"),
    ];
    for (text, message, span) in cases {
        let case = Case::new(text);
        let errors = case.errors();
        assert!(!errors.is_empty(), "{text} was accepted");
        assert!(errors[0].0.contains(message), "{text}: {errors:?}");
        assert_eq!(errors[0].1, span, "{text}");
        assert_eq!(case.kinds()[0], TokenKind::MalformedLiteral, "{text}");
    }
    // `1._0` splits at the dot; the member access is rejected by later stages.
    let case = Case::new("1._0");
    assert_eq!(case.tokens()[2], (ident(), "_0"));
    // Non-decimal float spellings are never accepted.
    assert!(!Case::new("0x1.0p2").errors().is_empty());
}

#[test]
fn operators_use_maximal_munch() {
    let text = "<<= >>= << >> <= >= == != && || += -= *= /= %= &= |= ^= \
                + - * / % & | ^ ! < > = ? . , : ( ) [ ] { }";
    let case = Case::new(text);
    case.assert_clean();
    let expected: Vec<TokenKind> = Punct::ALL.iter().map(|&p| punct(p)).collect();
    assert_eq!(case.kinds()[..expected.len()], expected);

    let case = Case::new("a&^b");
    case.assert_clean();
    assert_eq!(case.kinds()[1..3], [punct(Punct::Amp), punct(Punct::Caret)]);
    let case = Case::new("x:=1");
    case.assert_clean();
    assert_eq!(case.kinds()[1..3], [punct(Punct::Colon), punct(Punct::Eq)]);
    let case = Case::new("- -x");
    case.assert_clean();
    assert_eq!(
        case.kinds()[..2],
        [punct(Punct::Minus), punct(Punct::Minus)]
    );
}

#[test]
fn increment_and_decrement_are_rejected() {
    for (text, spelling) in [("x++", "++"), ("--x", "--"), ("a--b", "--"), ("++x", "++")] {
        let case = Case::new(text);
        let errors = case.errors();
        assert_eq!(errors.len(), 1, "{text}");
        assert!(errors[0].0.contains("not a Zore operator"), "{errors:?}");
        assert_eq!(errors[0].1, spelling);
    }
}

#[test]
fn unexpected_characters_recover_and_continue() {
    let case = Case::new("a @ b # c \u{00A0} d \\ e");
    let names: Vec<&str> = case
        .tokens()
        .into_iter()
        .filter(|(kind, _)| *kind == ident())
        .map(|(_, text)| text)
        .collect();
    assert_eq!(names, ["a", "b", "c", "d", "e"]);
    let spans: Vec<&str> = case.errors().into_iter().map(|(_, span)| span).collect();
    assert_eq!(spans, ["@", "#", "\u{00A0}", "\\"]);
}

#[test]
fn unspecified_whitespace_is_rejected_and_lone_cr_is_string_content() {
    for (text, bad) in [
        ("a\u{000C}b", "\u{000C}"),
        ("a\u{000B}b", "\u{000B}"),
        ("\u{FEFF}a", "\u{FEFF}"),
    ] {
        let case = Case::new(text);
        let spans: Vec<&str> = case.errors().into_iter().map(|(_, span)| span).collect();
        assert_eq!(spans, [bad], "{text:?}");
    }
    assert_eq!(string_value("\"a\rb\""), "a\rb");
    assert_eq!(rune_value("'\r'"), '\r');
}

#[test]
fn lexing_always_progresses_and_covers_the_input_in_order() {
    const PIECES: &[&str] = &[
        "a", "_", "é", "0", "0x", "1.", "e", "+", "-", "_", ".", "\"", "'", "`", "\\", "/", "*",
        "\n", "\r", " ", "u", "{", "}", "?", "@", "😀", ";",
    ];
    // Deterministic xorshift so failures are reproducible without dependencies.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    for _ in 0..2000 {
        let mut text = String::new();
        for _ in 0..(state % 24) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            text.push_str(PIECES[(state % PIECES.len() as u64) as usize]);
        }
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let case = Case::new(&text);
        let tokens = &case.lexed.tokens;
        assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof, "{text:?}");
        assert_eq!(
            tokens.iter().filter(|t| t.kind == TokenKind::Eof).count(),
            1,
            "{text:?}"
        );
        let mut previous_end = 0;
        for token in tokens {
            assert!(token.span.start() >= previous_end, "{text:?}: {tokens:?}");
            previous_end = token.span.end();
            let synthetic = matches!(
                token.kind,
                TokenKind::Eof
                    | TokenKind::Semicolon(Separator::Newline)
                    | TokenKind::Semicolon(Separator::Eof)
            );
            assert_eq!(token.span.is_empty(), synthetic, "{text:?}: {token:?}");
        }
        for window in tokens.windows(2) {
            let both_inserted = [&window[0].kind, &window[1].kind].iter().all(|kind| {
                matches!(
                    kind,
                    TokenKind::Semicolon(Separator::Newline | Separator::Eof)
                )
            });
            assert!(!both_inserted, "{text:?}: duplicate insertion");
        }
    }
}

#[test]
fn examples_lex_without_diagnostics() {
    for path in [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/hello/main.ore"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/semantic-target/main.ore"),
    ] {
        let mut sources = SourceMap::new();
        let id = sources.load(path).unwrap();
        let lexed = lex(sources.file(id).unwrap());
        assert!(
            lexed.diagnostics.is_empty(),
            "{path}: {:?}",
            lexed.diagnostics
        );
        assert!(lexed.tokens.len() > 1, "{path}");
    }
}
