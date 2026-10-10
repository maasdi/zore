//! Compares the Zore lexer in `compiler-zore/lexer` with the Rust lexer, record by record.
//! Requires clang (LLVM 15+) and rustc 1.98+; a missing toolchain fails rather than skips.
//! Set `ZORE_LEXER_SEED` to explore other generated inputs.

#[path = "common.rs"]
mod common;

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use common::{
    Case, Generator, Program, build_program, byte_list, code_blocks, ore_files, repository,
};
use zore::lexer::{IntBase, Separator, TokenKind, lex};
use zore::source::{SourceError, SourceMap};

const GENERATED_CASES: usize = 4000;
const GENERATED_BYTE_CASES: usize = 400;
const LEX: u8 = b'l';

fn oracle() -> &'static Path {
    static ORACLE: OnceLock<Program> = OnceLock::new();
    &ORACLE
        .get_or_init(|| build_program("compiler-zore/oracle/main.ore"))
        .executable
}

fn compare(cases: &[Case]) {
    common::compare(oracle(), cases, |dir, index, case| {
        rust_records(dir, index, &case.bytes)
    });
}

fn seed() -> u64 {
    common::seed("ZORE_LEXER_SEED")
}

fn lex_case(label: impl Into<String>, text: &str) -> Case {
    Case::text(label, LEX, text)
}

/// The Rust records for one case, read through the compiler's own source loading.
fn rust_records(dir: &Path, index: usize, bytes: &[u8]) -> Vec<String> {
    let path = dir.join(format!("case-{index}.ore"));
    fs::write(&path, bytes).unwrap();
    let mut sources = SourceMap::new();
    let id = match sources.load(&path) {
        Ok(id) => id,
        Err(SourceError::InvalidUtf8 { .. }) => return vec!["invalid-utf8".into()],
        Err(error) => panic!("cannot load case {index}: {error}"),
    };
    let file = sources.file(id).unwrap();
    let lexed = lex(file);
    let mut records = Vec::new();
    for token in &lexed.tokens {
        let span = token.span;
        let spelling = sources.slice(span).unwrap();
        records.push(format!(
            "{} {} {}",
            token_record(&token.kind, spelling),
            span.start(),
            span.end()
        ));
    }
    for diagnostic in &lexed.diagnostics {
        let span = diagnostic.span();
        records.push(format!(
            "error:{} {} {}",
            error_code(diagnostic.message()),
            span.start(),
            span.end()
        ));
    }
    records
}

fn token_record(kind: &TokenKind, spelling: &str) -> String {
    match kind {
        TokenKind::Ident => "ident".into(),
        TokenKind::Underscore => "underscore".into(),
        TokenKind::Keyword(_) => format!("keyword:{spelling}"),
        TokenKind::Reserved(_) => format!("reserved:{spelling}"),
        TokenKind::Int(base) => format!(
            "int:{}",
            match base {
                IntBase::Binary => 2,
                IntBase::Octal => 8,
                IntBase::Decimal => 10,
                IntBase::Hexadecimal => 16,
            }
        ),
        TokenKind::Float => "float".into(),
        TokenKind::String(value) => format!("string:{}", byte_list(value)),
        TokenKind::Rune(value) => format!("rune:{}", *value as u32),
        TokenKind::MalformedLiteral => "malformed".into(),
        TokenKind::Unknown => "unknown".into(),
        TokenKind::Punct(_) => format!("punct:{spelling}"),
        TokenKind::Semicolon(separator) => format!(
            "semi:{}",
            match separator {
                Separator::Explicit => "explicit",
                Separator::Newline => "newline",
                Separator::Eof => "eof",
            }
        ),
        TokenKind::Eof => "eof".into(),
    }
}

/// A stable code for each lexer message; display text is compared by the Rust lexer tests.
fn error_code(message: &str) -> &'static str {
    let starts = |prefix: &str| message.starts_with(prefix);
    if message == "unterminated block comment" {
        "unterminated-block-comment"
    } else if starts("identifier `") && message.contains("contains non-ASCII character") {
        "non-ascii-identifier"
    } else if starts("invalid digit ") {
        "invalid-digit"
    } else if starts("invalid suffix ") {
        "suffix"
    } else if message == "digit separator must be between two digits" {
        "separator"
    } else if starts("expected fractional digits") {
        "expected-fraction-digits"
    } else if starts("expected exponent digits") {
        "expected-exponent-digits"
    } else if starts("expected ") && message.contains(" digits after `") {
        "expected-prefixed-digits"
    } else if starts("expected digits") {
        "expected-digits"
    } else if message == "unterminated string literal" {
        "unterminated-string"
    } else if message == "unterminated rune literal" {
        "unterminated-rune"
    } else if message == "unterminated raw string literal" {
        "unterminated-raw-string"
    } else if starts("unknown escape sequence") {
        "unknown-escape"
    } else if message.contains("escape requires exactly") {
        "escape-digits"
    } else if message.ends_with("is not a Unicode scalar value") {
        "not-scalar"
    } else if message == "empty rune literal" {
        "empty-rune"
    } else if message == "rune literal contains more than one character" {
        "long-rune"
    } else if message.ends_with("is not a Zore operator") {
        "increment-decrement"
    } else if starts("unexpected character") {
        "unexpected-character"
    } else {
        panic!("no comparison code for lexer diagnostic `{message}`")
    }
}

