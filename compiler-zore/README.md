# Zore compiler written in Zore

This folder holds stages 1 and 2 of the staged self-hosting plan in
`docs/proposals/self-hosting-plan.md`. Stage 1 is a lexer and stage 2 is a
parser, a source manager, and diagnostic records, all written in Zore and
checked against the Rust frontend. The Rust compiler remains the reference and
bootstrap compiler. Nothing here resolves names, type-checks, or compiles
programs, and this is not a self-compilation claim.

## Contents

- `lexer/` is the `lexer` package. `lexer.Lex(text)` takes source text that has
  already passed UTF-8 validation. It returns every token with its kind, byte
  span, and decoded literal text, plus the lexical errors with their byte spans.
- `lexer/unicode.ore` is a generated table of non-ASCII letters and digits, as
  Rust's `char::is_alphanumeric` defines them for the pinned toolchain.
- `source/` is the `source` package. A `source.Manager` holds files by number,
  with their path, text, and line starts. It answers line text and one-based
  line and column lookups.
- `diagnostic/` is the `diagnostic` package. It provides diagnostic records
  (severity, message, primary label, related labels, and notes) and
  `diagnostic.Render`, which prints them the way the Rust compiler does.
- `ast/` is the `ast` package: a syntax tree stored as one array of nodes.
  Each node has a tag, a byte span, one attribute, and child node numbers.
- `parser/` is the `parser` package. `parser.Parse(text, file, native)` runs the
  lexer and builds the tree. It reports the same syntax errors, and recovers
  from them, the same way as `compiler/src/parser`. With `native` set, function
  declarations may omit their bodies, as in the bundled standard packages.
- `oracle/` and `parseoracle/` are small programs for the comparison tests. Each
  reads `cases.bin` from the current folder and prints records for each case.
  They are not compiler drivers.

## Commands

Build the Rust compiler first, then run from the repository root:

```sh
cargo test --locked --test selfhost_lexer
cargo test --locked --test selfhost_parser
ZORE_LEXER_SEED=12345 cargo test --locked --test selfhost_lexer seeded
ZORE_PARSER_SEED=12345 cargo test --locked --test selfhost_parser seeded
ZORE_WRITE_UNICODE_TABLE=1 cargo test --locked --test selfhost_lexer unicode_table
```

Each test builds its Zore program with the Rust compiler and gives the same
bytes to both implementations, then compares their records. The seed variables
explore other generated inputs. The default seeds are fixed, so ordinary runs
are reproducible. The last command regenerates the Unicode table after a
toolchain upgrade; without the variable, that test fails if the table is out of
date.

A mismatch report names the case and its input, then shows the first record
that differs and both full record lists. Generated cases are named by seed and
index, so any reported case can be reproduced.

## Lexer records

Each token is a line of the form `<kind> <start> <end>`. The kind is one of the
following:

- `ident` or `underscore`
- `keyword:<spelling>` or `reserved:<spelling>`
- `int:<base>` or `float`
- `string:<UTF-8 bytes joined by .>` or `rune:<code point>`
- `malformed` or `unknown`
- `punct:<spelling>`
- `semi:explicit`, `semi:newline`, or `semi:eof`
- `eof`

Each error is a line `error:<code> <start> <end>`. The codes are fixed names
such as `unterminated-string` or `invalid-digit`. The test maps each Rust lexer
message to one of them, and an unmapped message fails the test. Offsets are
byte offsets. A case whose bytes are not UTF-8 gives the single record
`invalid-utf8`. That check happens before lexing: in Rust it is done by
`SourceMap::load`, and in Zore by `strings.FromBytes`.

## Parser and diagnostic records

A parsed case starts with `tree`, followed by the whole tree:

- A node prints as `(tag start end attribute children...)`.
- A list prints as `[...]`.
- A missing optional part prints as `_`.
- A node with no span of its own omits `start end`. These are an assignment,
  a `return`, `break`, or `continue` statement, a `for` loop and its header,
  and a function-type parameter.

Every node carries its exact byte span, and every string value is printed as its
UTF-8 bytes.

Next come the lexer errors as `lexerror:<code> <start> <end>`. Then each parser
diagnostic prints as `diag <start> <end> <message>`, followed by:

- its primary label message
- its related labels with their spans
- its notes
- every line of its rendered text

Parser messages, labels, notes, and rendered text are compared exactly.

A probe case checks the source manager. It records:

- the line count, and every line's bytes
- the line and column of every byte offset, or `-` where an offset splits a
  character
- the rendering of a diagnostic with labels at fixed positions in the text

## Coverage

The lexer comparison covers every token form of the Rust lexer:

- names, keywords, and comments
- semicolon insertion
- strings, raw strings, and runes
- numbers in every base
- operators and malformed input

The parser comparison covers every syntax form and error of `compiler/src/parser`,
including recovery:

- skipping to the next statement or declaration
- unclosed braces
- trailing commas
- a closing `>>` split in two
- semicolons inserted after a `>`
- the struct-literal retry in `for` headers

Inputs:

- **Lexer:** targeted cases; each line of `tests/lexer/lexer.rs`; the lexical
  conformance documents; every `.ore` file in the repository; and 4,000
  generated inputs, plus byte sequences for the UTF-8 check.
- **Parser:**
  - targeted programs and statements
  - every string literal in the parser, lexer, and source-diagnostic Rust tests,
    alone and inside a `main` body
  - every code span in the conformance documents
  - every code block in the specification
  - every `.ore` file in the repository; the standard packages are parsed with
    bodiless native declarations allowed
  - 4,000 generated token sequences
  - 2,000 generated operator expressions
  - 2,000 random edits of the example programs
- **Source manager:** targeted texts with tabs, CRLF, control characters,
  combining marks, and emoji; example programs; and 600 generated texts.

Deliberately broken versions of the Zore code have been run to check that the
tests catch them: changed operator precedence, recovery, `>>` splitting, CRLF
handling, the `for` retry, token descriptions, column counting, and tab
expansion.

Not compared:

- The display text of lexer diagnostics, which only appears in the Rust
  output. Rust formats characters in those messages with its `Debug` escaping
  rules, which this code does not reproduce. Lexer errors are compared by code
  and span, and the Rust lexer tests keep checking their text.

## What writing it showed

- There are no enums, so token kinds and tree node kinds are integer constants
  or string tags.
- `rune` does not convert to `int`, so the lexer decodes UTF-8 into integer
  code points itself. It encodes decoded literals back into a `string` through
  `strings.FromBytes`.
- There are no package-level values, so each `Lex` call rebuilds the Unicode
  table.
- A call that borrows a value as `mut` cannot also read one of its fields as an
  argument, as in `l.report(code, start, l.pos)`. The same applies to a nested
  call on the same value, as in `p.append(list, p.node(...))`. The checker
  rejects the overlap, and the specification leaves exactly when borrows start
  open. The code copies the value into a local first.
- An element cannot be moved out of an array by index. Tokens are therefore a
  Copy struct whose decoded text is a `string`. That lets the parser insert a
  token by shifting elements.
- Comma-separated lists pick their item parser from a small integer code. The
  alternative is passing a closure that would itself need the parser as `mut`
  while the list also holds it.
