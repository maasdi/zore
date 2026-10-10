//! Compares the Zore lexer in `compiler-zore/lexer` with the Rust lexer, record by record.
//! Requires clang (LLVM 15+) and rustc 1.98+; a missing toolchain fails rather than skips.
//! Set `ZORE_LEXER_SEED` to explore other generated inputs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use zore::build::{TempDir, build_project};
use zore::driver::project::{Disk, LoadError, load_project};
use zore::lexer::{IntBase, Separator, TokenKind, lex};
use zore::source::{SourceError, SourceMap};

const DEFAULT_SEED: u64 = 0x5EED_2026_1009;
const GENERATED_CASES: usize = 4000;
const GENERATED_BYTE_CASES: usize = 400;

struct Case {
    label: String,
    bytes: Vec<u8>,
}

impl Case {
    fn text(label: impl Into<String>, text: &str) -> Self {
        Self {
            label: label.into(),
            bytes: text.as_bytes().to_vec(),
        }
    }
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the compiler crate is inside the repository")
        .to_path_buf()
}

struct Oracle {
    _dir: TempDir,
    executable: PathBuf,
}

fn oracle() -> &'static Path {
    static ORACLE: OnceLock<Oracle> = OnceLock::new();
    &ORACLE
        .get_or_init(|| {
            let entry = repository().join("compiler-zore/oracle/main.ore");
            let mut sources = SourceMap::new();
            let project = match load_project(&mut sources, &Disk, &entry) {
                Ok(project) => project,
                Err(LoadError::Source(error)) => panic!("cannot load the Zore lexer: {error}"),
                Err(LoadError::Diagnostics(diagnostics)) => panic!(
                    "the Zore lexer does not load:\n{}",
                    diagnostics
                        .iter()
                        .map(|d| d.render(&sources).unwrap_or_else(|_| d.message().into()))
                        .collect::<String>()
                ),
            };
            let dir = TempDir::new().unwrap();
            let executable = dir.path().join("lexer-oracle");
            if let Err(error) = build_project(&project, &sources, &executable) {
                panic!("the Zore lexer does not build: {error:?}");
            }
            Oracle {
                _dir: dir,
                executable,
            }
        })
        .executable
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
        TokenKind::String(value) => format!(
            "string:{}",
            value
                .chars()
                .map(|c| (c as u32).to_string())
                .collect::<Vec<_>>()
                .join(".")
        ),
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

fn zore_records(dir: &Path, cases: &[Case]) -> Vec<Vec<String>> {
    let mut input = Vec::new();
    for case in cases {
        input.extend_from_slice(format!("{}\n", case.bytes.len()).as_bytes());
        input.extend_from_slice(&case.bytes);
    }
    fs::write(dir.join("cases.bin"), input).unwrap();
    let output = Command::new(oracle())
        .current_dir(dir)
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .expect("run the Zore lexer");
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "the Zore lexer failed ({}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("records are text");
    let mut blocks = vec![Vec::new()];
    for line in stdout.lines() {
        if line == "done" {
            blocks.push(Vec::new());
        } else {
            blocks.last_mut().unwrap().push(line.to_string());
        }
    }
    assert!(
        blocks.pop().is_some_and(|rest| rest.is_empty()),
        "output does not end with `done`"
    );
    assert_eq!(blocks.len(), cases.len(), "one record block per case");
    blocks
}

/// Fails with every case whose records differ, showing the first differing record.
fn compare(cases: &[Case]) {
    assert!(!cases.is_empty());
    let dir = TempDir::new().unwrap();
    let actual = zore_records(dir.path(), cases);
    let mut mismatches = Vec::new();
    for (index, (case, zore)) in cases.iter().zip(&actual).enumerate() {
        let rust = rust_records(dir.path(), index, &case.bytes);
        if &rust == zore {
            continue;
        }
        let at = rust
            .iter()
            .zip(zore)
            .position(|(r, z)| r != z)
            .unwrap_or(rust.len().min(zore.len()));
        mismatches.push(format!(
            "{}\n  input: {:?}\n  record {at}: rust {:?}, zore {:?}\n  rust: {rust:?}\n  zore: {zore:?}",
            case.label,
            String::from_utf8_lossy(&case.bytes),
            rust.get(at),
            zore.get(at),
        ));
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ:\n{}",
        mismatches.len(),
        cases.len(),
        mismatches
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
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
        .map(|(index, text)| Case::text(format!("targeted case {index}"), text))
        .collect();
    compare(&cases);
}

#[test]
fn rust_lexer_test_lines_match() {
    let path = repository().join("tests/lexer/lexer.rs");
    let text = fs::read_to_string(&path).unwrap();
    let mut cases = vec![Case::text("tests/lexer/lexer.rs (whole file)", &text)];
    for (number, line) in text.lines().enumerate() {
        cases.push(Case::text(
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
        cases.push(Case::text(format!("{file} (whole file)"), &text));
        let mut block: Option<(usize, String)> = None;
        for (number, line) in text.lines().enumerate() {
            let number = number + 1;
            cases.push(Case::text(format!("{file} line {number}"), line));
            for (index, span) in line.split('`').enumerate() {
                if index % 2 == 1 && !span.is_empty() {
                    cases.push(Case::text(format!("{file} line {number} code span"), span));
                }
            }
            if line.trim_start().starts_with("```") {
                match block.take() {
                    Some((start, body)) => {
                        cases.push(Case::text(format!("{file} block at line {start}"), &body));
                    }
                    None => block = Some((number, String::new())),
                }
            } else if let Some((_, body)) = &mut block {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    compare(&cases);
}

fn ore_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            ore_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "ore") {
            found.push(path);
        }
    }
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
            bytes: fs::read(path).unwrap(),
        })
        .collect();
    compare(&cases);
}

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }
}

fn seed() -> u64 {
    match std::env::var("ZORE_LEXER_SEED") {
        Ok(text) => text.parse().expect("ZORE_LEXER_SEED is a decimal number"),
        Err(_) => DEFAULT_SEED,
    }
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
            Case::text(format!("seed {seed} generated case {index}"), &text)
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