#[test]
fn empty_input_whitespace_and_targeted_cases_match() {
    let inputs = [
        "",
        " ",
        "\t\r ",
        "\n",
        "\r\n",
        "x\r\ny",
        "x\r",
        "// only a comment",
        "x // comment\ny",
        "/* block */",
        "x /* one\nline */ y",
        "x /* same line */ y\n",
        "return /* a\r\nb */",
        "/* unterminated",
        "x /* unterminated\n",
        "/**/",
        "/*/",
        "break\ncontinue\nreturn\ntrue\nfalse\nnil\n",
        "f()\na[1]\n{}\nx?\n",
        "if x {\n} else {\n}\n",
        "let a = 1; let b = 2;",
        "_ _a a_ __",
        "interface trait impl enum match unsafe macro defer",
        "café := 1",
        "x\u{301}y",
        "名前",
        "a١b ١",
        "Ⅻ ² ß",
        "\u{feff}x",
        "\u{a0}\u{2028}\u{c}",
        "😀",
        "\"\"",
        "\"plain\"",
        "\"é😀\\n\\r\\t\\\\\\\"\"",
        "\"\\u00e9\\U0001F600\"",
        "\"\\uD800\" \"\\U00110000\" \"\\u12\" \"\\U1234567\"",
        "\"\\x41\" \"\\'\" \"\\a\"",
        "\"unterminated",
        "\"line\nbreak\"",
        "\"crlf\r\nbreak\"",
        "\"lone\rcr\"",
        "\"continued\\\nline\"",
        "\"continued\\\r\nline\"",
        "\"ends with backslash\\",
        "''",
        "'a'",
        "'é'",
        "'😀'",
        "'ab'",
        "'\\''",
        "'\\\"'",
        "'\\u0041'",
        "'x\n'",
        "'",
        "`raw`",
        "`multi\nline \\n raw`",
        "``",
        "`unterminated raw",
        "0 7 42 1_000 0b1010 0o17 0x1F 0X_FF 0B1 0O7",
        "0b 0o 0x 0b2 0o8 0xG 0b1_2",
        "1__0 1_ _1 1_e5 1_x 0x_1 0x1_",
        "1.5 1.5e10 2e-3 3E+4 1e 1e+ 1e- 1.e5 1. .5 1..2",
        "1.5.6 1e5.5 1.5_5 1_000.000_1",
        "12abc 0x1p3 1é 1٣ 1Ⅻ",
        "123456789012345678901234567890",
        "<<= >>= << >> <= >= == != && || += -= *= /= %= &= |= ^= ! ? . , : ( ) [ ] { }",
        "x++ y-- a+-b a- -b",
        "# @ $ ~ \\",
        "\u{0}",
        "x\u{0}y",
    ];
    let cases: Vec<Case> = inputs
        .iter()
        .enumerate()
        .map(|(index, text)| lex_case(format!("targeted case {index}"), text))
        .collect();
    compare(&cases);
}

#[test]
fn rust_lexer_test_lines_match() {
    let path = repository().join("tests/lexer/lexer.rs");
    let text = fs::read_to_string(&path).unwrap();
    let mut cases = vec![lex_case("tests/lexer/lexer.rs (whole file)", &text)];
    for (number, line) in text.lines().enumerate() {
        cases.push(lex_case(
            format!("tests/lexer/lexer.rs line {}", number + 1),
            line,
        ));
    }
    compare(&cases);
}

#[test]
fn lexical_conformance_documents_match() {
    let mut cases = Vec::new();
    for name in [
        "comments",
        "strings",
        "runes",
        "integers",
        "floats",
        "identifiers",
        "keywords",
        "statement-boundaries",
    ] {
        let file = format!("tests/conformance/{name}.md");
        let text = fs::read_to_string(repository().join(&file)).unwrap();
        cases.push(lex_case(format!("{file} (whole file)"), &text));
        for (number, line) in text.lines().enumerate() {
            let number = number + 1;
            cases.push(lex_case(format!("{file} line {number}"), line));
            for (index, span) in line.split('`').enumerate() {
                if index % 2 == 1 && !span.is_empty() {
                    cases.push(lex_case(format!("{file} line {number} code span"), span));
                }
            }
        }
        for (start, body) in code_blocks(&text) {
            cases.push(lex_case(format!("{file} block at line {start}"), &body));
        }
    }
    compare(&cases);
}

#[test]
fn example_standard_and_self_hosted_sources_match() {
    let root = repository();
    let mut paths = Vec::new();
    for dir in ["examples", "std", "compiler-zore", "benchmarks"] {
        ore_files(&root.join(dir), &mut paths);
    }
    assert!(paths.len() > 30, "found only {} source files", paths.len());
    let cases: Vec<Case> = paths
        .iter()
        .map(|path| Case {
            label: path.strip_prefix(&root).unwrap().display().to_string(),
            mode: LEX,
            bytes: fs::read(path).unwrap(),
        })
        .collect();
    compare(&cases);
}

#[rustfmt::skip]
const FRAGMENTS: &[&str] = &[
    " ", "  ", "\t", "\n", "\n", "\r\n", "\r", "a", "x1", "_", "_a", "func", "return", "break",
    "nil", "true", "enum", "defer", "0", "1", "7", "9", "0x", "0X1f", "0b", "0b10", "0o", "0o17",
    "1_000", "__", "1e", "e", "E", "e+", "e-", "+", "-", ".", "..", "5.", ".5", "1.5", "2e10",
    "3.0e-2", "\"", "\"", "'", "'", "`", "\\", "\\n", "\\t", "\\u", "\\U", "\\u00e9",
    "\\U0001F600", "\\uD800", "\\U00110000", "\\x", "\\'", "\\\"", "/", "//", "/*", "*/", "*",
    "(", ")", "[", "]", "{", "}", "?", ";", ",", ":", "<<=", ">>", "&&", "||", "!=", "==", "++",
    "--", "+=", "é", "café", "名", "\u{301}", "😀", "\u{a0}", "\u{feff}", "١", "Ⅻ", "²", "ß",
    "\u{2028}", "#", "@", "$", "~", "abc", "f", "g", "z9", "A", "F", "p",
];

#[test]
fn seeded_generated_inputs_match() {
    let seed = seed();
    let mut generator = Generator(seed | 1);
    let cases: Vec<Case> = (0..GENERATED_CASES)
        .map(|index| {
            let count = 1 + generator.below(24);
            let text: String = (0..count)
                .map(|_| FRAGMENTS[generator.below(FRAGMENTS.len())])
                .collect();
            lex_case(format!("seed {seed} generated case {index}"), &text)
        })
        .collect();
    compare(&cases);
}

#[test]
fn invalid_utf8_is_rejected_before_lexing() {
    let fixed: [&[u8]; 9] = [
        b"\xff",
        b"x\xc3",
        b"\xc3x",
        b"\xc0\x80",
        b"\xed\xa0\x80",
        b"\xf4\x90\x80\x80",
        b"\"\x80\"",
        b"// \xfe\n",
        b"ok \xe2\x82",
    ];
    let mut cases: Vec<Case> = fixed
        .iter()
        .enumerate()
        .map(|(index, bytes)| Case {
            label: format!("invalid byte case {index}"),
            mode: LEX,
            bytes: bytes.to_vec(),
        })
        .collect();
    let seed = seed();
    let mut generator = Generator(seed.rotate_left(17) | 1);
    let pool: &[u8] = b"a1_\"'`\\/*\n \x80\xbf\xc2\xc3\xe2\xed\xf0\xf4\xff";
    for index in 0..GENERATED_BYTE_CASES {
        let length = generator.below(10);
        let bytes = (0..length)
            .map(|_| pool[generator.below(pool.len())])
            .collect();
        cases.push(Case {
            label: format!("seed {seed} generated byte case {index}"),
            mode: LEX,
            bytes,
        });
    }
    let invalid = cases
        .iter()
        .filter(|case| std::str::from_utf8(&case.bytes).is_err())
        .count();
    assert!(
        invalid >= fixed.len() + 100,
        "only {invalid} invalid inputs"
    );
    compare(&cases);
}

fn alphanumeric_ranges() -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();
    let mut start = None;
    for code in 0x80..=0x11_0000u32 {
        let alphanumeric = char::from_u32(code).is_some_and(char::is_alphanumeric);
        match (alphanumeric, start) {
            (true, None) => start = Some(code),
            (false, Some(first)) => {
                ranges.push((first, code - 1));
                start = None;
            }
            _ => {}
        }
    }
    ranges
}

fn alphanumeric_table_source() -> String {
    let mut source = String::from(
        "package lexer\n\n\
         // Generated from Rust's `char::is_alphanumeric` above ASCII as inclusive ranges.\n\
         // The selfhost_lexer test checks it; ZORE_WRITE_UNICODE_TABLE=1 rewrites it.\n\
         func alphanumericRanges() Array<int> {\n    return Array<int>{\n",
    );
    for row in alphanumeric_ranges().chunks(4) {
        let values: Vec<String> = row
            .iter()
            .flat_map(|(first, last)| [format!("0x{first:X}"), format!("0x{last:X}")])
            .collect();
        source.push_str(&format!("        {},\n", values.join(", ")));
    }
    source.push_str("    }\n}\n");
    source
}

#[test]
fn unicode_table_matches_the_rust_lexer() {
    let path = repository().join("compiler-zore/lexer/unicode.ore");
    let expected = alphanumeric_table_source();
    if std::env::var_os("ZORE_WRITE_UNICODE_TABLE").is_some() {
        fs::write(&path, &expected).unwrap();
    }
    let actual = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "{} is out of date; rerun with ZORE_WRITE_UNICODE_TABLE=1",
        path.display()
    );
}
