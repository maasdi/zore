# Zore Language Specification

**Document status:** Locked MVP language specification\
**Audience:** Compiler implementers, coding agents, tooling authors\
**Language:** Zore\
**Source extension:** `.ore`\
**Compiler / CLI:** `zore`\
**Project configuration:** `zore.toml`\
**Tagline:** **Simple code. Strong guarantees.**

---

## 0. How to Use This Specification

This document is the authoritative implementation guide for the currently locked Zore MVP decisions.

A coding agent or compiler implementer MUST follow these rules:

1. **Do not invent syntax or semantics that are not explicitly specified here.**
2. A rule marked **LOCKED** is part of the Zore MVP language contract.
3. A rule marked **TBD** is intentionally unresolved. Implementations should preserve room for it rather than silently choosing a permanent design.
4. A feature listed under **OUT OF MVP** must not be added to the MVP unless the language specification is explicitly revised.
5. Compiler-internal implementation details may evolve as long as observable language semantics remain compatible with this specification.
6. When examples conflict with a normative rule, the normative rule wins.
7. Zore should not be described as “Rust but easier” or “Go but safer.” Go and Rust may be used as explanatory comparisons only.

The project should favor the smallest coherent implementation that satisfies the locked semantics.

---

# 1. Language Goals

## 1.1 Core goals — LOCKED

Zore is a native programming language designed around:

- simple, readable syntax
- native compilation
- predictable performance
- no garbage collector
- ownership-based memory safety
- borrowing by default
- deterministic resource cleanup
- explicit errors
- first-class `async` / `await`
- lightweight tasks
- channel-based message passing
- compiler-inferred lifetimes
- safe concurrency using the same ownership model as synchronous code
- eventual compiler self-hosting

The source language should remain relatively simple even when the compiler performs sophisticated ownership, lifetime, async, drop, and data-flow analysis.

## 1.2 Fundamental language principle — LOCKED

> **Async, concurrency, and resource management use the same ownership model.**

Zore must not introduce a separate memory-safety model for asynchronous or concurrent code.

The following concepts retain the same meaning everywhere:

- `let`
- `var`
- `mut`
- `own`
- Copy
- Move
- Borrow
- Lifetime
- Drop

## 1.3 Language personality — LOCKED

Zore should be:

- minimal
- precise
- modern
- fast
- calm
- explicit where safety or ownership matters
- concise where the compiler can safely infer details

The language should avoid source-level complexity that exists only to expose compiler machinery.

---

# 2. Language Identity

## 2.1 Naming — LOCKED

| Item | Value |
|---|---|
| Language name | `Zore` |
| Source extension | `.ore` |
| Compiler executable | `zore` |
| Project configuration | `zore.toml` |
| Tagline | `Simple code. Strong guarantees.` |

The official spelling is `Zore`.

Do not use:

- `Soré`
- `Sore`
- `.zor`

## 2.2 Compilation model — LOCKED

Zore is intended to compile to native machine code.

The initial compiler implementation is expected to use LLVM as its backend.

The initial compiler may be written in Rust.

Long term, Zore should be capable of compiling a compiler written in Zore itself.

Self-hosting is a project goal, but it is not required for the first MVP compiler.

---

# 3. Program Structure

## 3.1 Source files — LOCKED

Zore source files use the `.ore` extension.

Example:

```text
hello/
├── zore.toml
├── main.ore
└── user.ore
```

## 3.2 Packages — LOCKED

A source file declares its package using:

```ore
package main
```

Files in the same package share declarations according to package visibility rules.
Which files form a package, and how packages are found, is specified in §3.20.

Example:

```ore
package main

func main() {
    println("Hello, Zore!")
}
```

## 3.3 Imports — LOCKED

Import syntax is Go-like:

```ore
import "zore/strings"
```

The meaning of an import path, and how imported names are used, is specified in
§3.20. The full package-resolution, registry, version-selection, and
dependency-solver model is outside the MVP.

## 3.4 Project file — LOCKED

The minimal project configuration uses `zore.toml`.

Example:

```toml
name = "hello"
version = "0.1.0"
```

Additional fields may be added later, but the MVP must not require a package registry or sophisticated dependency solver.

## 3.5 Identifiers — LOCKED

MVP identifiers use ASCII characters only:

```text
identifier_start    = "A" … "Z" | "a" … "z" | "_" .
identifier_continue = identifier_start | "0" … "9" .
identifier          = identifier_start { identifier_continue } .
```

Identifiers are case-sensitive: `user` and `User` are distinct names.
Keywords cannot be used as identifiers; the reservation policy is defined in
§3.15 and the reserved word lists are defined in §3.16–3.17.
The standalone `_` is reserved and cannot name a declaration or be read as a
value. It may appear as a discard target in the contexts specified in §5.5;
such a target does not declare a name.

Valid names include `user`, `user2`, `user_name`, `User`, and `_internal`.
`2user`, `café`, and `用户` are not valid identifiers.

This restriction applies to identifiers, not to string or comment contents.
Unicode text is permitted in strings and comments; their delimiters and escape
rules are specified separately and remain TBD where not otherwise locked.

Ownership, error, and async implications: identifier spelling does not change
ownership contracts, Copy/Move classification, cleanup, error propagation, or
task/async behavior.

Compiler impact: recognize identifier characters using the ASCII ranges above,
retain source byte spans, and diagnose non-ASCII characters used in names.
No Unicode normalization or Unicode character tables are needed for identifier
recognition. Resolution must distinguish case and use semantic IDs as in §27.
Conformance cases are recorded in `tests/conformance/identifiers.md`; execution
is pending the lexer, parser, and resolver milestones.

## 3.6 Comments — LOCKED

Zore supports two comment forms:

- A line comment starts with `//` and continues to the end of the line or EOF.
- A block comment starts with `/*` and ends at the first following `*/`.
  Block comments may span multiple lines and do not nest. EOF before the closing
  delimiter is a compile-time error.

```ore
// A line comment

/* A block comment
   spanning two lines */

let count = 1 /* an inline comment */
```

Comment delimiters inside string literals are literal contents, not comments.
Inside a comment, quote characters and other opening comment delimiters do not
start a string or nested comment. For example, `/* outer /* inner */` is one
complete block comment, ending at its first `*/`.

Comments separate tokens: `user/* note */Name` must not become the identifier
`userName`. Unicode text is allowed in comment contents. Newlines in or after
comments participate in statement termination as specified in §3.7;
implementations must retain that information.

Ownership, error, and async implications: comments introduce no executable
operations and do not change ownership, cleanup, error propagation, or async
behavior. Unterminated block comments are lexical errors, not runtime errors.

Compiler impact: the lexer must recognize both forms without nesting, track
source positions and newlines while skipping comments, and produce a source-aware
diagnostic for an unterminated block comment identifying its opening delimiter.
Conformance cases are recorded in `tests/conformance/comments.md`; execution is
pending the lexer milestone.

## 3.7 Statement boundaries and semicolons — LOCKED

Zore uses automatic semicolon insertion. Explicit `;` may separate statements
on the same line. Most source code should omit semicolons at line endings.

At a newline or EOF, insert a semicolon if the last significant token is:

- an identifier,
- a literal of a supported literal form,
- the keywords `break`, `continue`, `return`, `true`, `false`, or `nil`,
- one of `)`, `]`, `}`, or the postfix error-propagation operator `?`, or
- the `>` that closes a type argument list, such as the final `>` of
  `Array<int>` (locked by Q17c).

A closing type-argument `>` ends a type exactly where an identifier type name
would, so it is eligible for the same reason the identifier is. The `>`
comparison operator is never eligible, so a comparison may still continue after
a line-ending `>`. When one source token holds several closing `>`, as in
`Array<Array<int>>`, only the last one can end the line. Because only the
grammar distinguishes a closing type-argument `>` from the comparison operator,
this one case is recognized while parsing, not while lexing (see Compiler
impact below). Its newline, comment, and EOF behavior is the same as every
other eligible token's.

```ore
type Bag struct {
    Items Array<int>      // the field ends after the closing `>`
    Count int
}

let more = count >
    limit                 // comparison: no insertion after `>`
```

Comments and horizontal whitespace are not significant tokens. An explicit or
inserted semicolon is not eligible for another insertion, so blank lines,
consecutive comments, and EOF after a terminating newline do not produce extra
semicolons. No semicolon is inserted into an empty or comment-only file.

For these rules, newline means LF (U+000A). CRLF behaves as one newline; CR
(U+000D) is whitespace and does not independently terminate a statement.
Newlines contained within a literal token do not trigger insertion.

Keyword eligibility is defined in §3.17; it does not settle the remaining grammar
or typing of those constructs. Future keyword additions must explicitly specify
insertion behavior. Do not inherit Go's `fallthrough`, `++`, or `--` from this rule.

A block comment without a newline acts as a space. A block comment containing
one or more newlines acts as a newline for insertion. A line comment leaves its
terminating newline (or EOF) effective. Consequently, a newline inside a block
comment can terminate the preceding statement even if code follows its closing
delimiter on the same physical line.

Insertion is lexical: parentheses, brackets, and braces do not suppress it.
Break a continued expression after an operator or comma, not after an eligible
ending token. There is no backslash line-continuation syntax.

```ore
let user = loadUser()
greet(user)

let total = first +
    second

let content = read(file)? // the statement ends after ?

let a = 1; let b = 2
```

By contrast, `let total = first` followed by a newline then `+ second` does not
form one continued initializer: insertion terminates the first line.

A semicolon required at the end of a statement or declaration may be omitted
immediately before `)` or `}`. This does not allow semicolons to replace commas
in argument lists or composite literals, nor does it remove an already inserted
semicolon. Multiline comma-separated lists must have a trailing comma when the
last element ends a line with an eligible token; the parser must support that
form for argument lists and struct initializers.

```ore
greet(
    user,
)

func main() { greet(user) }
```

An opening function-body brace must stay on the signature's final line when the
signature ends in an eligible token: `func main()` followed by a newline then
`{` has an intervening semicolon and is invalid. The same insertion rule applies
to other constructs as their grammars are finalized.

Ownership, error, and async implications: insertion defines statement boundaries
only. It does not change ownership or cleanup rules. Postfix `?` terminates a
line without changing its error-propagation semantics; `await` and `go` do not
themselves trigger insertion. Operands and statement legality remain subject to
the expression and statement grammar.

Compiler impact: retain the last significant token and comment newline
information in the lexer. Give synthetic semicolons a source location at the
triggering newline (the first newline in a multiline comment) or EOF. The parser
must honor inserted and explicit separators and diagnose invalid line breaks.
The lexer cannot tell a closing type-argument `>` from the comparison operator,
so the parser inserts that semicolon itself. It happens when the parser consumes
the `>` closing a type argument list, including a `>` split off a `>>`, `>=`, or
`>>=` token, provided the next token does not already follow on the same
line. Insertion requires a newline (including one inside a block comment) or
EOF between that `>` and the next token, and the next token must not already be
a separator. The inserted semicolon is placed where the lexer would place it.
Conformance cases are in `tests/conformance/statement-boundaries.md`; executable
coverage is pending the lexer and parser milestones.

## 3.8 String literal forms — LOCKED

Zore has two string literal forms. Both produce values of the built-in `string`
type and permit Unicode text despite identifiers being ASCII-only.

### Double-quoted strings

A double-quoted string starts and ends with `"`. Backslash introduces an escape
sequence. An escaped quote does not close the literal. Physical newlines are
not allowed in this form; use a newline escape or a raw string instead.
Backslash followed by a physical newline is not line continuation.

```ore
let greeting = "Hello\nZore"
let empty = ""
```

The accepted escape sequences and decoding rules are defined in §3.9.

### Backtick raw strings

A raw string starts with a backtick (U+0060) and ends at the next backtick.
Its contents are literal: backslashes do not introduce escapes, and physical
newlines are allowed. Whitespace and indentation are preserved, with no automatic
trimming or dedenting. CR and LF characters within raw contents are preserved;
the whitespace rules outside literals in §3.7 do not strip their contents.
A backtick cannot appear within this form, and a backslash cannot escape its
closing delimiter. Use a double-quoted string to include a backtick.

~~~ore
let path = `C:\projects\zore`
let message = `first line
second line`
let emptyRaw = ``
let backtick = "`"
~~~

### Common rules

Neither form supports string interpolation in the MVP. Text such as `${name}`
or `{name}` remains literal content. Triple-quoted strings are not an additional
literal form. Comment delimiters within either string form are ordinary contents.
EOF before a closing delimiter is a compile-time lexical error.

A complete string literal is one token. Newlines inside raw strings do not
trigger semicolon insertion; a newline or EOF after the closing delimiter does,
as specified in §3.7.

Ownership, error, and async implications: both forms have the ordinary Copy
semantics of `string` (§10.2), with no new ownership, cleanup, error-propagation,
or async behavior. Malformed literals are compile-time errors. The internal
string representation remains an implementation detail (§41.5); the complete
string value/encoding model is not settled by delimiter choice.

Compiler impact: distinguish escaped and raw literal scanning, preserve source
byte spans across Unicode and multiline contents, and identify the opening
delimiter in unterminated-literal diagnostics. Preserve raw contents without
normalizing line endings. Do not implement speculative escape forms.
Conformance cases are in `tests/conformance/strings.md`; executable coverage is
pending the lexer and literal-decoding milestones.

## 3.9 String escape sequences — LOCKED

Double-quoted strings accept exactly the following escapes:

| Source spelling | Decoded character |
| --- | --- |
| `\n` | Line feed, U+000A |
| `\r` | Carriage return, U+000D |
| `\t` | Horizontal tab, U+0009 |
| `\\` | Backslash, U+005C |
| `\"` | Double quote, U+0022 |
| `\uXXXX` | Unicode scalar value specified by exactly four hexadecimal digits |
| `\UXXXXXXXX` | Unicode scalar value specified by exactly eight hexadecimal digits |

Hexadecimal digits may be `0`–`9`, `a`–`f`, or `A`–`F`. Digit separators and
braces are not allowed in Unicode escapes. Each escape consumes exactly its
specified number of digits; subsequent characters are ordinary string contents.
For example, `"\u0041B"` has the value `"AB"`.

A Unicode escape must denote a scalar value in U+0000–U+10FFFF excluding the
surrogate range U+D800–U+DFFF. Surrogate pairs are not accepted as a substitute
for a scalar escape. U+0000 is permitted via `\u0000`; it is a character in the
value, not a string terminator. Escapes decode once: a decoded backslash does
not start another escape. Escaped and directly written versions of the same
character have the same string value.

```ore
let line = "Hello\nZore"
let quote = "She said \"hello\""
let letter = "\u0041"
let emoji = "\U0001F600"
```

Unknown escapes, missing or non-hexadecimal digits, surrogate values, and values
above U+10FFFF are compile-time errors. In particular, `\a`, `\b`, `\f`, `\v`,
`\xNN`, octal escapes, `\0`, and `\'` are not supported. A single quote can be
written directly inside a double-quoted string. Physical newline continuation
remains prohibited (§3.8). Rune-literal escapes are defined in §3.10.

Raw backtick strings do not decode any escapes: `\n` in a raw string is a
backslash followed by `n`. A decoded newline inside a quoted string does not
trigger semicolon insertion, since it is part of the value, not a source newline.

Ownership, error, and async implications: escape spelling does not change
ordinary string Copy semantics, cleanup, error propagation, or async behavior.
Malformed escapes produce compile-time diagnostics, not runtime errors.

Compiler impact: validate exact digit counts and scalar ranges, decode escapes
once, and retain original source spans for diagnostics at malformed escapes.
Do not interpret Unicode escapes as arbitrary byte escapes or use host-language
escape acceptance as the definition of Zore. Runtime string layout remains open.
Conformance cases are recorded in `tests/conformance/strings.md`; executable
coverage is pending lexer and literal-decoding implementation.

## 3.10 Rune literals — LOCKED

A rune literal is enclosed in single quotes and contains exactly one Unicode
scalar value after escape decoding. It denotes a rune value, not a one-character
string. A scalar is in U+0000–U+10FFFF, excluding U+D800–U+DFFF.

The scalar may be written directly or using any escape supported by
double-quoted strings (§3.9), plus `\'` for a single quote. Thus the complete
rune escape set is `\n`, `\r`, `\t`, `\\`, `\"`, `\'`, `\uXXXX`, and
`\UXXXXXXXX`. Unicode escapes use the same exact digit counts, scalar validation,
and single-pass decoding as string escapes. A double quote may also be written
directly. A single quote or backslash in the value must be escaped.

```ore
let letter = 'A'
let accented = 'é'
let emoji = '😀'
let newline = '\n'
let escapedLetter = '\u0041'
let quote = '\''
```

Empty literals, multiple decoded scalars, physical newlines, unknown or malformed
escapes, surrogate values, and out-of-range values are compile-time errors.
EOF before the closing quote is an unterminated-literal error. Backslash followed
by a physical newline is not continuation. There is no raw rune literal form.

The count is Unicode scalars, not bytes or displayed characters. For example,
`'é'` is valid, but `'e\u0301'` contains two scalars and is invalid. Do not
normalize multiple scalars into one to accept a literal. Surrogate-pair escapes
are invalid even if they would encode a single character in UTF-16.

A rune literal is one token and triggers semicolon insertion at a following
newline or EOF (§3.7). An escaped newline is part of its value, not a source
statement boundary. Comment markers and quote characters are interpreted within
the literal rather than starting comments or other literal forms.

Ownership, error, and async implications: rune values are primitive Copy values
(§10.2). This syntax introduces no new ownership, cleanup, error-propagation,
or async rules. Literal errors are compile-time errors. Numeric representation,
conversions, and contextual literal typing remain part of the type-model
decisions; this section does not declare `rune` an alias of an integer type.

Compiler impact: scan single-quoted literals, reuse the specified escape decoder
with the rune-specific allowance for `\'`, validate exactly one decoded scalar,
and preserve source byte spans. Diagnose invalid contents and identify the opening
quote for unterminated literals. Conformance cases are in
`tests/conformance/runes.md`; executable coverage awaits lexer and literal decoding.

## 3.11 Integer literal bases — LOCKED

Integer literals support four bases:

| Base | Prefix | Digits |
| --- | --- | --- |
| Decimal (10) | None | `0`–`9` |
| Binary (2) | `0b` or `0B` | `0`–`1` |
| Octal (8) | `0o` or `0O` | `0`–`7` |
| Hexadecimal (16) | `0x` or `0X` | `0`–`9`, `a`–`f`, `A`–`F` |

Digits are ASCII. At least one digit is required, including after a base prefix.
Unprefixed literals are always decimal: leading zeros do not select octal.
Thus `0755` is decimal 755, `08` is decimal 8, and `0o755` is decimal 493.
Hexadecimal digits and the allowed prefix letter variants are case-insensitive.

```ore
let decimal = 42
let binary = 0b101010
let octal = 0o755
let hexadecimal = 0xFF
let leadingZeros = 0755
```

A prefix with no digits or a digit invalid for the selected base is a
compile-time error. For example, `0x`, `0b102`, `0o8`, and `0xG` are invalid
integer literals; they must not be silently accepted as a shorter valid number.

Digit separators are defined in §3.12 and floating-point forms in §3.13. Numeric
suffixes are excluded by §3.14; sign/operator grammar remains a separate decision.
Base notation denotes the
numeric value; it does not select an
integer width, signedness, or overflow behavior. Those remain type-model questions.

Ownership, error, and async implications: integer values retain primitive Copy
semantics (§10.2). Base spelling does not change ownership, cleanup, error
propagation, or async behavior. Malformed literal spellings are compile-time
errors, not runtime errors.

Compiler impact: recognize explicit base prefixes, validate the corresponding
ASCII digit set, preserve source byte spans, and decode using the selected base.
Do not use host parser conventions that interpret leading zeros as octal. Keep
literal spelling/value handling separate from target-width and range checking.
Conformance cases are in `tests/conformance/integers.md`; executable coverage
awaits lexer and literal-decoding implementation.

## 3.12 Numeric digit separators — LOCKED

A single underscore may separate two digits within a numeric digit sequence.
Both adjacent characters must be digits valid for that sequence's base. Group
sizes are unrestricted; separators have no effect on the numeric value.

For each integer base in §3.11, its digit sequence has this form, where `digit`
is an ASCII digit valid for the selected base:

```text
digit_sequence = digit { [ "_" ] digit } .
```

Separators are not allowed at the start or end of a digit sequence, immediately
after a base prefix, or next to another underscore. Removing underscores before
validating their positions must not make an invalid spelling valid.

```ore
let count = 1_000
let mask = 0xFF_FF
let bits = 0b1010_0101
let permissions = 0o7_55
```

`1000_`, `1__000`, `0x_FF`, `0b_10`, and `0o_755` are invalid numeric spellings.
`_1000` is an identifier under §3.5, not a numeric literal with a leading
separator. `0_755` remains decimal 755; underscores do not introduce implicit
octal notation. Hexadecimal letter digits count as digits for this rule.

For floating-point forms (§3.13), the same between-digits rule applies
separately to each digit sequence. Separators cannot touch a decimal point,
exponent marker, or exponent sign. This rule does not itself introduce float
forms or numeric suffixes. Unicode escapes retain their no-separator rule (§3.9).

Ownership, error, and async implications: separators affect readability only;
they do not change value, type, Copy semantics, cleanup, error propagation, or
async behavior. Invalid separator placement is a compile-time error.

Compiler impact: validate separator positions and base-specific digits before
decoding, ignore valid separators when computing the value, and retain the
original spelling's byte spans for diagnostics. Conformance cases are in
`tests/conformance/integers.md`; executable coverage awaits the lexer and decoder.

## 3.13 Floating-point literal forms — LOCKED

Floating-point literals use decimal notation, with a fractional part, a decimal
exponent, or both. A decimal point requires digits on both sides. An exponent
starts with `e` or `E`, may have one `+` or `-` sign, and requires decimal digits.

```text
decimal_digit    = "0" … "9" .
decimal_digits   = decimal_digit { [ "_" ] decimal_digit } .
decimal_exponent = ( "e" | "E" ) [ "+" | "-" ] decimal_digits .
float_literal   = decimal_digits "." decimal_digits [ decimal_exponent ]
                | decimal_digits decimal_exponent .
```

```ore
let fraction = 0.5
let whole = 1.0
let million = 1e6
let small = 1.5e-3
let large = 2E+8
let grouped = 1_000.25
```

All digits are ASCII. Leading zeros remain decimal. A plain digit sequence such
as `1` is an integer literal, while `1.0` and `1e0` are floating-point literals.
The exponent scales the decimal significand by ten raised to the exponent value.
The sign within an exponent is part of the literal token; leading unary signs
belong to the separate expression-grammar decision, not this production.

Underscores may separate digits within each integer, fractional, or exponent
digit sequence (§3.12), as in `1_000.2_5e1_0`. They may not touch the decimal
point, exponent marker, or exponent sign, or occur consecutively or at the end.

`.5`, `1.`, and `1.e2` are not floating-point literals. Write `0.5`, `1.0`, and
`1.0e2` instead. Exponents with missing digits, such as `1e`, `1e+`, or `1.0e-`,
are malformed literals and must be diagnosed. Hexadecimal, binary, and octal
floating-point forms are not supported in the MVP. Numeric suffixes are not
supported (§3.14).

Float token recognition must not swallow a field-access dot: a dot is part of
a decimal float only when immediately followed by a decimal digit. For example,
`1.field` starts with an integer token and a separate dot, not a `1.` float.
Whether field access on such a value is legal is a semantic question. Standalone
`.5` or `1.` used as a numeric initializer must be rejected, not reinterpreted
as a supported float. Token splitting for recovery is an implementation detail.

Ownership, error, and async implications: float values retain primitive Copy
semantics (§10.2); literal spelling does not change cleanup, error propagation,
or async behavior. Malformed forms are compile-time errors. Default float type,
precision, rounding, representable range, overflow, and underflow remain type-model
decisions; this syntax does not silently select host floating-point behavior.

Compiler impact: recognize the grammar and validate separators before decoding,
retain source byte spans and the decimal spelling/value information needed by
later typing, and diagnose missing exponent digits. A completed float triggers
semicolon insertion at a following newline or EOF (§3.7). Conformance cases are
in `tests/conformance/floats.md`; executable coverage awaits lexer and decoder work.

## 3.14 Numeric suffixes — LOCKED OUT OF MVP

Integer and floating-point literals do not accept type suffixes or other numeric
suffixes. Write `42` and `1.5`, not `42u8`, `42int64`, `1.5f32`, or `1.5float32`.
This applies to every integer base and to floats with or without an exponent.

```ore
let count = 42
let ratio = 1.5
```

An identifier-like continuation immediately attached to a numeric spelling,
such as `42u8`, `0xFFu8`, `1e3f64`, or `42_name`, is invalid. The compiler must
diagnose it rather than silently accepting the numeric prefix and treating the
rest as an implicitly separated declaration or expression. Token splitting for
error recovery is an implementation detail.

Characters that are part of the locked numeric grammar are not suffixes:
`0xFF` and `0xF32` are hexadecimal integers, `1e3` is a decimal float, and
`1_000` uses digit separators. This rule must not reject such valid spellings.

Literal types will be determined by the type model, including context, defaults,
and explicit typing/conversion rules when locked. This decision does not define
those rules or introduce annotation or conversion syntax.

Ownership, error, and async implications: excluding suffixes does not change
numeric Copy semantics, cleanup, error propagation, or async behavior. Unsupported
suffixes are compile-time errors, not runtime errors.

Compiler impact: validate numeric-token boundaries and produce source-aware
diagnostics for unsupported suffixes, preserving spans through recovery. Do not
inherit suffix syntax from the implementation language. Pending conformance cases
are in `tests/conformance/integers.md` and `tests/conformance/floats.md`.

---

## 3.15 Keyword reservation policy — LOCKED

Zore will reserve both keywords used by adopted MVP syntax and an explicit set
of words intended for possible future syntax. Future-reserved words cannot be
used as identifiers, even while their corresponding features are unavailable.

Reservation does not implement or promise a feature and does not move anything
from OUT OF MVP into the MVP. A future feature still requires a specification
change under §53 before implementation.

The MVP keyword list is locked in §3.17 and the future-reserved list in §3.16.
Implementers must not reserve
additional words based on their presence in another language or speculation.

For an illustrative future-reserved word `word`, a declaration such as
`let word = 1` must be rejected once `word` is explicitly on that list. This
example defines the effect of reservation; it does not reserve the name `word`.
Ordinary identifiers remain case-sensitive (§3.5); keyword matching uses exact
spelling, without case folding.

Ownership, error, and async implications: reservation changes which names can be
declared, not ownership, cleanup, error propagation, or async semantics. Uses of
reserved words as names produce compile-time errors. In particular, reserving
`unsafe` does not permit unsafe Zore code or alter the MVP safety guarantees.

Compiler impact: maintain explicit keyword sets and distinguish unavailable
future syntax from supported syntax in diagnostics. Changes to the lists must
also record semicolon-insertion behavior where relevant (§3.7). Use only the
explicitly locked future-reserved list. Pending conformance
requirements are in `tests/conformance/keywords.md`.

## 3.16 Future-reserved words — LOCKED

The following seven words are reserved for possible future features:

```text
trait impl enum match unsafe macro defer
```

These exact lowercase spellings cannot be identifiers in any name position,
including package, type, function, receiver/parameter, local, or field names.
There is no escaped-identifier syntax to bypass reservation in the MVP.
Reservation is case-sensitive and matches whole words: `Trait`, `matchValue`,
and `_unsafe` are ordinary identifier spellings, not future-reserved words.

For example, `let match = 1` is invalid, while `let matchValue = 1` is valid
with respect to name reservation. Reserved spellings inside string literals or
comments remain ordinary contents.

None of these words enables its associated syntax in the MVP. In particular,
reserving `defer` does not add deferred execution or make cleanup depend on it;
reserving `unsafe` does not weaken memory-safety guarantees. They are not
identifiers or statement-ending tokens for semicolon insertion (§3.7).

Ownership, error, and async implications: the words introduce no executable
operations or new ownership, cleanup, error, or async behavior. Attempts to use
them as names or unavailable syntax produce compile-time diagnostics.

Compiler impact: recognize these seven exact spellings as future-reserved,
diagnose unsupported use with source spans, and keep their token classification
distinct from ordinary identifiers and supported grammar roles. Do not implement
the associated features. Pending conformance cases are in
`tests/conformance/keywords.md`.

## 3.17 MVP keywords and predeclared names — LOCKED

The MVP keyword list is:

| Purpose | Keywords |
| --- | --- |
| Structure | `package import func type struct interface` |
| Bindings | `let var const` |
| Ownership | `mut own` |
| Control flow | `if else for in break continue return` |
| Concurrency | `async await go select` |
| Built-in type syntax | `map channel` |
| Literal values | `true false nil` |

Match these exact lowercase spellings as whole words. They cannot be used as
identifiers in any name position. (`case` and `default` are not keywords; they
have a meaning only at the start of a `select` arm, §19.14.) For example, `let func = 1` and a field named
`own` are invalid; `let functionName = 1` is valid with respect to reservation.
Case variants such as `Func` and `True` are ordinary identifier spellings.
There is no escaped-identifier syntax in the MVP.

At a following newline or EOF, `break`, `continue`, `return`, `true`, `false`,
and `nil` trigger semicolon insertion. The other MVP keywords do not (§3.7).
Keywords within strings or comments do not participate in token classification.

This locks lexical classification, not unspecified grammar. In particular,
`break` and `continue` follow §5.10, and conditionals follow §5.9.
The type/zero-value rules for `nil` are locked in §41.4. Do not infer
Go's complete statement or type grammar from this keyword list.

Built-in primitive type names (§6.1), `Array`, `Task`, `Mutex`, `mutex`, `error`, `println`,
`panic`, `clone`, and `drop`, and the constraints `any`, `copyable`, `comparable`, and
`ordered` (§22.1), are predeclared names, not keywords. They are lexed as identifiers
and resolved semantically; their built-in meaning may still require special
compiler handling. Declarations may not shadow them (§3.18). This distinction
does not imply user-defined generic types or additional built-in APIs.

Ownership, error, and async implications: keyword classification preserves the
locked meanings of `mut`, `own`, `async`, `await`, and `go`; it does not change
ownership or cleanup. Classifying `drop` and `clone` as predeclared names does
not remove their semantic requirements. Invalid keyword use is a compile-time
error. Cleanup on future loop exits must obey ordinary ownership rules when
their control-flow semantics are finalized.

Compiler impact: recognize the exact keyword set, preserve byte spans, and
separate lexical keywords from semantic built-in identities. Enforce the shadowing
rule in §3.18. Pending conformance cases are in `tests/conformance/keywords.md`
and `tests/conformance/statement-boundaries.md`.

## 3.18 Predeclared-name shadowing — LOCKED

User declarations must not shadow predeclared names in unqualified lookup.
This applies at package scope and in all nested scopes, including function and
closure bodies, parameter names, receiver binding names, local bindings,
constants, and user-defined type or function names. If an import introduces a
name into unqualified lookup, that name must obey the same restriction.

The protected names currently established in §3.17 are all primitive type names
from §6.1, plus `Array`, `Task`, `Mutex`, `mutex`, `error`, `println`, `clone`, `drop`,
`any`, `copyable`, `comparable`, and `ordered`.
Any additional predeclared names must be explicitly specified; implementations
must not protect speculative library names. The full built-in API inventory
and signatures remain separate decisions.

```ore
let println = 42 // invalid: shadows a predeclared name
let string = 1   // invalid: shadows a predeclared type name
```

The rule matches exact case-sensitive spelling. `Println` does not shadow
`println`. This is a semantic name-resolution restriction, not keyword
classification: predeclared names continue to be lexed as identifiers.

Fields and method names use member lookup and are not rejected merely for
matching a predeclared name. For example, a field named `string` accessed through
`record.string` does not shadow the unqualified type name. The receiver binding
itself remains subject to the no-shadowing rule. This preserves user-defined
`drop` methods (§14.3), whose special cleanup contract still applies. Keywords
remain forbidden as member names (§3.16–3.17). Method/member conflicts follow
§7.8; ordinary user-defined-name shadowing follows §5.7.

Ownership, error, and async implications: declarations cannot redirect
unqualified built-in operations such as `drop` or `clone` to another binding.
Their existing ownership and cleanup semantics remain unchanged. Violations
produce compile-time diagnostics; this adds no runtime error or async behavior.

Compiler impact: reject conflicting declarations during resolution, identify
the declaration span and protected predeclared name, and distinguish unqualified
binding lookup from member lookup. Do not implement the rule as a lexical ban.
Pending conformance cases are in `tests/conformance/keywords.md`.

## 3.19 Program entry point — LOCKED

An executable program's entry package is named `main`. It must declare exactly
one package-level function named `main` with no receiver, no parameters, no
results, and no `async` modifier:

```ore
package main

func main() {
    println("Hello, Zore!")
}
```

The program runs by calling `main` in the program's initial task (§18.10).
When `main` returns normally, its scope cleanup runs as for any function and
the process then exits with status 0. Tasks still running are abandoned as
specified in §18.11.

If the initial task panics, it unwinds (§15.4), the runtime reports the panic
on standard error (§18.10), and the process exits with a nonzero status. The
specific nonzero value is implementation-defined. An abort (§15.4) also ends
the process with a nonzero status and without further unwinding.

A `main` package without such a function has no entry point and is rejected
when checked or built as a program. These declarations are invalid:

```ore
func main(args Array<string>) {}   // invalid: parameters
func main() int { return 0 }       // invalid: results
func main() error { return nil }   // invalid: results
async func main() {}               // invalid: async entry point
```

A method named `main` does not satisfy or conflict with this requirement;
methods use member lookup (§3.18). In a package not named `main`, a function
named `main` has no special meaning. Apart from being called at program start,
the entry-point `main` is an ordinary package-private function.

There is no command-line argument parameter, returned exit code, error-returning
`main`, or package initialization hook in the MVP entry point. Later additions
require a specification change; a program-arguments or exit-code API belongs to
the standard library (§37).

Ownership, error, and async implications: `main` owns and cleans up its locals
under ordinary rules; values whose lifetime ends inside `main` are dropped
before the process exits normally. `main` cannot return an `error`, so every
error result it receives must be handled or explicitly discarded (§15.6); `?`
inside `main` is invalid because `main` has no error result (§15.2). Because
`main` is not `async`, it cannot use `await` directly (§17.8); it may create
tasks with `go` and retrieve them with `.wait()` (§18.9).

Compiler impact: when checking or building a `main` package as a program,
require exactly one conforming package-level `main` and diagnose a missing or
non-conforming declaration at its name, or at the package clause when absent.
Duplicate functions are already invalid (§7.8). Code generation emits a
process entry that runs `main` in the initial task, then exits with status 0,
and makes initial-task panics exit nonzero. Pending conformance cases are in
`tests/conformance/entry-point.md`.


## 3.20 Projects, packages, and imports — LOCKED

**Packages are folders.** A package is the set of `.ore` files directly inside
one folder (subfolders are separate packages). Every file declares the same
package name, and the files share one package scope: a declaration in one file
is visible in the others (§3.2), and a name declared twice anywhere in the
package is a duplicate (§7.8). Files are processed in file-name order, so
diagnostics are deterministic. Test files and other file kinds are not part of
this decision.

**Projects.** A project is a folder tree whose root holds `zore.toml` (§3.4).
The `name` in that file is the project name. A project name is one or more
ASCII letters, digits, `_`, or `-`, and must not be `zore`, which is reserved
for standard packages.

**Building a program.** `zore check`, `zore build`, and `zore run` take a file
(§45). The file's folder is the entry package; building or running it as a
program requires it to be named `main` (§3.19), while `zore check` accepts any
name. The project root is found by walking up from that folder to the first
folder that holds `zore.toml`. When there is none, the entry folder is a project with no
name, which can import only standard packages.

**Import paths.** An import declaration names exactly one package by a
double-quoted path of segments separated by `/` (§3.3):

| Path | Package |
| --- | --- |
| `"zore/<path>"` | The standard package at `<path>` shipped with the compiler, such as `"zore/os"` or `"zore/os/exec"` (§37.2–§37.5) |
| `"<project>/<dir>/..."` | The folder `<dir>/...` below the project root, where `<project>` is the project's name |

Anything else is an error that names the path. A project imports only its own
packages and the standard packages; dependencies on other projects are outside
the MVP. A path segment, other than the project name, is an ASCII identifier
(§3.5), and the last segment of the path is the imported package's name: a
non-`main` package's package clause must be exactly that name. The `main`
package cannot be imported.

**Using imports.** An import is visible only in the file that declares it, and
its name is the last path segment. Members are used with a qualifier:

```ore
import "myapp/shapes"
import "zore/strings"

func main() {
    let circle = shapes.Circle{Radius: 2}
    println(shapes.Area(circle))
    println(strings.ToUpper("ok"))
}
```

`shapes.Name` names an exported package-level function, type, or constant of
the imported package. A qualified type name is written the same way in every
type position (`var c shapes.Circle`). The rules are:

- *Export rule (§4.1).* Only names beginning with ASCII `A`–`Z` can be used
  across packages. Using an unexported or missing name is an error naming the
  package. An exported struct field or method is usable anywhere. An
  unexported field or method is usable only inside its own package: it cannot be
  read, written, or called, and, because a struct literal must name every field
  exactly once (§8.4), a struct with an unexported field can be constructed
  only inside its package, which offers constructor functions instead.
- *Types are nominal.* A struct or named type (§8.5) is identified by its
  package and name; `a.T` and `b.T` are different types.
- *Methods.* A method may be declared only on a struct or named type (§8.5)
  defined in the same package, so imported types cannot be extended. Calling an exported method on a
  value of an imported type is allowed. Custom `drop` and `clone` methods are
  honored wherever such a value is created, copied, or destroyed.
- *Unused or conflicting imports are errors.* An import that is never used in
  its file, a second import of the same path in a file, two imports in one file
  with the same name, and an import whose name matches a package-level
  declaration or a predeclared name are all errors.
- *No cycles.* A package that imports itself, directly or through other
  packages, is an error that lists the chain.
- *Not in the MVP.* Import aliases, dot imports, blank imports, grouped
  imports, package-level `var`, `init` functions, and imports of other
  projects. Package-level `let` is specified in §3.21.

Each package is checked once, however many files import it. Package-level
constants are evaluated as in §5.3 and may be used across packages when
exported. Resource cleanup, borrowing, and `async` rules do not depend on which
package a function or type came from.

Ownership, error, and async implications: none beyond the rules above; imports
add names, not new kinds of values.

Compiler impact: load the entry package, parse every file, discover imports,
and load imported packages (folder or standard) before resolving names;
resolve each package's declarations into a shared set of IDs and give every
file its own import table; check visibility at each qualified use and field or
method access; report cycles during loading; give each declaration a symbol
that includes its package path so equal names in different packages never
collide in generated code. Pending conformance cases:
`tests/conformance/packages.md`.

## 3.21 Package-level values — LOCKED

A package may declare values outside any function with `let`:

```ore
let EOF = error("EOF")
let DefaultPort int = 8080
let Banner = "zore " + version()
```

The rules are:

- *One name, one value.* Each declaration names exactly one value and has an
  initializer; the type may be written or inferred, as for a local `let`
  (§5.4). Package-level `var` and multiple names are errors.
- *Never changes.* A package-level value cannot be assigned, compound-assigned,
  or passed to a `mut` parameter. Reading it gives a copy.
- *Copy types only.* Its type must be Copy (§10.2) and must not contain a slice,
  a function value, a `Task`, a `channel`, or a `Mutex`. Booleans, numbers,
  runes, strings, `error`, named types (§8.5), and structs and fixed arrays of
  those are allowed.
- *Names.* A package-level value is in package scope with the package's
  functions, types, and constants, and is exported by the usual rule (§4.1):
  `config.Port` reads another package's exported value.
- *Initialization order.* Before `main` runs, every value is computed once, in
  order: the packages a package imports come first, and within a package the
  declarations run in the order of the package's files and their text. An
  initializer may call functions, start tasks, and use other values, but
  everything it reaches, directly or through the functions it calls or starts,
  may read only package-level values initialized before it. Anything else is a
  compile-time error that names the later value.
- *Failure.* A panic in an initializer ends the program the same way a panic in
  `main` does (§3.19, §15.4); `main` does not run.

```ore
let Limit = 3
let Squares = [int; 2]{Limit, Limit * Limit}   // uses an earlier value

let Early = Late + 1   // invalid: Late is initialized after Early
let Late = 2
var Counter = 0        // invalid: package-level values cannot change
let Ready = channel<bool>()   // invalid: channels are not allowed
```

Ownership, error, and async implications: package-level values are Copy and
immutable once initialized, so any task may read them without locking. A task
started by an initializer may read only values initialized before that
initializer. Text held by a package-level value lives until the program ends.

Compiler impact: give each value a function that computes it and a global
storage slot; check each initializer like a function body with an inferred
result; reject later values reached through the call graph, including closures
and spawned tasks; run the initializers in order from the entry point before
`main`, stopping on a panic. Pending conformance cases:
`tests/conformance/packages.md`.

---

# 4. Visibility

## 4.1 Export rule — LOCKED

Visibility follows a Go-like naming convention:

- package declarations and fields whose identifiers begin with ASCII `A`–`Z` are exported from the package
- those beginning with ASCII `a`–`z` or `_` are package-private

Local bindings do not become package exports because of capitalization.
For example, `_User` is package-private despite its second character being uppercase.

Example:

```ore
type User struct {
    Name string
    age int
}
```

`User` and `Name` are exported.

`age` is package-private.

No additional `public`, `private`, `pub`, or visibility modifier syntax is part of the MVP.
How imports apply this rule across packages is specified in §3.20.

---

# 5. Variables and Constants

## 5.1 Immutable bindings — LOCKED

`let` declares an immutable binding.

```ore
let user = loadUser()
```

The binding cannot later be reassigned.

The value itself may still participate in ownership operations according to its type and context.

## 5.2 Mutable bindings — LOCKED

`var` declares a mutable binding.

```ore
var count = 0
count = count + 1
```

## 5.3 Compile-time constants — LOCKED

`const` declares a compile-time constant.

`const` is unrelated to ownership.

Do not interpret `const` as:

- immutable borrow
- readonly reference
- permanent object
- ownership modifier

### Constant-expression subset

A constant expression's value must be one of the scalar types: `bool`, an
integer type, a float type, `rune`, or `string`. Struct types, `[T; N]`, `[]T`,
`Array<T>`, `map[K]V`, `error`, `Task<...>`, and `channel<T>` have no constant
form in the MVP. `nil` is never a constant expression, since the only types it
targets (`error` and `Task<...>`, per §41.4) are runtime values.

A constant expression is exactly one of:

- a `bool`, integer, float, rune, or string literal;
- a reference to another `const` name;
- a unary or binary operator from §7.6 applied to constant-expression operands:
  arithmetic `+ - * / %`, bitwise `& | ^` and unary `^`, shifts `<< >>` (the
  shift-count operand must also be a constant expression), unary `+ -` and `!`,
  comparisons `== != < <= > >=`, boolean `&& ||`, and string `+` concatenation;
- an explicit numeric conversion (§6.6) applied to a constant-expression
  operand; or
- any of the above enclosed in parentheses.

Every operand of a constant expression must itself be a constant expression;
function/method calls, indexing, field/member access, `clone`, `drop`,
`await`, `?`, `go`, and struct/collection literals are never constant
expressions, matching the general rule already stated in §6.5 that calls,
mutation, I/O, and other runtime effects are not constant operations.

A named constant may reference another `const` declared later in the same
package; constants have no evaluation side effects, so declaration order does
not affect their values. A cycle between constant definitions (directly or
through intermediate constants) is a compile-time error.

The untyped-constant preservation rule in §6.5 continues to apply: an untyped
constant expression keeps its exact mathematical result until it is assigned a
type, and is rejected if that result is not representable in the required
type. Once a constant has an explicit type (`const name Type = expression`) or
takes a default type from use, combining it with another value follows the
ordinary typed rules in §6.6 — there is no implicit conversion between two
differently typed constants.

Anywhere a constant expression is required outside a `const` declaration — for
example, the size `N` in a fixed array type `[T; N]` — the same subset and
typing rules apply, and the value must be a non-negative integer. Array literal
construction is specified in §12.6 and map construction in §13.3;
collection literals are not constant expressions.

```ore
const pageSize = 4096
const bufferSize = pageSize * 4   // 16384, untyped int until it takes a type
const maxRetries uint8 = 5
const label = "buf-" + "v2"       // constant string concatenation
const half = maxRetries / 2       // typed uint8 arithmetic, no implicit conversion
```

Ownership, error, and async implications: constant expressions never own,
borrow, or clean up a resource; they are pure compile-time values. This does
not change `?` propagation, task/channel semantics, or the zero-value rules in
§41.4.

Compiler impact: evaluate constant expressions exactly (§6.5's exact-result
requirement) before any range check, reject cycles between constant
definitions during resolution rather than during evaluation, and reuse the
typed-conversion checks from §6.6 for explicit conversions inside constant
expressions. Pending conformance cases are in
`tests/conformance/constant-expressions.md`.

## 5.4 Initialization syntax — LOCKED

The confirmed initialization form is:

```ore
let x = value
var x = value
let x Type = value
var x Type = value
```

Some earlier design examples used `:=` for illustration. `:=` has **not** been locked as Zore syntax.

**MVP implementations MUST NOT add `:=` unless the specification is explicitly revised.**

Every `let` and `var` declaration requires an initializer. An explicit type follows
the name without a colon. Thus `var count int64 = 0` is supported, while
`var count int64` is not. An explicit type constrains the initializer; it does
not authorize implicit conversions that the type model has not defined.

Constants use `const name = expression` or `const name Type = expression`.
The initializer must be compile-time evaluable; the constant-expression subset
is locked in §5.3, and numeric defaults/conversion rules are locked in
§6.5–6.6. `const` does not become a runtime immutable-binding alternative.

```ore
let count int64 = 0
var ratio float32 = 0.5
const limit = 100
const capacity int64 = 100
```

Multiple-result bindings use `let name, name = expression` or the corresponding
`var` form. There is one initializer expression, evaluated once, producing exactly
as many results as targets. `_` may occupy any target position (§5.5). Mixed
per-name type annotations and typed multiple bindings are not supported in the
MVP; use separate declarations when explicit per-name types are needed. This
does not add tuple destructuring or comma-separated initializer expressions to
binding declarations.

## 5.5 Discard target `_` — LOCKED

Standalone `_` may be used as a discard target in `let` and `var` bindings and
ordinary assignments, including individual positions in a multiple-value result.
It creates no binding, occupies no name in scope, and cannot be read as a value.
Multiple discard positions may occur in the same binding or assignment.

```ore
let value, _ = pair()
let _ = calculate()
var _ = calculate()
_ = calculate()
let _, _ = pair()
```

These examples may also discard error results explicitly. Error-result use and
discard requirements are specified in §15.6.

The right-hand expression is evaluated normally, exactly once; discarding its
result does not suppress side effects or bypass type checking. Each `_` consumes
one result position, so ordinary result-count checks still apply. This does not
introduce arbitrary destructuring or pattern matching. Multiple-assignment
evaluation and storage ordering are specified in §5.6.

Ownership follows ordinary assignment semantics (§10.1). Discarding a Copy value
discards a copy and leaves its source usable. Discarding a Move value transfers
ownership from its source, making that source unavailable; the discarded owned
value undergoes normal deterministic destruction. `_ = resource` must not be
treated as a no-op that preserves a Move resource. The compiler must reject
moving a borrowed place into `_`, just as with any other ownership transfer.

A discarded borrowed view does not acquire ownership of its backing storage and
must not destroy the backing owner. All ordinary borrow/lifetime checks remain
in force. Discarding an owned temporary or result must not leak it or cause a
double-drop. Destruction follows the ordinary lifetime and cleanup rules (§14);
this section introduces no alternative cleanup ordering.

`_` cannot be used as an expression, argument, type name, function name, field
name, or parameter/receiver binding name. Named identifiers such as `_unused`
remain ordinary bindings. This decision does not introduce `_` in `const`
declarations, imports, patterns, or other contexts.

Ownership, error, and async implications: discard uses the existing Copy/Move,
borrow, and drop rules. Evaluation still performs any explicitly requested `?`,
`await`, or task creation. Discarding a Task handle detaches it without cancelling
the task (§18.6). Error-value discards follow §15.6. No implicit
error propagation, awaiting, or cancellation is introduced by `_`.

Compiler impact: represent discard targets without allocating a user binding,
retain source spans, type-check their result positions, and lower ownership
transfers and required cleanup explicitly. Reject reads of `_` and invalid
ownership transfers. Pending cases are in `tests/conformance/discards.md`.

## 5.6 Assignments and compound updates — LOCKED

Assignment is a statement of the form `target = expression`, never an expression
with a result. A target must be an existing mutable local, a writable field or
index place, or `_`. Reject assignment to immutable bindings, constants, and
non-writable places. Field/index access remains subject to type, mutability,
and borrowing rules; assignment does not grant write access through a shared
borrow or imply that all collection elements are writable.

The compound assignment operators are:

```text
+= -= *= /= %= &= |= ^= <<= >>=
```

A compound assignment has one writable place target and one RHS value. It
performs the corresponding binary operation followed by assignment, using that
operator's type rules (§7.6). Evaluate the target once, obtain its current value,
evaluate the RHS, compute the result, and store it. `_` is not a compound target
because it has no stored value. Preserve exclusive update access and reject
conflicting accesses under the ordinary borrow rules; do not duplicate target
side effects by naively rewriting `target += value` as two evaluations of target.

Multiple assignment allows a comma-separated target list and either a matching
list of single-result RHS expressions or one expression returning the matching
number of results. Do not implicitly splice a multiple-result call into an RHS
list with other expressions. The target and result counts must agree.

```ore
var left = 1
var right = 2
left, right = right, left

var count int64 = 0
count += 1
```

Evaluation and storage occur in three phases:

1. Evaluate target places from left to right, including receiver/base/index
   expressions, exactly once. `_` has no place to evaluate.
2. Evaluate RHS expressions from left to right and retain their resulting values
   before performing any assignment stores. One multiple-result expression is
   evaluated once.
3. Assign the retained results to targets from left to right; process discards
   using §5.5. Each writable target must still be valid when it is assigned.

This ordering supports swaps without rereading a value after a preceding store.
All non-discard targets in a multiple assignment must be provably non-overlapping.
Reject known overlaps and possible overlaps that cannot be proven disjoint.
For example, `a, a = 1, 2` is invalid; repeated `_` targets are allowed. For
indexed places, distinct source expressions alone do not prove distinct storage.
Map assignment targets follow the special §13.3 rule: retain map identity and
key rather than a borrowable element place. Compound map assignment is rejected;
map subscripts used as expressions produce two results, not writable values.

Ownership operations occur during evaluation according to normal Copy/Move
rules. Before replacing a still-owned value in a target, perform its required
cleanup. If its prior value has already been moved out, do not drop it again.
Move-value swaps must preserve exactly one owner per value. Retaining RHS values
must not duplicate Move values, move borrowed storage, or invalidate target places.

If target or RHS evaluation propagates an error, later evaluations and the
not-yet-started stores do not run, and required cleanup of owned temporaries
occurs. Earlier expression side effects and ownership transfers are not rolled
back. This is sequencing, not a transactional assignment guarantee. The precise
panic cleanup contract remains separate.

Ownership, error, and async implications: replacements, compound updates, and
swaps obey the same ownership model as other expressions. Any target or borrow
retained across an explicit `await` must remain valid and exclusive where needed;
reject the program when this cannot be proven. Error results still require use
or explicit discard (§15.6). No implicit propagation or concurrency is introduced.

Compiler impact: represent target places separately from RHS temporaries, lower
the evaluation/store phases explicitly, check result counts and target overlap,
track moved-out versus still-owned target values, and insert required drops.
Retain source spans for invalid target, overlap, and ownership diagnostics.

## 5.7 Declaration scope and shadowing — LOCKED

Reject duplicate declarations in the same scope. A nested scope may shadow an
ordinary user-defined name from an enclosing scope. It must not shadow a
predeclared name at any scope (§3.18), and keyword restrictions still apply.
Repeated `_` targets are not duplicate declarations because they introduce no
names. Multiple named targets in one binding must be distinct.

```ore
var count = 0
let count = 1 // invalid: duplicate declaration in the same scope
```

When a nested scope ends, name lookup again finds the enclosing declaration;
shadowing does not transfer or merge ownership between the two bindings. Give
each declaration its own semantic identity. Their lifetimes, borrows, and cleanup
remain independently subject to ordinary rules, including across error exits
and async suspension. Duplicate declarations are compile-time errors.

Local declarations enter scope after their initializer (§5.8). Package function
and type declaration ordering and method/member conflicts follow §7.8;
package variable initialization order remains a separate decision.
Nested shadowing does not introduce a value-producing block expression.

Compiler impact: maintain lexical scopes and stable binding IDs, diagnose
duplicates with both declaration locations, and keep the predeclared-name rule
distinct from ordinary shadowing. Pending conformance cases for §5.4, §5.6, and
§5.7 are in `tests/conformance/bindings-assignments.md`.

## 5.8 Blocks and local scope — LOCKED

A block is a brace-delimited sequence of statements and creates a lexical scope.
Blocks may be empty or used as standalone statements. They do not produce values
and cannot be used as expressions. Statement boundaries follow §3.7.

A local declaration's names enter scope after its initializer has been evaluated.
The initializer resolves names in the previously existing environment, so a new
binding cannot refer to itself through its own name. All names in a multiple
binding enter scope together after the initializer. A nested declaration may
refer to an enclosing binding with the same name in its initializer.

```ore
let count = 1
{
    let count = count + 1 // initializer reads the outer count
    println(count)
}
println(count) // refers to the outer count again
```

Parameters, receiver bindings, and declarations in the outermost function-body
block share one scope for duplicate-name checking. A function-body declaration
cannot redeclare a parameter; a further nested block may shadow an ordinary
parameter name. Closure parameters and their outermost body follow the same rule.

Scopes constrain visibility and lifetimes but do not replace the last-use/drop
rules (§14). Exiting a scope on normal completion or control transfer must perform
its required cleanup. Returning a value does not permit a borrowed local to
escape its valid lifetime. Preserve these rules across error exits and suspension.

Compiler impact: distinguish statement blocks from expressions, maintain lexical
scopes and declaration-entry points, and preserve separate IDs for shadowed names.

## 5.9 Conditional statements — LOCKED

The conditional forms are:

```text
if condition { statements }
if condition { statements } else { statements }
if condition { statements } else if condition { statements } else { statements }
```

Conditions must have type `bool`. Parentheses around a condition are optional;
body braces are required. Evaluate conditions in order, selecting the first true
branch or the final `else` if present. Execute only the selected body. Each body
has its own block scope; branch-local declarations do not escape it.

There is no initializer clause before an `if` condition. `else` must occur on
the same line as the preceding closing brace so that semicolon insertion does
not separate it from the `if`. Chained `else if` and a final `else` are optional.
An `if` is a statement, not a value-producing expression.

Ownership analysis must merge branch states safely; a move on one possible path
does not permit an unconditional later use. Required cleanup follows each actual
control-flow path, including propagated errors. Conditions and bodies may use
explicit `await` where otherwise valid; conditional syntax grants no extra borrow
or lifetime permissions.

Compiler impact: require bool conditions, reject missing braces and initializer
clauses, and lower branches to explicit control flow with source spans.

## 5.10 Loop statements and loop exits — LOCKED

The MVP supports these four loop forms:

```ore
for {
    work()
}

for ready() {
    work()
}

for var i = 0; i < limit; i += 1 {
    work()
}

for index, item in items {
    work()
}
```

The infinite form repeats its body until control exits. The conditional form
evaluates a bool condition before every iteration and exits when false. The
counting form executes its initializer once, tests a bool condition before each
iteration, executes the body when true, then executes its update before testing
again. Braces are required; condition parentheses are optional.

Counting headers have three explicit, nonempty clauses separated by semicolons.
The initializer may be a `let`/`var` binding, assignment, or call statement. The
update may be an assignment (including compound assignment) or call statement,
not a declaration. Any error results still require use or explicit discard.
Omitted counting clauses and other loop forms are not introduced by this grammar;
use the infinite or conditional form where appropriate.

The counting loop creates a scope for its initializer bindings, visible in the
condition, update, and body, but not after the loop. The body is a nested block
scope; body-local declarations are not visible in the update or condition.

Unlabelled `break` exits the nearest enclosing loop. Unlabelled `continue` ends
the current iteration of that loop. For a counting loop, `continue` proceeds to
the update, then the condition; for a conditional loop, it proceeds to the
condition; for an infinite loop, to the next body execution. `break` skips any
counting-loop update. Neither statement takes an operand or label. Using either
outside a loop is a compile-time error. They cannot target a loop outside the
current function or closure.

### Collection loops

```ore
for name in names { println(name) }
for i, name in names { println(i) }
for key, value in scores { println(key) }
for _, value in scores { total += value }
```

`for item in c` and `for index, item in c` visit the elements of a fixed array,
slice (shared or `mut`), or `Array<T>` in increasing index order from 0;
`index` is an `int`. A `string` is also a collection here, visited by character
as specified in §6.8: `item` is a `rune` copy and `index` the byte index. `for key, value in m` visits each entry of a map once;
both names are required for a map. Map visiting order is unspecified, but two
loops over the same unchanged map visit entries in the same order. `in` is a
keyword. Either name may be `_`, and there is no `:=`, `range`, or
declaration keyword in the header.

The collection expression is evaluated once, before the first iteration. If it
is a place, the loop shared-borrows that place until the loop ends: the body
may read it but may not assign, move, push to, pop from, remove from, or
mutably borrow it, directly or through another name. Otherwise its value is
held by the loop and destroyed when the loop ends, on every exit path. The
number of iterations is therefore fixed when the loop starts.

`item` and `value` are shared borrows of the current element or entry value,
not copies. They cannot be assigned, moved, or passed to `mut` or `own`
parameters; they can be read, passed to shared parameters, and copied when
their type is Copy. A collection whose elements or values hold a `mut []T` view
cannot be looped over this way, since a shared borrow gives no mutable access
through views inside it (§12.3); use a counting loop over indexes instead.
`index` and `key` are copies. Every name is a fresh binding for each
iteration, scoped to the loop, as for a counting loop's initializer.

`continue` proceeds to the next element; `break` ends the loop. Both obey the
cleanup rules below; a loop-held collection value is destroyed after `break`.

The MVP has no `range` loops over integers or channels, labelled
jumps, or `goto`. This syntax does not import additional Go loop forms or
permit `:=`, `++`, or `--`.

Before a control transfer, perform required cleanup for scopes exited by that
transfer. A `continue` cleans up iteration-local resources as required but does
not exit the counting initializer's scope; `break` exits that scope as well.
Loop-carried ownership state must be valid on every iteration. Explicit errors,
`await`, and task creation retain their ordinary semantics.

Compiler impact: lower initialization, condition, body, update, and exit to
distinct control-flow regions; resolve exits to the nearest loop in the current
function; analyze loop backedges and required drops. Lower a collection loop
to a counting loop over a shared borrow of the collection that stays live for
the whole loop, rebinding the item borrow on each iteration. Pending cases for §5.8–5.10
are in `tests/conformance/control-flow.md`.

---

# 6. Built-in Types

## 6.1 Primitive types — LOCKED

The MVP includes:

```text
bool

int
int8
int16
int32
int64

uint
uint8
uint16
uint32
uint64

float32
float64

byte
rune
string
```

## 6.2 Collection types — LOCKED

The MVP includes:

```text
[T; N]      fixed-size array
[]T         borrowed slice
Array<T>    owned dynamic array
map[K]V     owned map
```

## 6.3 Additional core semantic types — LOCKED

The language model also includes:

```text
error
channel<T>
Task<R1, ..., Rn>   (written plain `Task` when the spawned call has no results)
```

The exact generic surface syntax for every runtime/library type does not imply general user-defined generics. General-purpose generics are outside the MVP.
`Task`'s type arguments mirror the spawned call's result list (§18.8).

## 6.4 No source-level `void` requirement — LOCKED

Functions that do not return a value do not require a source-level `void` type.

Example:

```ore
func greet(name string) {
    println(name)
}
```

## 6.5 Numeric type widths and defaults — LOCKED

The fixed-width primitive types have their named widths. `int8`, `int16`,
`int32`, `int64`, `uint8`, `uint16`, `uint32`, and `uint64` have exactly 8, 16,
32, or 64 bits as their names indicate. `float32` and `float64` use IEEE 754
binary32 and binary64 formats respectively.

`int` is an alias of `int64`; `uint` is an alias of `uint64`; and `byte` is an
alias of `uint8`. Their widths do not depend on the host architecture. `rune`
is a distinct type representing exactly one Unicode scalar value, not an alias
of an integer type. Its valid values are U+0000–U+10FFFF excluding U+D800–U+DFFF.
Conversions to or from integer types must follow §6.6.

Integer literals have no suffixes (§3.14). When a literal has no expected type,
integer literals default to `int` and floating-point literals default to
`float64`. A numeric literal may take an expected integer or float type when
its value is representable in that type as defined in §6.7. Otherwise the
program is rejected; the compiler must not silently truncate or wrap a
constant to make it fit, and rounds a float constant only as §6.7 allows. A rune literal has type `rune`, not the
default integer type. Contextual typing does not make differently typed
variables implicitly compatible.

For untyped constant arithmetic, preserve the exact mathematical result until
the expression is assigned a type. Reject a constant expression if its result
is not representable in the required type. Untyped constant kinds, operators,
representability, and required precision are specified in §6.7. Detailed constant-expression syntax
remains restricted to operations whose operands and results are themselves
valid constants; calls, mutation, I/O, and other runtime effects are not constant
operations.

```ore
let count = 42             // int, which is int64
let small uint8 = 42       // contextual type is representable
let ratio = 1.5            // float64
let precise float32 = 0.5  // representable (§6.7)
let letter rune = 'A'
```

Ownership, error, and async implications: all numeric primitives and `rune` are
Copy. Width/default rules do not change cleanup, error propagation, or async
ownership. Invalid contextual literal values produce compile-time diagnostics.

Compiler impact: keep literal magnitudes/decimal values independent of host types,
apply expected types deliberately, and make `int`, `uint`, and `byte` aliases of
their specified widths in type identity/layout. Preserve a distinct rune type
and validate scalar range. Pending cases are in `tests/conformance/numerics.md`.

## 6.6 Numeric conversions and arithmetic — LOCKED

Implicit conversion between already typed numeric values is not allowed. Numeric
conversion uses a type name as a conversion form, such as `int64(value)` or
`float32(value)`. Conversions never silently change ownership; all numeric values
remain Copy.

Integer-to-integer and float-to-integer conversions are checked. A constant
conversion requires the constant to be representable in the destination type
(§6.7); otherwise it is a compile-time error. In particular, a constant with a
fractional part cannot be converted to an integer type: `int64(2.5)` is
invalid, while `int64(2.0)` is the integer 2. A runtime float-to-integer
conversion truncates toward zero and then checks the destination range; a
runtime value outside the range panics. NaN and infinities are invalid for
integer conversion. Integer-to-float and float narrowing use
round-to-nearest, ties-to-even. A finite source value outside the destination
float's finite range is an error at compile time for constants and a runtime
panic otherwise. Rounding a representable finite value to the nearest destination
value is permitted, including rounding to a subnormal or signed zero.

**Rune conversions — LOCKED.** A `rune` converts to and from integer types with
the same conversion form. `T(r)` for an integer type `T` gives the rune's
scalar value, checked like any integer conversion: `uint8('é')` is `233`, and a
runtime value outside `T` panics. `rune(n)` for an integer `n` gives the rune
with that scalar value; a runtime value that is negative, above `0x10FFFF`, or
in the surrogate range `0xD800`–`0xDFFF` panics. With a constant operand,
either conversion is evaluated at compile time, and a value that does not fit
or is not a scalar is a compile-time error. `rune(r)` of a rune is that rune.
A rune does not convert to or from a float type or `bool`, and the only other
conversion involving a rune is `string(r)` (§6.8).

```ore
let code = int('A')          // 65
let next = rune(code + 1)    // 'B'
let byteValue = uint8('é')   // 233
let bad = rune(0xD800)       // invalid: a surrogate is not a scalar value
let wide = float64('a')      // invalid: convert through an integer type
```

An explicitly converted integer value and a float value may be combined only
when their types match; there is no automatic common numeric type. Comparisons
require matching types after aliases are resolved. `==` and `!=` are permitted
for bool, numeric primitives, rune, string, and `error` (§15.1 defines `error`
equality). Ordered comparisons `<`, `<=`, `>`, and `>=` are permitted for
numeric primitives, rune, and string. Strings are ordered lexicographically by
their UTF-8 encoding bytes. Booleans support equality only. `Task<...>`
supports only equality against `nil` (§41.4), not general equality.
`channel<T>` is not comparable at all, including against `nil`, since channels
have no `nil` state (§19.12). Other types are not comparable unless a later
specification explicitly adds that capability.

Integer `+`, `-`, `*`, `/`, and `%` use checked arithmetic. A result
outside the destination type's range is a compile-time error for a constant
expression and a runtime panic for a runtime expression, in every build mode.
Integer division truncates toward zero; `%` satisfies `a == (a / b) * b + (a % b)`
for valid operands, so a nonzero remainder has the dividend's sign. Division
or remainder by zero panics at runtime and is a compile-time error in a constant
expression. The minimum signed integer divided by `-1` is an overflow under this
rule.

Bitwise `&`, `|`, `^`, and unary `^` operate on the fixed-width integer bit
representation. **Fixed-width shifts — LOCKED:** for a typed left operand of
width W, left shift keeps the low W bits and fills vacated low bits with zeros.
Discarded high bits do not cause an overflow error or panic. Signed operands
use two's-complement bit patterns; interpret the resulting W bits in the left
operand's signed or unsigned type. Right shift of unsigned types fills with
zero bits; right shift of signed types replicates the sign bit. These rules
apply identically to typed constant expressions and runtime expressions.

Shift counts must be nonnegative and less than the left operand's bit width;
invalid constant counts are compile-time errors, and invalid runtime counts
panic. Counts are never masked or reduced modulo the width. The shift-count
operand must have an integer type or be an untyped constant representable as an
integer (§6.7), so `1 << 3.0` is valid and `1 << 3.5` is not. The
left operand determines the result type; the right operand is not implicitly
converted to it. A zero count preserves the operand. Compound shifts use these
same rules and the assignment evaluation order (§5.6).

An untyped constant left operand must be representable as an integer (so
`1.0 << 3` is the untyped integer 8); shifting it yields an untyped integer
constant. Untyped constant shifts preserve exact mathematical values under §6.5: left
shift multiplies by 2 to the count's power; right shift divides by that power,
rounding toward negative infinity. They do not discard high bits before a type
has been assigned. Counts must be nonnegative; when the expression receives an
expected or default integer type, its counts must also be less than that type's
width and its exact result must fit. An explicit conversion of the left operand
before shifting makes the shift typed and therefore uses the fixed-width rule.
Contextual typing of a literal operand similarly supplies its width before a
typed shift. Context must not retroactively truncate a previously evaluated
untyped named constant.

```ore
let wrapped = uint8(128) << 1   // uint8 zero; discarded high bit is not overflow
let signed = int8(64) << 1     // int8 -128, from its two's-complement bit pattern
let negative = int8(-2) >> 1   // int8 -1, sign-preserving right shift
let invalid = uint8(1) << 8    // compile-time error: count equals width
const wide = 128 << 1          // exact untyped constant 256
let tooSmall uint8 = wide      // compile-time error: exact value does not fit
```

Floating-point arithmetic uses IEEE 754 binary32 or binary64 operations with
round-to-nearest, ties-to-even, without fast-math reassociation. Runtime
operations may produce positive/negative infinity, signed zero, or NaN according
to IEEE 754; these results are not panics by themselves. Constant floating-point
evaluation that overflows the finite range of its required float type is a
compile-time error. Underflow and inexact rounding follow IEEE 754, including
subnormals and signed zero. Floating-point division by zero follows IEEE 754 and
may produce infinity or NaN; it does not panic. `%` is integer-only.

String `+` concatenates strings, with the ordinary Copy semantics of `string`.
No implicit number-to-string conversion or operator overloading is introduced.

Ownership, error, and async implications: arithmetic and conversions on numeric
values preserve Copy semantics. Runtime panics use the language's panic behavior;
they are not ordinary error results and do not add hidden exception propagation.
Async execution follows the same numeric rules and ownership model.

Compiler impact: preserve exact untyped constant evaluation before range
checking; evaluate typed shifts with their specified width and signedness,
including at compile time. Emit checked arithmetic and conversions consistently
across build modes; validate shift counts before emitting shifts, without
arithmetic-overflow checks for discarded bits or backend assumptions that signed
left shift cannot overflow. Use explicit IEEE operations without unsafe
fast-math; and implement
the specified comparison eligibility. Diagnostics identify source spans for
constant failures; runtime panics identify invalid operations when available.
Pending cases are in `tests/conformance/numerics.md`.

## 6.7 Untyped constants — LOCKED

Untyped numeric constants follow the Go programming language's constant model
(Go specification, "Constants", "Representability", and "Constant
expressions", checked against language version go1.27). Zore differs only
where stated below.

### Kinds

An untyped numeric constant has one of two kinds:

- **untyped integer**: integer literals (§3.11) and constant expressions whose
  untyped operands are all untyped integers;
- **untyped float**: floating-point literals (§3.13) and constant expressions
  with at least one untyped float operand.

If the untyped operands of a binary operation other than a shift have
different kinds, the result is an untyped float. Unlike Go, Zore has no untyped
rune, boolean, string, or complex constants: rune literals have type `rune`
(§6.5), and string and boolean literals have types `string` and `bool`. An
untyped constant cannot take the type `rune`, because `rune` is not an integer
type (§6.5).

Untyped constants have default types `int` (untyped integer) and `float64`
(untyped float), used when no expected type applies (§6.5).

### Exact evaluation and required precision

Untyped numeric constants denote exact values of arbitrary precision and do
not overflow. No constant denotes negative zero, infinity, or NaN. Constant
expressions are evaluated exactly; intermediate results may need far more
precision than any predeclared type.

An implementation may use limited internal precision, but it must:

- represent integer constants with at least 256 bits;
- represent float constants with a mantissa of at least 256 bits and a signed
  binary exponent of at least 16 bits;
- report a compile-time error if it cannot represent an integer constant
  exactly;
- report a compile-time error if it cannot represent a float constant because
  of overflow;
- round to the nearest representable constant if it cannot represent a float
  constant because of limited precision.

These requirements apply to literals and to the results of constant
expressions. Such rounding may make a float constant expression non-integral
in an integer context, or integral when it would not be at infinite precision.

### Operators on untyped constants

An operation whose operands are all untyped constants yields an untyped
constant. The operators of §7.6 apply as follows:

| Operator | Untyped integer | Untyped float |
| --- | --- | --- |
| `+`, `-`, `*`, unary `+`, `-` | Exact result | Exact result |
| `/` | Quotient truncated toward zero | Exact quotient |
| `%` | Remainder with the dividend's sign, so `x == (x / y) * y + x % y` | Invalid |
| `&`, `\|`, `^`, unary `^` | Infinite-precision two's complement; unary `^x` equals `-1 ^ x`, that is `-x - 1` | Invalid |
| `<<`, `>>` | See §6.6; operands must be representable as integers | Left operand must be integral; the result is an untyped integer |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | `bool` result | `bool` result |

A zero divisor in a constant `/` or `%` is a compile-time error. Operations
on typed constants use the typed rules of §6.6: a result that is not
representable in the type is a compile-time error, and `^` uses the type's
width.

```ore
const a = 2 + 3.0          // untyped float 5.0
const b = 15 / 4           // untyped integer 3 (truncated division)
const c = 15 / 4.0         // untyped float 3.75
const d = -7 % 3           // untyped integer -1
const e = ^1               // untyped integer -2
const f = -4 | 1           // untyped integer -3
const g = 1 << 3.0         // untyped integer 8
const h = 1.0 << 3         // untyped integer 8
const huge = 1 << 100      // exact untyped integer
const four int8 = huge >> 98   // 4, of type int8
const half float64 = 3 / 2     // 1.0: 3 / 2 is integer division
const exact float64 = 3 / 2.0  // 1.5
```

```ore
const bad1 = 1 / 0         // invalid: division by zero
const bad2 = 7.5 % 2       // invalid: % needs integers
const bad3 = 1.5 & 1       // invalid: bitwise operators need integers
const bad4 = 1 << 3.5      // invalid: count is not an integer
```

### Representability

A constant `x` is representable in type `T` when:

- `T` is an integer type and `x` is an integer value within `T`'s range. An
  untyped float with no fractional part qualifies: `let n uint8 = 42.0` is
  valid, while `let n int = 1.1` is not.
- `T` is a float type and `x` can be rounded to `T`'s precision without
  overflow. Rounding uses IEEE 754 round-to-nearest, ties-to-even, and a
  rounded negative zero becomes positive zero. `let f float32 = 0.1` is valid
  (the nearest `float32`); `let f float64 = 1e1000` is invalid because it
  overflows.

Wherever an untyped constant receives a type (an explicit or expected type,
a default type, a typed operand, a conversion, an argument, a field, or a
return value), it must be representable in that type. Otherwise the program
is rejected at compile time. A constant conversion follows the same rule
(§6.6): `uint8(-1)`, `int64(3.14)`, and `int64(huge)` are invalid.

```ore
let precise float32 = 2.718281828459045   // rounds to the nearest float32
let tiny float64 = -1e-1000               // rounds to 0.0 (positive zero)
let whole uint8 = 42.0                    // integer 42
let big uint64 = 1e10                     // integer 10000000000
let bad int = 1.1                         // invalid: not an integer value
let small uint8 = 1024                    // invalid: out of range
```

Ownership, error, and async implications: constants are pure compile-time
values with Copy semantics; this section adds no runtime operation, error
result, or async behavior. Every failure described here is a compile-time
error.

Compiler impact: evaluate untyped constants with arbitrary-precision integer
and float arithmetic meeting the minimums above, track each constant's kind,
apply representability whenever a constant is typed, and report
kind-specific operator errors and division by zero with source spans. Keep
constant evaluation separate from runtime arithmetic, which still follows
§6.6. Pending cases are in `tests/conformance/constant-expressions.md` and
`tests/conformance/numerics.md`.

## 6.8 String operations — LOCKED

A `string` is immutable well-formed UTF-8 (§41.5). Its length, indexing, and
slicing are measured in **bytes**, as in Go; looping visits whole characters.

```ore
let word = "héllo"
println(word.len())        // 6: `é` takes two bytes
println(word[0])           // 104, a `byte`
println(word[1:3])         // é
for i, ch in word {        // i is a byte index, ch is a rune
    println(i)
}
let joined = word + "!"    // runtime concatenation
println(string('é'))       // é
```

- `s.len()` returns the number of bytes as an `int` (§12.7).
- `s[i]` returns the byte at index `i` as a `byte` (`uint8`). The index follows
  the array indexing rules (§12.6): any integer type or untyped constant, with
  a runtime panic for an index below zero or at or above the length. A string
  is immutable, so `s[i]` cannot be assigned to, compound-assigned, or passed to
  a `mut` parameter.
- `s[low:high]` returns a `string`. Omitted bounds default to `0` and the
  length, and the bounds follow the slicing rules of §12.6 (`0 <= low <= high
  <= len`). In addition, `low` and `high` must each fall on a character
  boundary, that is, equal the length or index the first byte of a UTF-8
  sequence; otherwise the operation panics, so a slice never splits a
  character and is always well-formed UTF-8. Constant bounds that are negative
  or reversed are compile-time errors. Because strings are immutable Copy
  values, the result is an ordinary string value: it creates no borrow, is
  never `mut`, and may share storage with its source.
- `for ch in s` and `for i, ch in s` visit each character in order (§5.10).
  `ch` is a `rune` copy, never a borrow; `i` is the byte index at which that
  character starts, an `int`. The string is evaluated once before the loop.
  The one-name form binds the character, not its index.
- `+` concatenates two strings at run time as well as in constant expressions
  (§7.6), producing a new string. `+=` applies the same rule. Neither operand
  changes. An implementation may share storage between values and defines when
  runtime-built storage is released (§41.5); no source-level observation
  depends on it.
- `string(r)` converts a `rune` to the one-character string holding its UTF-8
  encoding. It is the only string conversion: no other type converts to
  `string`, `string(s)` of a string is rejected, and there is no implicit
  number-to-string conversion. Converting numbers to text, and text to
  numbers, is the job of `zore/strconv` (§37.2).
- `==`, `!=`, `<`, `<=`, `>`, `>=` compare by bytes (§6.6).

Ownership, error, and async implications: `string` stays Copy and no operation
here consumes or borrows its operands. Indexing and slicing panics are ordinary
runtime panics (§15.4) with the usual cleanup. Nothing suspends.

Compiler impact: type `len`, index, slice, loop, and `string(rune)` as above;
lower slices to a checked operation that validates bounds and character
boundaries; lower concatenation and rune-to-string conversion to runtime
calls. Pending conformance cases: `tests/conformance/strings.md`.

---

# 7. Functions

## 7.1 Basic function syntax — LOCKED

```ore
func greet(name string) {
    println(name)
}
```

A return type follows the parameter list when present.

Example:

```ore
func add(a int, b int) int {
    return a + b
}
```

## 7.2 Multiple return values — LOCKED

Multiple return values are part of the MVP because they are required by the explicit error model.

Example:

```ore
func readFile(path string) (string, error)
```

A function, method, or closure's result list may contain at most one
`error`-typed result, and if present it must be the last result. This
generalizes the pattern already used throughout this specification —
`(string, error)`, `task.wait() -> (T, error)` (§18.7) — into a general rule:
error-bearing results are always trailing and singular. A result list with
`error` in a non-final position, or with more than one `error`-typed result,
is invalid. This rule enables the `?` typing and zero-value-fill behavior in
§15.2.

## 7.3 Parameter ownership syntax — LOCKED

Zore uses the following parameter forms:

```ore
func read(user User)
func update(user mut User)
func save(user own User)
```

Their meanings are:

| Form | Meaning |
|---|---|
| `user User` | shared / immutable borrow by default |
| `user mut User` | mutable borrow |
| `user own User` | ownership transfer |

This syntax is a core locked Zore decision.

## 7.4 Call-site syntax — LOCKED

Call sites remain clean:

```ore
read(user)
update(user)
save(user)
```

There is no `mov`, `move`, `borrow`, or equivalent ownership keyword required at the call site.

Ownership behavior is determined from:

- the callee parameter contract
- the argument type
- compiler ownership analysis

---

## 7.5 Expression evaluation order — LOCKED

Expression operands and call arguments are evaluated from left to right in
source order, subject to explicitly specified conditional evaluation such as
short-circuiting. Each evaluated subexpression completes its evaluation before
the next begins. This rule does not change operator precedence or associativity;
those determine the expression tree as specified in §7.6.

For a call, evaluate the callee expression (including a method receiver) first,
then each argument from left to right, then invoke the callee. Evaluation does
not itself imply a copy, move, or borrow independent of the callee's contract.

```ore
combine(first(), second())
```

In this example, evaluate `first()` before `second()` and invoke `combine` only
after both arguments have been evaluated. This applies even when only one
operand has an obvious side effect: optimizations must preserve the observable
behavior defined by this ordering.

If evaluating an earlier subexpression exits the current computation, later
subexpressions are not evaluated. For example, in
`combine(first()?, second())`, an error propagated by `first()?` prevents
`second()` and `combine` from running. Merely returning an error value without
propagating it does not itself skip later evaluation.

Explicit suspension in an earlier subexpression preserves this order. In
`combine(await first(), second())`, `second()` is not evaluated until the await
has completed successfully and control reaches it. Other tasks may run while
the current computation is suspended. Evaluating a task creation or an async
value does not introduce an implicit wait: completion of expression evaluation
does not necessarily mean completion of work represented by its result.

Conditional evaluation rules take precedence over evaluating all operands.
The short-circuit operators in §7.6 evaluate their right operand only when
required. Left-to-right evaluation does not
introduce parallel argument evaluation or eager evaluation of skipped operands.

Ownership, error, and async implications: ownership analysis must respect the
specified evaluation sequence, reject later uses invalidated by earlier moves,
and preserve valid borrows across any explicit suspension. Ordering alone does
not grant overlapping borrows or settle exact borrow activation/duration rules.
If earlier argument evaluation creates owned temporaries and a later argument
propagates an error, required temporary cleanup must occur before returning.
Panic behavior remains subject to its separately specified cleanup contract.

Compiler impact: lower evaluations in the specified sequence, retaining source
spans and explicit control flow for early exits and suspension. Preserve ordinary
ownership and drop analysis rather than relying on host-language evaluation
order. Assignment target/RHS/store ordering and multiple assignment follow §5.6;
struct initializer sequencing follows §8.4; array initializer sequencing follows
§12.6 and map initializer sequencing follows §13.3. Pending cases are
in `tests/conformance/evaluation-order.md`.

## 7.6 Operators and expression grouping — LOCKED

The MVP supports these operators:

| Category | Operators and constraints |
| --- | --- |
| Arithmetic | Binary `+`, `-`, `*`, `/`, `%`; `%` is integer-only |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=`; comparisons cannot chain |
| Boolean logic | Unary `!`, binary `&&`, `||`; operands must be `bool` |
| Bitwise | Binary `&`, `\|`, `^`, `<<`, `>>`, unary `^` for complement; integers only |
| Unary signs | `+x`, `-x`; signs are operators, not numeric literal token contents |
| Strings | Binary `+` concatenates strings; no implicit number-to-string conversion |
| Error propagation | Postfix `?`, subject to §15 and the await rule below |
| Suspension | Prefix `await`, subject to §17 |

Comparison operations yield `bool`. Detailed comparable/ordered type rules,
mixed numeric type compatibility, integer division/remainder behavior, overflow,
and shift-count limits remain type-model decisions. This operator inventory does
not add user-defined operator overloading or implicit numeric conversions.

### Precedence and associativity

The following table is ordered from highest to lowest precedence:

| Level | Forms |
| --- | --- |
| 1 | Calls, field access, indexing, slicing |
| 2 | Postfix `?` |
| 3 | Prefix `+`, `-`, `!`, `^`, `await` |
| 4 | `*`, `/`, `%`, `<<`, `>>`, `&` |
| 5 | `+`, `-`, `\|`, `^` |
| 6 | `==`, `!=`, `<`, `<=`, `>`, `>=` |
| 7 | `&&` |
| 8 | `\|\|` |

Binary operators at the same level associate left to right, except that
comparison operators are non-associative. Parentheses override grouping.
Consecutive prefix operators apply from the operand outward. Calls, fields, and
indexing/slicing form left-to-right access/call chains (§12.6). These grouping
rules do not
change left-to-right evaluation of the resulting operands (§7.5).

For example:

```text
a + b * c       groups as a + (b * c)
a - b - c       groups as (a - b) - c
a + b << c      groups as a + (b << c)
a | b ^ c       groups as (a | b) ^ c
a == b || c     groups as (a == b) || c
```

An unparenthesized comparison chain such as `a < b < c`, `a == b == c`, or
`a < b == c` is rejected. Explicitly grouped comparisons remain subject to
ordinary type checking; parentheses do not make incompatible operands legal.
Write `a < b && b < c` to express two ordered comparisons when appropriate.

### Boolean evaluation

`!` negates a boolean. `left && right` evaluates `left` first and evaluates
`right` only if `left` is true. `left || right` evaluates `right` only if `left`
is false. There is no implicit truthiness conversion from integers, strings,
collections, or other types. A skipped operand performs no side effects, moves,
borrows, error propagation, or suspension at runtime. Both operands must still
be well-typed, and ownership analysis must account for the conditional path.

### Await and propagation grouping

There is an explicit exception to the generic postfix/prefix precedence table:
an unparenthesized trailing `?` on an awaited operation propagates the result
after awaiting. In particular:

```text
await operation()?       means (await operation())?
await object.method()?   means (await object.method())?
```

The same grouping applies to awaiting a task handle (§18.9): `await task?`
means `(await task)?`.

The parser must construct propagation around the await expression in these
forms, not await a prematurely propagated result. Explicit parentheses control
grouping: `await (operation()?)` instead places propagation inside the awaited
operand and is accepted only if the resulting types support that operation.
This is not permission to invent an async-result type or relax `?` typing.

### Assignment, task creation, and exclusions

Assignment is a statement and does not produce an expression value. Chained
assignment such as `a = b = value` is not supported. Assignment targets, compound
assignment, and sequencing are specified in §5.6; this table does not add
assignment expressions.

`go` takes a call expression, consistent with §18. It does not act as an ordinary
arithmetic unary operator. Its complete grammar and interaction with surrounding
expressions remain for the task rules; do not infer them from precedence.

The MVP excludes increment/decrement (`++`, `--`), ternary `?:`, exponentiation,
and source-level address-of or pointer-dereference operators. Binary `&` is
bitwise AND, and binary `*` is multiplication; neither introduces pointers.
Do not inherit other operators such as `&^` from another language. Lexical token
handling must distinguish adjacent prefix signs from unsupported increment or
decrement syntax; it must not silently reinterpret `++x` or `--x` as supported
increment/decrement operations.

Ownership, error, and async implications: preserve normal Copy/Move and borrow
rules for operands. Short-circuiting creates control-flow paths whose live
values and required drops must be analyzed separately. `await operation()?`
preserves suspension safety and performs required cleanup if the awaited error
propagates. No hidden exceptions, implicit waits, or source pointers are added.

Compiler impact: encode precedence and non-associative comparisons explicitly,
retain source spans, implement the await/propagation grouping rule in the parser,
and lower short-circuit logic to conditional control flow. Keep type checking
and ownership analysis separate from parsing. Pending cases are recorded in
`tests/conformance/expressions.md`.

---

## 7.7 Return statements and completion — LOCKED

A no-result function may use bare `return` or complete normally at the end of
its body. A result-returning function uses `return expression` for one result
or `return expression, expression` for multiple results, matching the declared
result count and types. Evaluate return operands left to right (§7.5), transferring
returned values according to ordinary ownership rules.

```ore
func add(a int, b int) int {
    return a + b
}

func pair() (int, int) {
    return 1, 2
}
```

Named return parameters and implicit bare returns of named results are not
supported. A bare `return` in a result-returning function is invalid. Returning
values from a no-result function is invalid. Multiple-result forwarding follows
§7.8. Error-propagation typing is locked in §15.2, including result-list shape
(§7.2) and zero-value fill on an early `?` return.

Every reachable path in a result-returning function must return the required
results or never complete. A provably non-completing path, such as an infinite
loop without a reachable exit, does not require an artificial return. A loop
that can exit does not by itself prove completeness. Reject possible fallthrough
without required results. A statement that is a call of the predeclared `panic`
(§15.4) never completes, so it ends a path the same way `return` does.

A return exits the current function or closure, not an enclosing function. It
performs required cleanup of still-owned values whose lifetimes end on that
path, without destroying values transferred to the caller. If evaluating a return
operand propagates an error, later operands are skipped and required temporary
cleanup occurs. In an async function, return completes that async computation;
it does not weaken ownership or suspension safety.

Compiler impact: type-check return counts/contracts, analyze reachable completion
paths, retain source spans, and lower returned values separately from cleanup.
Pending cases are in `tests/conformance/control-flow.md`.

## 7.8 Function declarations, calls, and result forwarding — LOCKED

Each parameter declares its own name and type, with the ownership modifier in
the position defined in §7.3. For example, `a int, b int` is valid; grouped
`a, b int` is not. Arguments are positional and their count must match the
declared parameters. The MVP has no default parameters, named arguments, or
user-defined variadic parameter syntax. Type compatibility and ownership checks
apply after matching arguments to parameters.

A trailing comma is allowed in parameter and argument lists. On multiline lists,
commas are required where necessary to avoid semicolon insertion (§3.7). This
does not permit missing parameters or arguments between commas.

```ore
func add(a int, b int,) int {
    return a + b
}

let total = add(
    first(),
    second(),
)
```

User-defined functions and methods require bodies. Signature-only examples
elsewhere in this specification illustrate contracts; they do not authorize
bodyless user declarations, FFI declarations, or forward-declaration syntax.
Package functions and types may reference later declarations in the same package,
including across its files. Collect declaration identities before resolving
bodies. This does not settle package variable initialization order or permit
recursive by-value layouts without a valid type/layout model.

There is no overloading by parameter count, parameter types, return types, or
ownership modifiers. Duplicate function names in one package are invalid.
Methods may be declared only on types defined in the declaring package. Within
one receiver type, method names must be unique and must not collide with field
names. Different receiver types may have methods with the same name.

### Result forwarding

`return pair()` may forward all results from a single expression when their
count and types match the enclosing function's return contract. Evaluate that
expression once, then return its results with ordinary ownership transfer and
cleanup. Forwarding an error as a declared result is an explicit use, not a
silently ignored error (§15.6).

A return may alternatively list separate single-result expressions. Do not
splice a multiple-result expression into a list with other expressions. Call
arguments never implicitly expand multiple results: bind them first, then pass
the named values. An ordinary single-result call as an argument remains valid.

```ore
func pair() (int, int) {
    return 1, 2
}

func forward() (int, int) {
    return pair()
}

func main() {
    let left, right = pair()
    let total = add(left, right)
}
```

### Expression statements

Calls and task creation may be used as statements, including call-based forms
with explicit `await` or `?` where their contracts permit. Unused arithmetic,
comparisons, bare identifiers/literals, and other bare value expressions are
not expression statements. Use an explicit discard assignment when a value is
intentionally discarded. Ignored non-error call results still require ordinary
cleanup; any unused error result requires explicit discard or use (§15.6).
Call-statement permission does not implicitly await an async operation.

Ownership, error, and async implications: declaration order and forwarding do
not weaken parameter contracts or return-lifetime checks. Argument evaluation
remains left to right (§7.5); returned Move values are transferred, not duplicated
or destroyed locally. Invalid signatures, call counts, member conflicts, and
unsupported statement forms produce compile-time diagnostics. Async functions
and methods obey the same rules; runtime/task contracts remain separately defined.

Compiler impact: validate individual parameter syntax and list separators,
register package-level function/type identities before resolving bodies, enforce
receiver ownership and member uniqueness, and distinguish result forwarding from
argument expansion. Built-in callable signatures remain part of the explicit
predeclared API inventory; do not invent them from user-function syntax.
Pending cases are in `tests/conformance/functions-structs.md`.

---

# 8. Structs

## 8.1 Struct declaration — LOCKED

```ore
type User struct {
    Name string
}
```

## 8.2 Struct initialization — LOCKED

```ore
let user = User{
    Name: "John",
}
```

## 8.3 Struct ownership classification — LOCKED

A struct's Copy/Move behavior is derived from its fields.

Rule:

> If all fields are Copy, the struct is Copy.\
> If any field is Move, the struct is Move.

Example:

```ore
type Point struct {
    X int
    Y int
}
```

`Point` is Copy.

Example:

```ore
type Session struct {
    socket Socket
}
```

If `Socket` is Move, then `Session` is Move.

The programmer does not manually annotate the struct as Copy or Move in the MVP.

A struct that defines a custom `drop` method (§14.3) is **always Move**,
overriding the field-derived rule above even when every field is Copy.
Silently duplicating a value with meaningful cleanup logic would run that
cleanup once per duplicate against what is really one underlying resource,
which is unsound; requiring Move for any `drop`-bearing type closes that gap.
This is a consequence of defining `drop`, not a manual annotation.

---

## 8.4 Struct construction requirements — LOCKED

Struct initializers use named fields only, with every field supplied exactly
once. Reject positional construction, omitted fields, unknown fields, and
duplicate field names. No omitted-field zero initialization or field defaults
are introduced by this syntax. An empty struct may use an empty initializer.
Fields may be written in any order, and a trailing comma is allowed (§3.7).

```ore
type User struct {
    Name string
    age int
}

let user = User{
    Name: loadName(),
    age: loadAge(),
}
```

Evaluate field expressions from left to right in their written order, not field
declaration order. Evaluate each expression once and require its type to match
the named field under the type model. Normal Copy/Move rules determine field
initialization ownership. If a later field expression exits via propagated error,
clean up previously acquired owned field values as required; never destroy a
field whose value was not initialized. Preserve §14's applicable cleanup order.

A caller may not name another package's private fields. Because all fields are
required, constructing such a type outside its package requires a constructor
function in the defining package (or another API returning a valid instance).
There is no implicit constructor bypass of visibility.

For parsing clarity, a struct literal used directly within an `if` or `for`
condition must be enclosed in parentheses so its opening brace cannot be confused
with the body. This applies to literals appearing as operands within the condition;
parenthesizing the whole containing condition also encloses the literal. For
example, `if (Point{X: 1}).X == 1 { ... }` has an explicit literal boundary.
The same requirement applies to the condition clause of a counting loop.

Ownership, error, and async implications: construction does not change automatic
struct Copy/Move classification (§8.3). Previously acquired field values must
remain valid across a later explicit await, with ordinary borrow and drop rules.
Partial construction on an error path must not leak or double-drop resources.
No implicit error propagation or additional concurrency is introduced.

Compiler impact: resolve named fields to stable IDs, enforce exactly-once complete
initialization and visibility, preserve source evaluation order independently of
layout order, and represent initialization progress for ownership/drop lowering.
Reject ambiguous unparenthesized condition literals. Pending cases are in
`tests/conformance/functions-structs.md`.

## 8.5 Named types — LOCKED

A type declaration may name a predeclared type instead of declaring a struct:

```ore
type Duration int
type Name string
type Seconds Duration
```

`type Name Base` declares a new type, distinct from `Base` and from every other
type, whose values, representation, and operations are those of `Base`. `Base`
is `bool`, an integer or float type, `rune`, `string`, or another named type;
the predeclared type at the end of that chain is the named type's *base type*.
A declaration that names a struct, collection, function, task, channel, mutex,
or `error` type, or a chain of named types that leads back to itself, is an
error.

- *Operations.* The operators, comparisons, indexing, slicing, `len()`, and
  `for ... in` loops of the base type apply, and give the named type wherever the
  base operation gives the operand type: `d * 2` is a `Duration`, `d > e` is a
  `bool`, slicing a `Name` gives a `Name`, and looping over a `Name` gives
  `rune` characters. A value whose base type is `bool` can be the condition of
  `if` and `for`.
- *No implicit conversion.* A named type never mixes with another type,
  including its base: `d + x` for a `Duration d` and an `int x` is an error.
  An untyped constant takes the named type the same way it takes its base type
  (§6.7), so `5 * Second` is a `Duration`. Literals of `bool`, `rune`, and
  `string` are typed (§3.8–§3.10) and need a conversion: `Name("zore")`.
- *Conversions.* `T(v)` converts between a named type and any type with the
  same base type, and between named and predeclared types wherever the base
  types convert (§6.6): `Duration(n)` for any integer or float `n`, `int(d)`,
  `string(name)`, `Name(s)`, and `rune(letter)`. A conversion between types with
  the same base type never fails and does not change the value.
- *Methods.* A named type may have methods (§9.1), declared in its package.
  The base type's operations are not methods, and a named type does not get its
  base type's methods or another named type's methods.
- *Constants.* A typed constant may have a named type: `const Second Duration =
  1000000000`.
- *Zero value.* The zero value of a named type is that of its base type
  (§41.4).

```ore
type Duration int

const Millisecond Duration = 1000000

func (d Duration) Milliseconds() int {
    return int(d / Millisecond)
}

func main() {
    let d = 250 * Millisecond
    println(d.Milliseconds())       // 250
    let raw int = int(d)            // conversion to the base type
    let bad = d + raw               // invalid: Duration and int do not mix
}
```

Ownership, error, and async implications: a named type is Copy, like every base
type it can have, and has no custom `drop` or `clone`. It adds no runtime
representation or cost.

Compiler impact: give each named type its own type identity whose
representation and operator rules come from its base type; resolve bases before
other declarations and diagnose cycles; key methods by receiver type. Pending
conformance cases: `tests/conformance/functions-structs.md`.

---

# 9. Methods

## 9.1 Method syntax — LOCKED

Methods use receiver syntax:

```ore
type User struct {
    Name string
}

func (user User) greet() {
    println(user.Name)
}

func (user mut User) rename(name string) {
    user.Name = name
}

func (user own User) save() {
    // consumes user
}
```

Receiver semantics follow the same ownership model as function parameters:

- `receiver Type` -> shared borrow
- `receiver mut Type` -> mutable borrow
- `receiver own Type` -> ownership transfer

There is no separate method ownership model.

The receiver type is a struct type or a named type (§8.5) declared in the same
package. Custom `drop` and `clone` methods (§14.3, §10.7) may be declared only
on struct types.

A method used as a value, `value.Method` without a call, is a function value
defined in §16.2.

---

# 10. Copy and Move Semantics

## 10.1 Assignment rule — LOCKED

Given:

```ore
let b = a
```

the result depends on the type of `a`.

- if the type is Copy, `a` is copied
- if the type is Move, ownership transfers from `a` to `b`

## 10.2 Copy types — LOCKED

Primitive value types are Copy.

This includes:

- `bool`
- signed integer types
- unsigned integer types
- floating-point types
- `byte`
- `rune`

`string` behaves as Copy from the programmer's perspective.

The runtime representation of `string` is an implementation detail as long as observable Copy semantics remain correct and safe.

## 10.3 Move types — LOCKED

Resource-owning values are Move.

Examples include:

- `File`
- `Socket`
- `Connection`
- `Array<T>`
- `map[K]V`
- other resource-owning values

A `Mutex<T>` handle is not Move: like a channel handle it is Copy and shares
one guarded cell (§20.2). An owned interface value is Move whatever it holds
(§22.2).

## 10.4 Fixed arrays — LOCKED

A fixed array follows the ownership semantics of its element type.

Conceptually:

- `[T; N]` is Copy if its contents are Copy and the compiler supports copying that array value
- `[T; N]` is Move if ownership of contained values requires Move semantics

Exact implementation thresholds for large values are not part of source-language semantics.

## 10.5 Dynamic arrays — LOCKED

`Array<T>` owns its storage.

Therefore `Array<T>` is a Move type.

## 10.6 Maps — LOCKED

`map[K]V` owns its map storage.

Maps are Move values.

## 10.7 Explicit cloning — LOCKED

Independent duplication of a value that would otherwise be Move uses:

```ore
clone(value)
```

`clone` is explicit. It takes a shared borrow of its argument and never
consumes it: `a` remains valid after `clone(a)`. Clone availability does not
waive borrow rules: cloned shared views retain backing provenance, and a
structural clone cannot create a mutable reborrow from its shared argument
(§11.7, §12.3). Copy fields remain subject to these restrictions.

A generic `copy(value)` operation was discussed but was **not** locked. Do not introduce it as an MVP language feature without a specification change.

### Clone availability

A struct or `[T; N]` supports `clone(value)` through the applicable rule below;
when both custom and structural cloning would be eligible, select custom clone:

1. **Structural default:** a compiler-synthesized field-wise (or
   element-wise) clone, available when every field/element is Copy or itself
   clonable by this same rule, recursively — **and the type defines no custom
   `drop` method.**
2. **Custom clone:** an explicit `func (c Type) clone() Type` method (a
   shared receiver; it takes no parameters and returns exactly the receiver's
   own type). A custom `clone` takes precedence over the structural default
   when both would otherwise apply, and is the only option available to a
   type that defines a custom `drop` (§14.3) — a `drop`-bearing type is not
   structurally clonable, for the same reason it is not Copy (§8.3):
   duplicating a resource handle field-by-field does not duplicate the
   resource it refers to.

A type meeting neither condition — most commonly a `drop`-bearing type with
no custom `clone` — makes `clone(value)` a compile-time error naming the
non-clonable type.

`Array<T>` and `map[K]V` cannot receive user-defined methods, since methods
may only be declared on types defined in the declaring package (§7.8). They
instead have a **built-in, compiler-provided element-wise/entry-wise clone**,
available under the same Copy-or-clonable eligibility rule applied to their
element type (`Array<T>`) or key and value types (`map[K]V`).

Unlike `drop` (§14.3), calling `value.clone()` directly via method-call
syntax is permitted — producing extra independent copies has no soundness
hazard, unlike calling a destructor more than once.

Ownership, error, and async implications: `clone` never transfers or
consumes ownership of its argument; the produced value is an independent
owner requiring its own eventual cleanup. This does not change Copy/Move
classification beyond what §8.3 and this section already state.

Compiler impact: for a type with a declared custom `clone`, validate and
select that method before considering structural cloning. Otherwise use the
built-in clone for `Array<T>`/`map[K]V`, or the eligible structural default for
a struct/fixed array. Diagnose an invalid custom declaration or a selected
method that fails ordinary type/borrow checking; never silently fall back to
structural cloning. Reject with a diagnostic naming the type when no valid
clone applies. This order implements the custom-precedence rule above.

```ore
type Point struct {
    X int
}

func (p Point) clone() Point {
    println("custom clone")
    return Point{X: p.X}
}

func demonstrate() {
    let original = Point{X: 7}
    let duplicate = clone(original)  // prints once; does not silently copy Point
    println(original.X)             // original remains available
    println(duplicate.X)
}
```

The selection rule does not change Copy/Move classification or borrow
provenance. A custom clone's ordinary side effects and panic behavior are
preserved, including required unwind cleanup; dispatch does not add implicit
error propagation or async suspension. Pending conformance cases:
`tests/conformance/destruction.md`.

---

# 11. Borrowing

## 11.1 Borrow by default — LOCKED

Passing a value to a normal parameter borrows it.

```ore
func printUser(user User) {
    println(user.Name)
}

let user = loadUser()
printUser(user)
println(user.Name)
```

The call to `printUser` does not consume `user`.

## 11.2 Mutable borrowing — LOCKED

Mutable access must be explicit in the callee contract:

```ore
func rename(user mut User, name string) {
    user.Name = name
}
```

The caller still writes:

```ore
rename(user, "Alice")
```

## 11.3 Exclusive mutable access — LOCKED

A mutable borrow requires exclusive access to the borrowed place for the duration of the borrow.

The compiler must reject conflicting aliases such as:

- shared borrow while an overlapping mutable borrow is active
- another mutable borrow while a mutable borrow is active
- moving a value while it is borrowed

## 11.4 Lifetime inference — LOCKED

Zore does not expose explicit lifetime parameters or lifetime syntax in ordinary source code.

The compiler infers lifetimes / regions.

Do not add Rust-style syntax such as:

```text
'a
&'a T
```

to the MVP.

## 11.5 Lifetime implementation — LOCKED SEMANTICS, FLEXIBLE IMPLEMENTATION

The implementation should prefer:

- local data-flow analysis
- control-flow-aware lifetime analysis
- inferred regions
- last-use information
- compact ownership contracts between functions

The language design intentionally avoids requiring global source-level lifetime annotations.

## 11.6 Mutable place requirements for callers — LOCKED

A place is **mutable** if and only if it is:

- a `var`-declared local binding (never `let` or `const`);
- a field or element projection reached entirely through mutable places (a
  field of a mutable place, or an element of a mutable fixed/dynamic array,
  including runtime indexing under §12.6); or
- reached through a `mut`-borrowed parameter, `mut` method receiver, or a
  `mut []T` slice element — such a parameter/receiver/element is itself a
  mutable place for the scope in which it is held.

An argument passed to a `mut Type` or `mut []T` parameter, and the receiver of
a `mut`-receiver method call, must be a mutable place by this definition.
A contextual mutable slice expression such as `edit(data[:])` is also allowed
for a `mut []T` parameter when its source supplies mutable access (§12.6); the
fresh descriptor carries that source's exclusive borrow for the call. This
does not permit arbitrary non-place expressions for other `mut` parameters.
Passing a `let` binding, or a place reached only through a shared borrow or a
shared `[]T` element, to a `mut` parameter, receiver, or slice position is a
compile-time error. This generalizes the writable-place requirement §5.6
already applies to assignment targets to argument-passing and method calls;
it does not change assignment's own rule.

```ore
let fixedUser = loadUser()
var mutableUser = loadUser()

rename(mutableUser, "Alice")   // valid: mutableUser is a var binding
rename(fixedUser, "Alice")     // invalid: fixedUser is a let binding
```

Ownership, error, and async implications: this rule only gates which places
may be passed as `mut`; it does not change borrow exclusivity (§11.3), Copy/Move
classification, or cleanup. No new runtime behavior is introduced.

Compiler impact: classify each argument/receiver expression's place as mutable
or not before matching it against a `mut` parameter, receiver, or slice
position, using the same place representation as assignment-target checking
(§5.6, §30.1). Reject with a diagnostic naming the non-mutable place and its
declaring binding. Pending conformance cases: `tests/conformance/ownership.md`.

## 11.7 Return-borrow contracts — LOCKED

Borrow provenance is separate from Copy/Move classification. A slice is a
borrowed view; copying its descriptor does not make its backing storage owned.
A value containing a borrowed view carries that view's provenance recursively,
including through struct fields, fixed-array elements, owned collections, and
closure captures. Moving a container transfers its storage and its existing
borrow obligations; it does not extend the backing storage's lifetime.

For every returned borrowed view, whether direct or nested in a result, the
compiler must infer which borrowed input storage it references. All possible
origins across reachable returns participate in the contract. At each call,
substitute the actual arguments' provenance into that contract. Preserve
field-level relationships where provable; otherwise conservatively retain all
possible origins. Recursive calls require mutually consistent contracts;
unresolved provenance is rejected, never treated as ownership.

A returned view must not refer to storage owned by a local, temporary, or
`own` parameter of the returning function. Returning that owner alongside a
view does not authorize a self-referential aggregate. A borrowed view already
contained in an input container may be forwarded only when its external
backing provenance is preserved; owning the container is not owning that
backing storage. The built-in empty zero slice has no backing-storage loan and
may be returned without a parameter origin (§41.4). This exception does not
apply to an arbitrary empty subslice, which retains its source provenance.

```ore
type View struct {
    Items []int
}

func wrap(items []int) View {
    return View{Items: items}  // valid: result retains items' backing provenance
}

func forward(view View) View {
    return view               // copying the struct preserves that provenance
}

func firstHalf(s mut []int) mut []int {
    return s                  // returns an exclusive reborrow, per §12.3
}
```

A `View` built from a local owned array cannot escape that array's lifetime,
including by return, assignment into longer-lived storage, or capture in an
escaping closure. Array/slice syntax is specified in §12.6 and map syntax in
§13.3. Closure and function types are specified in §16; these provenance rules
do not introduce further syntax.

The backing owner must remain valid through every use and any destructor that
can observe a contained view. Shared views prevent conflicting mutation; mutable
views require exclusive access. A shared borrow of a container does not grant
mutable access through a contained mutable view. Copies, explicit clones, and
parameter passing must obey the same rule (§12.3). No source lifetime syntax is
introduced. Borrow-containing values crossing task or channel boundaries must
also meet the independent-storage requirements in §18.4 and §19.4.

Ownership, error, and async implications: provenance survives both Copy and
Move operations and all nesting. Normal return, `?`, panic cleanup, and async
suspension must retain backing storage for as long as a live view needs it.
A zero slice returned by `?` has no loan; a successfully returned slice retains
its ordinary contract. No self-referential storage or lifetime extension is
implied by copying, moving, or placing a value in async state.

Compiler impact: track reachable borrowed views independently of type
classification, infer input-to-result provenance contracts, and propagate them
through aggregate construction, copies, moves, calls, and captures. Check escape
and destructor uses on every exit path. Reject when the required relationship
cannot be proven. Pending conformance cases: `tests/conformance/ownership.md`.

---

# 12. Slices

## 12.1 Slice meaning — LOCKED

A slice is a borrowed view into existing storage.

Syntax:

```ore
[]T
```

Example:

```ore
func sum(numbers []int) int
```

## 12.2 Mutable slices — LOCKED

A mutable borrowed slice uses:

```ore
mut []T
```

Example:

```ore
func sort(numbers mut []int)
```

## 12.3 Slice copying — LOCKED

Both shared and mutable slice descriptors remain Copy; neither owns or
implicitly duplicates the elements. Copy does not bypass borrow validation.

Copying a shared slice preserves its backing provenance. Multiple shared views
may coexist while the owner remains valid and no conflicting mutable access
occurs.

Copying a mutable slice creates an **exclusive reborrow**, not a second
independently usable mutable capability. While the derived view is live, access
to the overlapping storage through its source view is suspended. After the
reborrow's last use, access through the source may resume. The same restriction
applies recursively to copies of composites containing mutable views, argument
passing, and any clone that preserves a view. A shared borrow of a descriptor
or its container cannot be used to create a mutable reborrow. A custom clone
may instead create independent owned storage, subject to §11.7's return rules.

For example, inside `func edit(s mut []int)`, `var next = s` creates a reborrow.
Calling a mutating operation on `next` is allowed; accessing the same backing
storage through `s` before `next`'s final use is rejected. Once `next` is no
longer live, `s` may be used again. Binding mutability still follows §11.6.

Ownership, error, and async implications: derived views retain the original
loan and do not extend its lifetime; no cleanup of backing elements occurs when
a descriptor dies. Reborrow suspension applies across `await` and all control
flow. It cannot be bypassed by copying into a struct or closure.

Compiler impact: track parent/derived loan relationships, suspend overlapping
source access while a derived exclusive view is live, and preserve these
relationships through composite copies. Reject conflicts conservatively.
Pending conformance cases: `tests/conformance/ownership.md`.

## 12.4 Slice ownership — LOCKED

Slices do not own the backing allocation.

The primary owned dynamic collection is:

```ore
Array<T>
```

An `own []T` model is not part of the locked MVP design.

## 12.5 Slice aliasing — LOCKED

A slice borrow's exclusivity (§11.3) is checked against its **originating
place** — the whole `Array<T>`, `[T; N]`, or existing slice it was taken
from — not against its runtime index range. Two independently usable `mut []T`
values derived from
the same originating place conflict even when their ranges provably do not
overlap. A parent descriptor retained but suspended during an exclusive
reborrow (§12.3) is not independently usable. The compiler does not attempt
runtime-range-dependent aliasing proofs, consistent with the conservative, local data-flow model in §11.5.

This is a real MVP limitation, not an oversight: safe split-mutable-borrow
patterns (for example, obtaining two non-overlapping mutable sub-slices for
independent processing) are not expressible in the MVP language. There is no
`unsafe` escape hatch (§21.3) to hand-roll one; a dedicated future builtin
would be required to add this capability.

Ownership, error, and async implications: this rule is an application of the
existing exclusive mutable access model (§11.3), not a new ownership
category. It does not change Copy/Move classification or cleanup.

Compiler impact: track slice borrow conflicts at the granularity of the
originating place identified by the place abstraction (§30.1), not at the
granularity of an index expression's runtime value. Pending conformance
cases: `tests/conformance/ownership.md`.

---

## 12.6 Array literals, indexing, and slicing — LOCKED

### Array literals

```ore
var fixed = [int; 3]{10, 20, 30}
var dynamic = Array<int>{10, 20, 30}
let empty = Array<int>{}
let item = dynamic[1]
dynamic[1] = 25

let shared = dynamic[0:2]           // shared []int
var editable mut []int = fixed[:]  // exclusive mutable view
```

Array literals always state their type. Fixed-array literals must supply exactly
N elements; N follows the existing nonnegative constant-size rule. Dynamic-array
literals have as many elements as written, including zero. Reject omitted fixed
array elements, inferred element types, repeat/spread forms, and slice literals
in the MVP. Borrowed slices must view existing storage, except for zero views
already produced by built-in operations under §41.4.

Elements use comma separators, permit a trailing comma, and evaluate left to
right. Multiline literals require a trailing comma before a physical newline
following their final element, consistently with semicolon insertion. Every
element must produce one value compatible with the declared element type;
multiple-result calls cannot be spliced into an initializer. Copy elements are
copied and Move elements transferred under ordinary rules.

As with struct literals, parenthesize a collection literal used directly in an
if/for condition where its braces would otherwise conflict with the body.

### Indexing

`base[index]` is a postfix place expression for fixed arrays, dynamic arrays,
and shared/mutable slices. Evaluate base, then index, once each. Accept integer
indices, including contextually representable untyped integer constants; reject
float, rune, and bool indices. Check the mathematical integer value before any
machine-index narrowing: negative or index >= length is invalid.

Reject invalid bounds when both index and length are statically known;
otherwise panic on an invalid runtime bound, in every build mode. Large unsigned
indices must fail the bounds check rather than wrap into valid indices.

Copy element reads produce copied values. Move elements may be borrowed by an
ordinary parameter without being extracted. Moving out is permitted only for
constant-indexed fixed-array places allowed by §31.2; moving out of a dynamic
array, slice, or runtime-indexed fixed array remains rejected. Extraction APIs
are a separate Q05 decision.

Runtime indexing through mutable arrays and mutable slices produces a writable
place. Mutability of runtime-indexed places (§11.6) does not extend partial-move
tracking (§31.2). Runtime-index aliasing
remains conservative: two differently written indices are not proof that two
mutable borrows are disjoint. Shared slices never grant element mutation.

### Slicing

`base[low:high]` denotes a half-open range, excluding high. Permit omitted low
(default zero), omitted high (default current length), and `base[:]`. Bounds
use the same integer rules as indexing. Require 0 <= low <= high <= length;
equal bounds, including length:length, are valid. Reject statically known
invalid ranges; otherwise panic at runtime. Do not add step/stride or a third
capacity bound.

Evaluate base, low if present, then high if present, once each. Slicing borrows
existing backing storage; it neither clones nor transfers the elements.
Owned-array bases must be places whose lifetime covers the resulting borrow;
a view cannot escape a temporary owner. Slicing an existing view preserves its
original provenance. The whole originating place remains the alias-checking
unit (§12.5), even for disjoint runtime ranges or zero-length subslices.

Without a mutable-slice expected type, slicing yields shared `[]T`, even from a
var binding. An explicit `mut []T` binding/result annotation or a parameter of
that type requests an exclusive view. The source must supply mutable access
under §11.6; a let-owned array or shared slice cannot do so. A mutable view's
subslice is an exclusive reborrow when mutable access is requested; overlapping
access through the source remains suspended until that reborrow ends.

```ore
func inspect(items []int) { /* reads only */ }
func edit(items mut []int) { /* may mutate */ }

func demo() {
    var data = Array<int>{1, 2, 3}
    inspect(data[:])               // shared borrow during call
    edit(data[:])                  // contextual exclusive borrow during call
    var part mut []int = data[1:]  // exclusive view, named mutable binding
    edit(part)
    // data can be used again after part's final use
}
```

There is no implicit whole-array-to-slice conversion: use `data[:]`. An already
bound shared slice is not upgraded by a later mutable use. A let-bound mutable
slice descriptor still obeys existing §11.6 caller-place requirements; use var
for a named view that will be passed onward as mut.

### Ownership, errors, and async

Literal construction retains exactly one owner per Move element. On `?` or
panic during a later initializer, clean up already initialized elements and
owned temporaries exactly once; do not clean up nonexistent elements. No
source value is restored after ownership was already transferred.

Index assignment uses §5.6's target/RHS/store ordering and cleanup rules. Bounds
failures panic rather than return error values. Access through a view retains
its backing loan, including through composites and across suspension; §11.7,
§12.3, §17.6, and task/channel escape restrictions continue to apply. Resizing,
replacing, moving, or dropping a backing owner cannot invalidate a live view.

### Compiler impact

Add distinct AST forms for fixed/dynamic array literals, indexing, and slicing;
keep source spans for the base, bounds, and elements. Resolve collection types,
contextual slice mutability, element count/type checks, and place mutability
before ownership checking. Lower bounds checks without narrowing first; retain
partial-initialization state for cleanup. Do not treat runtime-index writable
places as permission for partial moves or proven disjointness.

Pending conformance cases: `tests/conformance/arrays-slices.md`. Map operations
are specified separately in §13.3. Length, growth, and removal of the last
element are specified in §12.7 and iteration in §5.10. String
indexing/slicing/length, capacity APIs, and full `go` grammar remain separate
Q02/Q05 decisions. Array/slice rules do not supply semantics for those forms.

---

## 12.7 Collection length, `push`, and `pop` — LOCKED

```ore
var names = Array<string>{"Ada"}
names.push("Lin")
println(names.len())          // 2
let found, last = names.pop() // true, "Lin"
let empty = Array<string>{}
let none, zero = empty.pop()  // rejected: `empty` is a `let` binding
```

`c.len()` returns the number of elements of a fixed array, slice (shared or
`mut`), or `Array<T>`, the number of entries of a map, or the number of bytes
of a `string` (§6.8), as an `int`. It takes
no arguments and shared-borrows its receiver only while it runs. A fixed
array's length is its declared size; the receiver is still evaluated once.

`a.push(value)` appends one element to an `Array<T>`. `a.pop()` removes the
last element and returns `(bool, T)`: presence first, then the removed value,
like map removal (§13.3). An empty array yields false and T's zero value
(§41.4). Both require `a` to be a mutable place (a `var` binding, a `mut`
parameter, or a field or element of one), as for assignment. Neither has a
method form on fixed arrays, slices, maps, or strings.

Evaluation order: the array place first, then the pushed value, then the
change. The pushed value is copied if Copy and transferred if Move; `pop`
transfers the removed element to the caller. A Move result of `pop` follows
the ordinary rules for multiple results (§7.2), including use of an error
result. Growth may move the elements to new storage, so `push` and `pop`
mutably borrow the whole array: no view of its elements may be live across the
call (§12.5). Growth reserves extra room so that repeated pushes take amortized
constant time; capacity is not observable. Allocation failure aborts the
process (Q17g).

Ownership, error, and async implications: no new ownership category. `push`
is a mutable use of the array and a use (copy or move) of the value; `pop` is a
mutable use of the array that produces an owned value. Views held by the
pushed value join the array's provenance, as for element assignment; views
held by the popped value keep the array's provenance. Neither suspends.

Compiler impact: resolve the three names as compiler-provided methods on
collection types only (user code cannot declare methods on them); check
mutable receivers; lower `push`/`pop` as calls that exclusively borrow the
array, and `len` as a read of the descriptor or the map's entry count. Pending
conformance cases: `tests/conformance/arrays-slices.md` and
`tests/conformance/maps.md`.

---

# 13. Maps

## 13.1 Map syntax — LOCKED

```ore
map[string]User
```

Examples:

```ore
func lookup(users map[string]User)
func addUser(users mut map[string]User)
func consume(users own map[string]User)
```

## 13.2 Map ownership — LOCKED

Maps are Move/resource-owning values.

Passing a map:

- normally borrows it
- with `mut` mutably borrows it
- with `own` transfers ownership

---

## 13.3 Map construction, lookup, assignment, and removal — LOCKED

### Construction and result shape

```ore
var scores = map[string]int{"Ada": 10, "Lin": 20}
let found, score = scores["Ada"]
scores["Ada"] = 30
let removed, oldScore = scores.remove("Ada")
let empty = map[string]int{}
```

Both lookup and removal always produce two results, `(bool, V)`: presence first,
value second. A missing key produces false and V's zero value. A present key
produces true even when its stored value equals zero. Explicit `_` discards are
allowed. There is no context-dependent one-result lookup form.

Presence first is deliberate: methods returning an error-typed V still obey
§7.2's trailing-error rule. For `map[string]error`, removal returns `(bool,
error)` and follows ordinary error-use/propagation rules. Subscript lookup is
not a call, so §15.2 does not permit appending `?` to it; bind/use/discard its
error result explicitly. This adds no exception to result forwarding
or error-result ordering.

### Key and value types

MVP keys are bool, integer types (including aliases), rune, and string. Key
matching uses their existing equality semantics; string keys compare content,
not buffer identity. Equal keys must have equal hashes. Hash algorithm, storage
layout, and collision strategy are implementation details.

Reject float keys for the MVP to avoid NaN/equality corner cases. Also reject
error, Task, channel, array, slice, map, struct, and closure keys in the MVP;
being Copy or supporting equality alone does not imply map-key eligibility.
General user-defined hashing/equality remains excluded. Key arguments must match
K under ordinary contextual-literal and exact typed-value compatibility rules.

Values may be Copy or Move, subject to existing recursive provenance and type
validity rules. Maps remain Move; storing borrowed views does not extend their
backing lifetime. No map equality, ordering, or hashing is added.

### Literals and evaluation

`map[K]V{key: value, ...}` explicitly states both types. Empty maps and trailing
commas are supported. Follow existing semicolon insertion: multiline final
entries need a comma before the newline. Parenthesize literals where a condition's
body brace would otherwise be ambiguous, as for struct/array construction.

Evaluate each key, then its value, and complete that entry before the next
entry, left to right. Each expression yields one value of its declared type;
multiple-result splicing, spreads, and type-elided literals are rejected.
Keys are copied; Move values are transferred, with no implicit clone.

Duplicate literal keys are rejected when equal constant keys can be established
statically; otherwise a duplicate encountered at runtime panics. For runtime
entries, evaluate the key and value before testing/inserting the complete entry.
On duplicate failure, destroy the uninserted owned value and previously
constructed entries exactly once. Do not silently overwrite an earlier literal
entry. This differs deliberately from assignment to an existing map.

### Lookup and borrowed access

Evaluate `m[key]` as map expression then key, once each. Lookup shared-borrows
the map, does not remove its entry, and returns `(found, copiedValue)` only when
V can be copied through shared access. If V is Move, reject lookup: no implicit
clone, ownership transfer, or reference-shaped result is invented.

A Copy type containing mutable views is also rejected when copying would grant
an exclusive capability through this shared access (§11.7, §12.3). Copy shared
views retain their external provenance; their backing must remain live after
lookup. Ordinary independent Copy values such as numbers and strings are valid.

Map subscripts are not general addressable element places. Reject borrowing an
entry by passing `m[key]` to a parameter expecting V, field updates such as
`m[key].Field = value`, and chained access treating the result pair as V. Bind
the two results first for Copy values; remove Move values to obtain ownership.

Borrowed in-place access for Move values is a follow-up Q05 API decision,
coordinated with closure/callback rules in Q02. No raw references or source
lifetimes are added here. This is a stated limitation; it does not imply that
all map operations needed by the MVP are resolved.

### Assignment and replacement

`m[key] = value` inserts or replaces one entry. It is a special map-assignment
target, not a borrowable pointer to an entry. The map must be a mutable place;
let-owned maps and maps reached through shared borrows cannot be modified.

Follow §5.6: evaluate the map/key target, then RHS, once each, retaining the
key and map identity rather than a pointer that rehashing could invalidate.
Validate ordinary aliasing/exclusivity constraints across these phases. Only
then modify the map. Copy values are copied; Move values transfer to the map.

For replacement, destroy the previous owned value before storing the new one.
Remove the old entry from the map's initialized-entry set before its destruction
begins; if that destruction panics, unwind without dropping it again. Clean up
the new RHS temporary and the remaining map entries under ordinary rules. A
containing custom destructor still sees a valid map whose replaced key is absent,
never uninitialized map storage. The key/value result is not rolled back.

Compound map assignment such as `m[key] += value` is rejected
because map lookup is a two-result expression and missing-entry behavior would
need another rule. Multiple assignment follows §5.6's non-overlap requirement;
do not assume two key expressions denote distinct or independently mutable slots
in the same map. Use separate assignments when independence cannot be proven.

### Ownership-transferring removal

The compiler-provided `m.remove(key)` uses a mut receiver, borrows the key for
lookup, and returns `(bool, V)`. It is available for both Copy and Move V.
On success, detach the entry and return its value without destroying that value;
the caller now owns it. On absence, return false and V's harmless zero state.
No source-level generic method declaration syntax is introduced.

```ore
// Resource is a Move type with harmless zero-state cleanup (§41.4).
func take(resources mut map[string]Resource) {
    let found, resource = resources.remove("primary")
    if found {
        use(resource)  // ordinary shared borrow; resource remains locally owned
    }
    // cleanup: acquired resource once, or harmless empty-state cleanup
}
```

The map stays valid and contains no moved-out hole. Removal is not the forbidden
partial move from a computed index (§31.2); it is an explicit builtin operation
that updates the collection's initialization state. A removed value retains any
external borrow provenance. No view into map-owned storage may survive mutation.

### Ownership, errors, async, and cleanup

Missing lookup/removal is represented by false, not a panic or an ordinary
error. Literal duplicate failure panics with ordinary cleanup. No operation
implicitly awaits or spawns work. Explicit await or `?` in key/value expressions
follows left-to-right evaluation and keeps retained values/borrows valid through
suspension and early exit. Future iteration and callback APIs must respect the
same map mutation exclusion.

Destroying a map destroys every remaining initialized entry exactly once;
ordering between distinct entries is unspecified. Resource zeros from a miss
honor §41.4; initialization/destruction is not skipped because presence is false.
Partially constructed literals and failed replacements need per-entry cleanup
state. Completed moves are never undone on error or panic.

### Compiler impact

Add map-literal AST entries, type-restricted two-result map lookup, a distinct
map-assignment target, and compiler-provided remove resolution. Preserve spans
for map/key/value expressions. Reject invalid key types and illicit shared
copies, track external view provenance, and lower mutable operations without
retaining raw bucket addresses across arbitrary RHS evaluation. Maintain entry
ownership states for duplicate failure, replacement panic, removal, and drop.

Pending conformance cases: `tests/conformance/maps.md`. Map length is
specified in §12.7 and iteration in §5.10; capacity APIs and borrowed in-place
entry access remain Q02/Q05 work.

---

# 14. Deterministic Destruction and Drop

## 14.1 Automatic destruction — LOCKED

Owned resources are destroyed automatically when their lifetime ends.

Zore does not depend on garbage collection for deterministic resource cleanup.

## 14.2 Last-use cleanup — LOCKED DIRECTION

Where safe, the compiler should be able to release an owned value after its last safe use instead of retaining it unnecessarily until lexical function exit.

The observable guarantee is correct deterministic destruction.

The exact optimization strategy may evolve.

## 14.3 User-defined `drop` method — LOCKED

A resource-owning type may define cleanup behavior:

```ore
type Connection struct {
    socket Socket
}

func (c mut Connection) drop() {
    c.socket.close()
}
```

The receiver must be `mut` — never a plain shared receiver or `own`. `mut`
grants mutable access for cleanup (setting a field, calling a mutating
cleanup method on a field) without ownership, so a `drop` method can never
move a field out of the value it is destroying; a method called on a field
from within `drop()` must itself take a shared or `mut` receiver, not `own`.
`drop` takes no parameters and returns no result. Defining a custom `drop`
forces the type to be Move (§8.3). Moving out of any part of such a value is
forbidden by §31.2, including from outside the destructor; whole-value moves
remain allowed. A destructor must also honor the harmless resource zero-state
contract in §41.4. In the illustrative `Connection` above, `Socket.close()` must
be harmless on an empty Socket and compatible with its later automatic field
cleanup; no such library signature is introduced by the example.

The compiler invokes destruction automatically when ownership ends. After a
custom `drop` body completes, the compiler still automatically recursively
drops every field (§14.5), exactly as it would for a type with no custom
`drop` — a custom `drop` supplements automatic field cleanup, it never
replaces it.

Calling the user-defined method directly via method-call syntax
(`connection.drop()`) is a compile-time error. The only ways to trigger
destruction are automatic cleanup at end of lifetime and the explicit
`drop(value)` builtin (§14.4); allowing direct method calls would let a
program invoke cleanup twice as ordinary calls, which is exactly the
double-drop hazard §14.4 exists to prevent.

## 14.4 Explicit `drop(value)` — LOCKED

Explicit destruction is allowed:

```ore
drop(connection)
```

After explicit drop, the value is consumed and may not be used again.

The compiler must prevent double-drop.

This builtin, and automatic cleanup at end of lifetime, are the only two ways
a value's destruction is triggered (§14.3); a value's own `drop` method may
never be called directly.

## 14.5 Struct cleanup order — LOCKED

Owned fields are cleaned up automatically.

Cleanup should occur in reverse acquisition/declaration order where applicable.

## 14.6 Ownership transfer and destruction — LOCKED

After ownership transfers, the previous owner must not destroy the value.

Only the current owner is responsible for eventual cleanup.

## 14.7 Resource resurrection — LOCKED

A value being destroyed must not be resurrected through `drop`.

## 14.8 `defer` — TBD / NOT REQUIRED FOR RESOURCE SAFETY

A general-purpose `defer` mechanism may exist later.

It is not the primary ownership or resource-management mechanism.

The MVP must not depend on `defer` for deterministic cleanup.

---

# 15. Error Handling

## 15.1 Explicit error values — LOCKED

Zore uses explicit error values.

Example:

```ore
func readFile(path string) (string, error)
```

`error` is a normal built-in value type, not a hidden exception mechanism, and
not an interface type (§22.2): `error` is exactly **one concrete predeclared
type**, not an extensible contract, and it stays Copy. There is no user-defined custom error type, no
downcasting, and no type-assertion mechanism in the MVP.

`error` is Copy (§10.2), consistent with `string`. It holds an immutable
message. The only way to produce a non-nil `error` value from source is the
predeclared constructor:

```ore
error(message string) error
```

reusing the type-name-as-callable-form convention already locked for numeric
conversions (§6.6), rather than introducing new construction syntax. The zero
value of `error` is `nil` (§41.4), meaning "no error." Wrapping/cause chains,
sentinel error declarations, and structured error payloads are not decided by
this section and remain open for a future standard-library decision (Q05);
nothing here should be read as authorizing them yet.

`==` and `!=` are defined between two `error` values (§6.6): both `nil` are
equal; one `nil` and one non-nil are unequal; two non-nil values are equal iff
their messages are equal (content equality, not identity — consistent with
`error` being Copy). This is a wider comparability than `Task<...>`, which is
comparable only against `nil` (§41.4), and `channel<T>`, which is not
comparable at all (§6.6); those are runtime handles without meaningful content
equality.

```ore
let notFound = error("not found")
let e = mayFail()
if e == notFound {
    // content-equal error value, not identity
}
```

## 15.2 Error propagation operator — LOCKED

`?` propagates an error to the caller.

Example:

```ore
let file = open(path)?
let content = read(file)?
return parse(content)?
```

A function, method, or closure's result list may contain at most one
`error`-typed result, which must be the last result if present (§7.2). `?` may
follow a call expression, an `await call()` expression, or an `await task`
expression (§18.9) under the grouping rule in §7.6, whose resolved last result
type is `error`. Using `?` is a compile-time error unless the enclosing
function/closure itself declares a trailing `error` result to propagate into —
there must be somewhere for the error to go.

Evaluate the call or awaited task (and its `await`, if present) exactly once. If the resulting
error is non-nil, immediately return from the enclosing function: every
non-error result position of the enclosing function's return takes that
type's zero value (§41.4), the error position carries the propagated error,
and required cleanup runs first (§15.3). If the resulting error is `nil`, the
`?` expression's value is the call's remaining non-error result(s) — a single
value, or a multiple-result expression usable anywhere one is already
permitted (matching bindings, whole-result forwarding per §7.8), never spliced
into an argument list. The error component is consumed by `?` and is not
separately observable from that evaluation. Forwarding an error result this
way is an explicit use, not a silently ignored one (§15.6).

## 15.3 Cleanup during error propagation — LOCKED

When `?` causes an early return, all still-owned resources whose lifetime ends on that path must be cleaned up before control leaves the function.

Conceptually:

```text
owned values
    ↓
operation returns error
    ↓
required Drop operations
    ↓
return error
```

## 15.4 Panic — LOCKED

`panic()` may be used for:

- invariant violations
- programmer errors
- unrecoverable runtime/system failures

Panic is not a replacement for ordinary error handling.

### The `panic` call

`panic` is a predeclared name (§3.17). A program raises a panic with

```ore
panic(message string)
```

It takes exactly one `string` argument; an untyped string constant takes the
type `string`. It has no result and never completes: the code after it on the
same path does not run, and a call statement of `panic` ends a path for the
completion rule of §7.7. Like `println` (§37.1), it can be used only as the
callee of a direct call; it cannot be bound, passed, returned, or stored. The
reported message is the argument followed by ` at ` and the call's
`file:line:column`, the same form as the runtime's own panics.

```ore
func pick(n int) int {
    if n > 0 {
        return n
    }
    panic("n must be positive")   // no return needed after it
}

panic(1)               // invalid: the message is a string
let p = panic          // invalid: panic can only be called
let x = panic("x")     // invalid: panic has no result
```

### Unwinding and cleanup

On `panic()`, the runtime unwinds the current task's call stack, running
`drop` (§14.1–14.3) for every still-owned value it passes on the way out.
Deterministic resource cleanup (§1.1) is a core goal that holds on the panic
path, not only on the success path.

Panic is not catchable or recoverable in the MVP: there is no `try`/`catch`/
`recover` builtin, and no operation converts a panic into a value the program
can inspect. This keeps panic a distinct, simple "unwind, clean up, then end"
mechanism rather than a second exception system layered on top of `error`
(§15.5). What ends is defined in §18.10: a panic in the program's initial
task terminates the process; a panic in a spawned task ends that task and is
raised again in any task that waits on it.

If a `drop` invoked while already unwinding from a panic itself panics, the
runtime aborts the whole process immediately rather than attempting a nested
unwind — running two unwinds at once has no well-defined semantics.
The runtime also aborts if destruction of an old field during replacement
panics after invalidating a field needed by a containing custom destructor
(§31.2). It must not invoke that destructor on an incomplete receiver.

Ownership, error, and async implications: unwind-driven cleanup uses the same
Copy/Move and drop rules as ordinary scope exit; it introduces no new resource
model. It does not change `error`/`?` propagation (§15.1–15.2), which remains
the ordinary, catchable way to signal expected failure.

Compiler impact: lower panic to a stack unwind of the current task that runs
pending drops in reverse acquisition order per frame (§14.5), then hands the
ended task to the runtime (§18.10); treat a panic raised by a `drop` running
during unwinding as a process abort. Pending conformance cases:
`tests/conformance/destruction.md` and `tests/conformance/concurrency.md`.

## 15.5 Hidden exceptions — LOCKED OUT

Zore does not use hidden exception control flow as the normal error model.

The compiler must not silently turn ordinary `error` returns into exception semantics.

---

## 15.6 Error-result use and explicit discard — LOCKED

Error results must not be silently ignored. An expression statement that leaves
a result of type `error` unused is a compile-time error, including a call returning
multiple values with an error result. Explicitly discarding that result with `_`
is allowed. The rule depends on the resolved result type, not the function name
or whether a particular call happens to succeed at runtime.

For these examples, assume `save()` returns `error` and `load()` returns
`(Value, error)`:

```ore
save()                 // invalid: silently ignored error result
_ = save()             // valid: explicitly discarded error
let value, _ = load()   // valid: explicitly discard the error result
let value = load()?     // propagate error, subject to the caller's return contract
```

`let _ = save()` and `var _ = save()` are also explicit discards under §5.5.
The expression still executes exactly once. Discarding its error does not undo
side effects, retry the operation, panic, or implicitly propagate the error.

A named local binding of type `error` that is never used is a compile-time
error, even if its name begins with an underscore. For example, binding
`let value, err = load()` and never using `err` does not acknowledge the error.
The programmer may inspect or pass the error, return/propagate it where permitted,
or explicitly discard it with `_ = err`. Merely creating a named binding or
running its automatic cleanup is not a use for this rule.

This is an explicit-use requirement, not proof that a program recovers correctly
from every failure.

### Flow-sensitive extensions

The never-used check is path-sensitive: the compiler must be able to prove a
use (read, comparison, `?` propagation, or explicit `_` discard) on **every**
reachable path before a named `error` binding's scope ends or it is
reassigned. If the compiler cannot prove this, the program is rejected. This
reuses the same class of flow analysis already required for ownership checking
(§23.2); it is not a new category of analysis.

Reassigning a `var` binding of type `error` is a compile-time error unless its
current value was used, by the same definition, on every path reaching that
reassignment:

```ore
var err = attempt1()
err = attempt2()   // invalid: attempt1()'s error was never used before being overwritten
```

```ore
var err = attempt1()
if err != nil {
    return err
}
err = attempt2()   // valid: the prior value was read and handled on every path first
return err
```

Once an `error` value is stored into a struct field, array/slice element, or
map value, this tracking stops: only directly named local bindings and
parameters of static type `error` are checked, never storage reached through a
composite. This keeps the analysis bounded to simple places, matching how it
is already scoped to bindings rather than arbitrary storage.

Ownership, error, and async implications: explicit discards preserve the ordinary
ownership and deterministic cleanup requirements of §5.5; `error`'s
representation and Copy classification are locked in §15.1. `?` continues to
propagate with required cleanup (§15.3). The same error-result rule applies to completed
`await` expressions and results retrieved through `task.wait()`. It does not
implicitly await a task or cancel detached work; task-handle detachment rules
remain unchanged. No hidden exception mechanism is introduced.

Compiler impact: use resolved result types to diagnose ignored error results,
distinguish explicit discard targets from absent result handling, and track
path-sensitive uses of error-typed local bindings across reads, comparisons,
`?`, reassignment, and scope exit. Point diagnostics to the ignored expression
or unused binding and suggest handling, propagation where valid, or explicit
`_`. Pending conformance cases are in `tests/conformance/errors.md`.

---

# 16. Closures

## 16.1 Closure syntax — LOCKED

A closure literal is an expression written like a function declaration without
a name. Parameters (with the `mut`/`own` modes of §7.3), the optional result
list (§7.2), and the block body follow the function-declaration rules:

```ore
let name = "John"

let greet = func() {
    println(name)
}

greet()

let add = func(a int, b int) int {
    return a + b
}
```

Parameter names, duplicate checks, shadowing, and the outermost-body scope
follow §5.7–5.8 and §7.8. `return` exits the closure, not an enclosing function
(§7.7), and `?` needs a trailing `error` result declared by the closure itself
(§15.3). `break` and `continue` cannot reach a loop outside the closure.
`await` is invalid in a closure body (§17.2); `async` closures are not part of
this decision. A closure literal appears only inside a function body, never in
a constant or a type.

A literal may be called where it is written, as in `func(x int) int { return x }(2)`.

## 16.2 Function types — LOCKED

A function type is written like a signature with no names. Each parameter is a
type with an optional leading `mut` or `own`, and an optional result list
follows:

```ore
func(int, int) int
func(mut []int)
func(string) (int, error)
func()
```

Two function types are identical when their parameter types, modes, and result
types are identical, in order. Function types are not comparable, have no zero
value (a binding must be initialized, §5.4), cannot be printed, and cannot be
map keys (§13.3).

**Declared functions as values.** The name of a declared synchronous function,
written where a value is expected, is a function value whose type is the
declaration's signature with the names removed. A package-qualified name
(`pkg.F`) converts the same way for every function the package exports, whether
it is written in Zore or implemented by the compiler. The value is a closure
with no captures: evaluating the name has no effect, borrows nothing, and moves
nothing. Like every function-typed value it is Move. A built-in operation
(`println`, `len`, `push`, `clone`, channel and mutex operations, error
construction) is not a function value and is rejected. The name of an `async func`
is a value of an async function type, described below. A method used as a value
is described below.

```ore
func add(a int, b int) int { return a + b }

let f = add                        // func(int, int) int
let apply = func(op func(int, int) int, x int, y int) int { return op(x, y) }
println(apply(add, 2, 3))          // 5
```

**Async function types and values.** A function type may begin with `async`:
`async func(int) (string, error)`. The `async` property is part of the type, so
two function types are identical only when they agree on it as well as on the
parameter types, modes, and results. There is no conversion between `func(T) R`
and `async func(T) R` in either direction. The name of a declared `async func`,
including a package-qualified one, is a capture-free Move value of the async
function type with its signature; evaluating the name has no effect. An `async`
method, a built-in operation, and a closure literal are not async function
values (a closure body is never async, §16.1).

A call through a value of async function type is an async call (§17.8): it must
be the operand of `await` or of `go`. The callee is evaluated first and the
arguments left to right, each once. An awaited call uses the callee exclusively
for the whole call, including while the task is suspended. Async function types
may appear wherever other function types may (§16.4), and never as a slice
element or a map key.

```ore
async func fetchUser(id int) (User, error) { /* ... */ }

type Route struct {
    Path    string
    handler async func(int) (User, error)
}

let h = fetchUser                // async func(int) (User, error)
let user, err = await h(7)       // inside an async func
let task = go h(7)               // anywhere; Task<User, error>
```

**Method values.** `value.Method`, written where a value is expected and not
called, is a function value and means exactly the closure literal that calls
the method:

```ore
func(params) results { return value.Method(params) }
```

with the method's parameters and results (the receiver is not a parameter). No
rule is added: the receiver is captured by the ordinary capture rules (§16.3,
§16.4), and the receiver's mode decides the capture. A shared receiver is
captured by a shared borrow. A `mut` receiver is captured by an exclusive borrow
and must be a mutable place (§11.6). An `own` receiver is moved into the
closure, which is call-once (§16.6). When the value escapes, including being
spawned by `go`, it is owning (§16.4): a Copy receiver is copied and a Move
receiver is moved when the value is created, and a spawned method value follows
§18.3 and §18.4.

```ore
type Counter struct { N int }

func (c Counter) read() int { return c.N }
func (c mut Counter) bump() { c.N += 1 }
func (c own Counter) finish() int { return c.N }

var counter = Counter{N: 1}
let read = counter.read           // func() int; shared capture of `counter`
println(read())                   // 1
let bump = counter.bump           // func(); exclusive capture of `counter`
bump()
let done = counter.finish         // call-once; consumes `counter`
println(done())
```

The receiver must be a local or a field path rooted at a local, because capture
is per whole local (§16.3). A call result, an index expression, or a map lookup
is rejected as the receiver of a method value; bind it to a local first.
Creating a method value evaluates nothing but the capture. A method declared
`async` is rejected as a value, for the reason an `async func` name is. A method
of another package is usable as a value exactly where a call is allowed
(§3.20). The `drop` method cannot be used as a value. Method expressions written
on the type (`Counter.read`) are not part of this decision.

Calling a value of function type uses ordinary call syntax and ordinary
parameter rules (§7.3–7.4): arguments are borrowed unless the parameter is
`mut` or `own`, with no call-site markers. The callee is evaluated before the
arguments (§7.5). The call yields the closure's results, which may be
forwarded, discarded, or propagated with `?` as for any call.

Calling a closure uses it **exclusively** for the duration of the call, because
the call may write through its captures. A closure that is captured by
another live closure therefore cannot be called directly (§16.3).

## 16.3 Capture rules — LOCKED

Closure captures use the same ownership model as normal code. The compiler
infers, for every outer local the body mentions, the weakest capture that
satisfies its uses; the programmer writes no capture list.

| Use inside the body | Capture |
| --- | --- |
| Only read (including copying a Copy value out, or passing it to a borrowed parameter) | Shared borrow of the outer local |
| Assigned, compound-updated, or passed to a `mut` parameter or receiver | Exclusive borrow; the outer binding must be a mutable place (§11.6) |
| A captured closure or `mut []T` value, whatever the use | Exclusive borrow; no `var` is required, as for copying a mutable view (§12.3) |
| A Move value consumed (passed to `own`, returned, `drop`ped, or moved into a binding or value) | The closure owns the value and is call-once (§16.6) |

A local used from a closure nested inside another closure is captured by each
enclosing closure in turn, and an exclusive use anywhere makes every capture in
the chain exclusive. Capture is per whole local; field-level captures are not
part of this decision.

A capture is a loan that begins when the closure literal is evaluated and ends
at the last use of the closure value (§12, region analysis). While it lasts, the
outer local obeys the normal borrow rules: a shared capture forbids writing or
moving the local, and an exclusive capture forbids any other use. A capture of
a value that holds views also keeps those views' backing borrowed. Because
writes are rejected while a closure is live, a closure never observes a stale
copy.

```ore
var count = 0
let bump = func() { count += 1 }
bump()
println(count)   // accepted: `bump` is not used again

let late = func() { count += 1 }
println(count)   // rejected: `late` holds `count` exclusively and is used below
late()
```

A closure value is a Move value. Binding it to another name moves it. Passing it
to a parameter of function type borrows it **exclusively** for the call, so the
same closure cannot be passed twice to one call; the callee may call it any
number of times. A closure that mutates captured state needs no `var` binding:
its exclusive loan is held by the closure itself, and only one live name for it
exists.

The rules above describe a **borrowing** closure. An owning closure (§16.4)
captures values instead of borrowing places.

Inside a borrowing closure's body, a capture refers to the outer local in place. A value
that holds a borrow may be stored into a captured local only when it borrows
from other captured locals; the outer local is then treated as borrowing them
from the point the closure is created. Storing a view of the closure's own
parameters or locals into a capture is rejected (the first as unsupported).
Storing other values is allowed through an exclusive capture.

Consuming a captured Move value inside the body makes the closure call-once
(§16.6).

## 16.4 Borrowing and owning closures — LOCKED

A closure is **borrowing** unless it escapes; then it is **owning**. The
compiler infers which; no syntax marks it. A closure literal is owning when the
literal itself, or a local it initializes (directly or through `let g = f`
rebinding), is:

- returned from a function or closure;
- stored in a struct field, fixed-array or `Array<T>` element, or map value,
  whether by a literal, an assignment to a field or element, a map assignment,
  or `push`;
- passed to an `own` parameter;
- the callee of `go`, or a `go` argument passed to an `own` parameter of
  function type (§18.3).

A call-once closure (§16.6) is also owning.

```ore
func counter() func() int {
    var count = 0
    return func() int {     // owning: the literal is returned
        count += 1
        return count
    }
}
```

An owning closure captures **values** when it is created, not places:

- a captured Copy local is copied; later changes inside and outside the closure
  are independent;
- a captured Move local is moved into the closure, so the outer local cannot be
  used afterwards;
- a captured Copy local that the closure assigns is also treated as moved, so
  the outer local cannot be read afterwards and nobody mistakes the closure's
  copy for the original.

The closure value owns those captured values. They are destroyed, in reverse
capture order, when the closure value is destroyed: at the end of its owner's
scope, on replacement, through `drop`, or when its owner is destroyed. A captured
value that holds a view keeps that view's backing borrowed for as long as the
closure lives (§11.7), so an owning closure still cannot outlive what its
captured views borrow.

Function types may therefore be results, struct field types, and fixed-array,
`Array<T>`, and map value types. They are never slice element types or map
keys. A parameter of function type may be `own`, which lets the callee store or
return it; it cannot be `mut`. As for mutable views (Q20), a shared parameter
whose type holds a function value inside a struct, array, or collection is
rejected: calling a closure uses it exclusively, which a shared borrow cannot
grant. For the same reason a collection loop (§5.10) cannot visit function
values. Calling a closure stored in a field or element requires that place to
be usable exclusively, as for any call (§16.2).

Every closure value, borrowing or owning, is checked by the same region
analysis: a borrowing closure that would outlive a local it captures (by being
returned, stored, or assigned to an outer place) is rejected, and the
diagnostic names the captured local. Liveness across `await` remains
unsupported (Q02g). Use with `go` is defined in §18.3 and §18.4.

A panic or `?` inside a closure runs the closure's own cleanup and then
continues in its caller as for any call.

## 16.5 Compiler impact (informative)

Resolution gives each closure literal its own body and records its captures;
typing adds interned function types and infers whether each literal is
borrowing, owning, or call-once; MIR lowers a closure to a separate body whose
capture locals refer to their referents by reference, plus a closure-creation
value; ownership reuses the existing loan and region machinery with the closure
value as the loan holder. Code generation represents a closure as a code
pointer, an environment pointer, and a destructor pointer. A borrowing
closure's environment lives in the creator's frame and holds the captured
places' addresses; an owning closure's environment is heap storage holding the
captured values, which its destructor destroys and frees. None of this
representation is a source-language contract.

Pending conformance cases: `tests/conformance/closures.md`.

## 16.6 Call-once closures — LOCKED

A closure whose body consumes a captured Move value is **call-once**:

```ore
let job = Job{Id: 7}
let finish = func() { consume(job) }   // owns `job`
finish()                                // consumes `finish`
finish()                                // rejected: `finish` was used
```

A call-once closure must be the initializer of a single-name `let` binding.
That binding may only be called directly, as in `finish()`; it cannot be passed
as an argument, returned, stored, rebound, captured by another closure, or
moved. Calling it consumes it, so a second call, or any use after the call, is
rejected as a use of a moved value. Inside the body each captured value is
consumed at most once, by the ordinary move rules (§31). Captured values the
call does not consume are destroyed when the call returns. A call-once closure
that is never called is destroyed at the end of its scope with every captured
value.

`go finish()` is the single permitted call of a call-once binding, deferred: it
consumes the binding as a call does (§18.3).

Ownership, error, and async implications: no new ownership category; an owning
closure is a Move value that owns its captures, and a call-once call is a move
of the closure. Panics and `?` inside the body follow §16.4. Async interaction
remains open.

---

# 17. Async / Await

## 17.1 Async is core — LOCKED

`async` / `await` is part of the Zore MVP and core language design.

This supersedes any earlier idea that async might be deferred beyond MVP.

## 17.2 Async function syntax — LOCKED

```ore
async func fetchUser(id int) (User, error) {
    let response = await http.get("/users/" + id)?
    return parseUser(response)?
}
```

An async function type is written with `async` before `func`, as in
`async func(int) (User, error)`, and a declared `async func` name is a value of
that type (§16.2).

## 17.3 `await` semantics — LOCKED

`await` suspends the current async computation until the awaited operation can make progress / completes according to the async runtime contract.

Suspension must not violate ownership safety.

**Waiting operations.** Channel `send` and `receive` and `select` (§19),
`Mutex.withLock` (§20.2), and the waiting functions of the standard packages
(§37.3–§37.4) are ordinary calls, written without `await`. In an `async func` body such
a call suspends the task until it can proceed, exactly as an `await` does, so it
is a suspension point for §17.5 and §17.6. In a function that is not async, the
same call blocks the calling thread, as `task.wait()` does (§18.9). `await` still
applies only to calls of `async func`s and to `Task` values (§17.8). The body of
a closure is a non-async body even when the closure is written inside an
`async func`, so a waiting call in it blocks the thread.

## 17.4 Unified ownership — LOCKED

Async functions use exactly the same:

- Copy
- Move
- Borrow
- `mut`
- `own`
- Drop

rules as synchronous functions.

## 17.5 Values across suspension — LOCKED

If a value is still needed after an `await`, the compiler/runtime representation must preserve it across suspension.

Conceptually:

```text
value
  ↓
async state
  ↓
suspend
  ↓
resume
  ↓
use
  ↓
drop when lifetime ends
```

## 17.6 Borrowing across `await` — LOCKED CONSERVATIVE RULE

A borrowed value may cross an `await` only when the compiler can prove that the borrow remains valid across suspension.

The MVP compiler is allowed to be conservative.

If safety cannot be proven, compilation must fail rather than guessing.

This applies especially to:

- mutable borrows
- borrowed stack data
- values whose owner may cease to exist while the async task is suspended

## 17.7 Async lowering — IMPLEMENTATION DETAIL

Async functions lower to compiler-generated state machines (§35.1). A task runs by
being polled: the runtime resumes the state machine at its saved state, and the
state machine either finishes or suspends again. No task needs a stack of its
own. How locals are laid out in the saved state, how tasks are scheduled, and
how a waiting call wakes its task are implementation details. The bootstrap
compiler keeps storage needed after a suspension or requiring a stable address
in a pinned heap frame. Proven poll-local storage may instead use the poll stack.
Tasks are polled on a pool of worker threads.
The compiler may insert cooperative scheduling points into async state machines
without changing source syntax or the ownership rules.

The exact ordering between:

- HIR
- ownership checking
- async lowering
- MIR
- drop insertion

is **not** a locked language-semantic decision.

Compiler implementers must preserve the locked observable behavior while being free to refine the internal pipeline.

## 17.8 Async call contract — LOCKED

A call to an `async func`, or through a value of async function type, must be
the operand of `await` or of `go`. Any other
use — a bare call statement, binding the call's result, or passing it as an
argument — is a compile-time error. There is no user-visible future or promise
type; an async call's result can only be obtained by awaiting it or by
spawning it and later retrieving it from the task (§18.9).

`await` is valid only inside the body of an `async func`. Its operand is
either a call to an `async func` or to a value of async function type (§16.2),
or a `Task<...>` value (§18.9). Using `await`
anywhere else — including in a synchronous function or a non-async closure —
is a compile-time error. A synchronous function reaches async work only by
spawning it with `go`.

```ore
async func loadProfile(id int) (Profile, error) {
    let user = await fetchUser(id)?        // valid: awaited async call
    let task = go fetchAvatar(id)          // valid: spawned async call
    let avatar = await task?               // valid: awaited task
    return Profile{User: user, Avatar: avatar}, nil
}

func refresh(id int) {
    fetchUser(id)                          // invalid: async call neither awaited nor spawned
    let user = await fetchUser(id)         // invalid: await outside an async func
}
```

Ownership, error, and async implications: awaiting or spawning an async call
applies the ordinary ownership rules to its arguments (§17.4, §18.4). Error
results obtained through `await` follow §15.2 and §15.6.

Compiler impact: classify each call by the callee's `async` property and reject
async calls outside `await`/`go` operand position; reject `await` outside async
function bodies with a diagnostic naming the enclosing function. Pending
conformance cases: `tests/conformance/concurrency.md`.

---

# 18. Tasks and `go`

## 18.1 Explicit concurrent work — LOCKED

`go` creates concurrent work.

Example:

```ore
go process(user)
```

## 18.2 Task handles — LOCKED

A spawned computation may produce a task handle:

```ore
let task = go calculate()
let result = task.wait()
```

## 18.3 Async work and tasks — LOCKED

Async operations may be spawned:

```ore
let task = go fetchUser(123)
let user = task.wait()?
```

`go` accepts a call to any declared function or method, async or not. The
callee may also be a closure literal written in place or a local of function
type; the callable is evaluated first, then the arguments left to right, each
exactly once in the spawner, and only then is the task created. The callable is
moved into the task: a local used as the callee is unusable afterward, and a
call-once closure may be spawned. The task owns the closure from creation, calls
it once, and then destroys it, so every captured value is destroyed exactly once
on a normal return, an error result, or a panic. A callee that is a field,
element, map value, or the result of a call is rejected: move a closure stored
in a field of a local struct into a local first, and take one out of an
`Array<T>` or a map with `pop` or `remove`. A closure body is never
async (§16.1), so a spawned closure runs as one plain-function task even when it
is written inside an `async func`. `go` also accepts a call through a value of
async function type (§16.2): the callable is moved into the task, and the task
resumes as for a declared `async func`. `go asyncFn(x)` on the declared name is
unchanged.
A call to an `async func` becomes a task that the runtime resumes as it becomes
ready. A call to a plain function runs to completion on a runtime thread as one
task. A
plain-function task that waits holds a thread while it waits (§17.3, §18.9), so
programs that create very many tasks should make the task functions `async`;
the calls inside them stay as they are.

## 18.4 Task ownership rules — LOCKED

When values enter a spawned task:

1. Copy values are copied into the task as needed.
2. Values passed with ownership transfer are moved into the task.
3. Borrowed values are allowed only if the compiler can prove the borrow remains valid for the task lifetime.
4. Mutable borrows require exclusive access until the borrow ends / task completes.
5. Closure captures follow the same rules.

**Spawned closures.** A closure that is spawned is owning (§16.4), so its
captures are values: a Copy capture is copied, and a Move capture is moved in
and unusable in the spawner afterward. These captures are rejected: a borrowed
parameter that is a Move value, a local that holds a view, a `mut` parameter or
exclusively captured value, and a borrowing closure. Assigning, compound
updating, or passing to a `mut` parameter a captured Copy local inside a
spawned closure is rejected at the change, because the task would change only
its own copy; a task starts from a copy inside its body (`var local = n`) or
shares a change through a channel or a `mutex`. A captured Move value may be
changed in the task. An argument of function type for an `own` parameter of the
spawned callee is accepted when it is an owning closure none of whose captured
values holds a view; a function-typed argument for a shared parameter, and a
closure that holds a view, are rejected. A function value that this function received through a parameter is rejected as a spawned callable or as a spawned `own` argument, because its captures are unknown here; spawn it in the function that creates the closure. A task result still cannot be a slice,
hold a view, or be a function value (Q25).

**Lifetime proof across all exits.** A planned `.wait()` or `await task` is not
proof that a spawned borrow is valid: normal early return, `?`, panic, handle
transfer, and handle drop can all leave the work running. The MVP rejects
spawned borrows of another task's local or temporary storage, including its
borrowed parameters, even when retrieval immediately follows spawning. There
is no implicit join on an error or unwind path. Scoped tasks remain a separate
unresolved extension (Q10), not an exception to this rule.

Inputs must instead be independently valid for the spawned computation:
Copy inputs are materialized in task-owned argument storage before the work
can outlive the caller; `own` inputs are transferred. Shared parameters of the
spawned callee may borrow those task-owned Copy argument values for the call.
A Move input to an ordinary shared parameter is not silently moved or cloned;
use an `own` parameter to transfer it. A `mut` parameter cannot be satisfied by
copying its argument, since that would change the caller-visible mutation
contract. Copying a slice or a wrapper containing a slice is not independent
storage for its backing elements (§11.7).

Borrowed input is allowed only if its backing storage and access rights are
proven valid independently of the spawner, for the full task lifetime. No
package storage is assumed immortal while package lifetime rules remain open
under Q05. Unknown provenance is rejected. These requirements apply recursively
to captures and composite inputs. Task results must likewise survive releasing
the task's argument/local storage: a view into that storage cannot escape via
retrieval. Copy/Move classification alone is never a lifetime proof.

```ore
func inspect(values Array<int>) { /* shared borrow */ }
func consume(values own Array<int>) { /* transferred ownership */ }

func start(values own Array<int>) {
    let task = go inspect(values)  // rejected: borrows start's owned local storage
    task.wait()                   // does not make the spawn safe
}

func startOwned(values own Array<int>) {
    let task = go consume(values)  // valid: task owns the array
    task.wait()
}
```

Ownership, error, and async implications: the task's independent inputs remain
valid if the parent returns through `?` or unwinds. Detachment and process-exit
behavior are unchanged. Awaiting an async call directly is not spawning; its
borrows follow §17.6 and must remain valid through that call's cleanup.

Compiler impact: validate recursive input and output provenance at spawn,
materialize Copy arguments in task storage, and reject dependencies on another
task's stack/owned locals even when a normal-path retrieval is visible. Include
unwind and early-return paths in lifetime checking. Pending conformance cases:
`tests/conformance/concurrency.md`.

## 18.5 Detached tasks — LOCKED

Detached `go ...` work may continue independently.

There is no implicit cancellation merely because the spawning scope exits.

## 18.6 Dropping a Task handle — LOCKED

Dropping a `Task` handle detaches the handle from the running work.

It does not implicitly cancel the task.

## 18.7 Task errors — LOCKED

A task may return an error-bearing result.

Example:

```ore
let task = go load()
let user, err = task.wait()
```

## 18.8 Task typing and classification — LOCKED

`go f(args)` has type `Task<R1, ..., Rn>`, where `(R1, ..., Rn)` is the
declared result list of `f`. For an `async func`, that is its declared result
list, not a future type. A spawned call with no results has type plain `Task`.
Two task types are identical iff they have the same number of type arguments
and each argument is the identical type. A written `Task<...>` type must mirror
a valid result list, including the `error`-position rule in §7.2: `Task<User,
error>` is valid, `Task<error, User>` is not. This is a built-in type form like
`Array<T>` and `channel<T>`; it does not introduce user-defined generics
(§22.1).

`Task<...>` is **Move**. A task is one computation with a one-shot result;
unlike a channel handle (§19.3), a task handle is never shared by copying. Its
ownership may be transferred — passed to an `own` parameter, stored in a
struct field or `Array<Task<...>>`, or sent over a channel — under ordinary Move
rules. Its zero value is `nil` (§41.4).

```ore
func collect(tasks own Array<Task<int>>) int {
    // each element's result type is known: int
    ...
}

let task Task<User, error> = go load()   // annotation mirrors load()'s results
```

Ownership, error, and async implications: dropping a task handle detaches it
(§18.6); being Move, this happens exactly once per task.

Compiler impact: derive the task type from the spawned callee's result list;
check `Task<...>` annotations against the §7.2 shape rule; classify `Task<...>`
as Move. Pending conformance cases: `tests/conformance/concurrency.md`.

## 18.9 Retrieving task results — LOCKED

A task's results are retrieved in one of two forms, each with exactly one
meaning:

- **`task.wait()`** blocks the caller until the task completes, then
  returns its results `(R1, ..., Rn)`. It is valid only outside an `async func`
  body. The examples in §18.2–18.7 are synchronous uses.
- **`await task`** suspends the calling async computation until the task
  completes, then evaluates to its results. Like every `await`, it is valid only
  inside an `async func` body (§17.8).

Writing `task.wait()` directly inside an `async func` body is a compile-time
error; use `await task`. Both forms compose with `?` when the last result is
`error` (§15.2, with `await task?` grouping as `(await task)?` per §7.6).

Both forms **consume** the task handle: `wait` has an `own` receiver, and
`await task` moves its operand. A task's results can therefore be retrieved at
most once; a second retrieval through the same binding is an ordinary
use-after-move error (§23.2). Retrieving results from a `nil` task panics.

```ore
func main() {
    let task = go compute()
    let total = task.wait()        // valid: synchronous context, blocks
    let again = task.wait()        // invalid: task was moved by the first wait
}

async func combine() (int, error) {
    let a = go partA()
    let b = go partB()
    let x = await a?               // valid: async context
    let y = b.wait()               // invalid: .wait() inside an async func
    return x, nil
}
```

**Runtime progress guarantee.** A blocking `task.wait()` must never prevent
other tasks from making progress. When `.wait()` blocks a scheduler worker —
including when a synchronous helper is called from async code and waits inside
it — the runtime must continue running other ready tasks, for example by
handing the blocked worker's work to another worker. Blocking may cost an
extra OS thread; it must not cause scheduler starvation. The compile-time rule
above rejects the direct case; this guarantee covers the indirect case the
compiler cannot see. It does not prevent logical deadlocks the program itself
creates, such as a task waiting on its own handle.

Ownership, error, and async implications: results are transferred to the
retriever under ordinary ownership rules. A task's error result is subject to
§15.6 when retrieved. A detached task's results, including any `error`, are
discarded when it completes; discarding the handle is the explicit
acknowledgment (§5.5).

Compiler impact: type `wait` and `await task` from the task's type arguments;
reject `.wait()` inside async bodies; treat both as moves of the handle.
Pending conformance cases: `tests/conformance/concurrency.md`.

## 18.10 Panics in tasks — LOCKED

A panic unwinds only the stack of the task in which it occurs, running that
task's drops (§15.4). What happens next depends on the task:

- **The program's initial task** (the one running the entry point): the
  process terminates after unwinding. Other tasks are abandoned as in §18.11.
- **A spawned task:** the task ends. When its handle is retrieved with
  `.wait()` or `await`, the panic is raised again in the retrieving task at the
  retrieval point, which then unwinds in turn. If the handle is never retrieved
  — detached, or dropped — the process continues running.

The runtime reports every panic, with its message and originating task, on
standard error when it occurs. The report format is implementation-defined;
the process exit status after an initial-task panic is specified in §3.19.

This does not make panics catchable (§15.4): no operation turns a panic into a
value, and no code resumes after the panicking point. A task boundary only
contains the unwinding. Containment is sound under the ownership model because
spawned work cannot borrow another task's locals merely on the strength of a
later retrieval (§18.4). Its input storage remains valid independently of
whether its spawner returns, unwinds, or detaches it. State shared through a mutex remains the
other channel of observation; a mutex held by a task that panics must be
marked poisoned (§20.2).

```ore
func main() {
    go handleRequest(badInput)   // detached: its panic is reported, main continues

    let task = go compute()
    let value = task.wait()       // if compute panicked, main panics here
}
```

Ownership, error, and async implications: each task's unwinding follows
§15.4. Panic is unrelated to `error` results; a panicked task never produces an
`error` value for its waiter.

Compiler impact: none beyond §15.4; the runtime tracks per-task panic state
and re-raises at retrieval. Pending conformance cases:
`tests/conformance/concurrency.md`.

## 18.11 Process exit with running tasks — LOCKED

When the program's initial task completes — normally or by panic — the process
terminates immediately. Tasks still running are abandoned where they are: their
pending drops do not run, and values in channel buffers are not dropped. This
follows from detached work continuing independently only while the process
runs (§18.5); it does not promise that detached work finishes. An implicit
join-all at exit is rejected, since a single infinite background loop would
then prevent the program from ever exiting.

A program that needs a task to finish — including its cleanup — keeps the
task's handle and retrieves it before the entry point returns.

Ownership, error, and async implications: deterministic cleanup (§1.1) applies
to values whose lifetime ends while the process runs; abandonment at exit is
a stated exception, alongside the abort cases in §15.4 and channel-handle
cycles in §19.13. The entry-point
signature and exit-status rules are specified in §3.19.

Compiler impact: none; this is a runtime contract. Pending conformance cases:
`tests/conformance/concurrency.md`.

---

# 19. Channels

## 19.1 Channel role — LOCKED

Channels are the primary built-in mechanism for safe message passing between concurrent tasks.

## 19.2 Channel creation — LOCKED

Unbuffered:

```ore
let ch = channel<User>()
```

Buffered:

```ore
let ch = channel<User>(10)
```

## 19.3 Channel handles — LOCKED

Channel handles are Copyable.

Copying a channel value copies a handle to the same synchronized underlying channel.

It does not copy:

- queued messages
- channel state
- the underlying communication object

This behavior intentionally allows multiple workers to share a channel handle.

Example:

```ore
go worker(ch)
go worker(ch)
go worker(ch)
```

## 19.4 Send — LOCKED

```ore
ch.send(value)
```

For Move values, sending transfers ownership into the channel.

Example:

```ore
ch.send(user)
// user is moved and cannot be used here
```

For Copy values, the value is copied according to normal Copy semantics.

Copying or moving a message never erases contained borrow provenance (§11.7).
A channel can retain messages after the sending scope or task ends. A sent
value must therefore be independently valid for retention and later receipt:
reject any contained view borrowing a sender's local, temporary, or borrowed
parameter storage. Neither an unbuffered send nor a later receive proves that
the receiver has finished using the message. Unknown backing provenance is
rejected; recursively owned values and independent Copy values such as strings
and channel handles remain valid message contents. An external borrowed view
requires proof of backing lifetime and access rights independent of the sender
and all channel retention; no package-lifetime assumption is supplied by Q05.

Ownership, error, and async implications: message backing remains valid across
sender return, `?`, panic, and suspension. Ordinary transfer and closed-send
cleanup are unchanged. Sending a wrapper around a local slice is rejected;
sending an owned array of independent elements transfers it normally.

Compiler impact: check recursive message provenance at send, including through
Copy structs and owned containers. Pending conformance cases:
`tests/conformance/concurrency.md`.

## 19.5 Receive — LOCKED

```ore
let value, ok = ch.receive()
```

`ok` indicates whether a value was successfully received.

## 19.6 Unbuffered semantics — LOCKED

An unbuffered send waits until a matching receive can accept the value.

## 19.7 Buffered semantics — LOCKED

A buffered send waits when the channel is full.

A receive waits when the channel is empty and still open.

## 19.8 Channel close — LOCKED

```ore
ch.close()
```

Closing a channel means no more values may be sent.

Buffered values already present remain receivable.

After the buffer is drained, receive returns the zero value plus `false`:

```ore
let value, ok = ch.receive()
```

where `ok == false`.

The zero-value model for all types is locked in §41.4. Do not invent additional receive-result syntax.

## 19.9 Send on closed channel — LOCKED

Sending on a closed channel causes a runtime panic.

## 19.10 Worker example — LOCKED INTENT

```ore
func worker(ch channel<User>) {
    for {
        let user, ok = ch.receive()
        if !ok {
            return
        }

        process(user)
    }
}
```

This demonstrates the intended channel-sharing and close behavior.

Supported `for` forms are specified in §5.10; this example does not introduce
additional loop syntax.

## 19.11 Close repetition and blocked operations — LOCKED

Closing an already-closed channel panics, consistent with send on a closed
channel (§19.9). Channel handles are Copy and shared across tasks (§19.3), so
this cannot be checked at compile time in general.

Closing a channel wakes every task blocked on it:

- A **sender** blocked waiting — on an unbuffered channel for a receiver, or on
  a full buffered channel for space — panics at its send, as if it had sent on
  the closed channel. It does not block forever.
- A **receiver** blocked on an empty channel returns the zero value plus
  `false` (§19.8).

```ore
let ch = channel<Job>(4)
ch.close()
ch.close()         // runtime panic: channel already closed
```

Ownership, error, and async implications: the Move value held by a sender that
panics this way is still owned by the sender and is dropped during its unwind
(§15.4); it was never transferred into the channel.

Compiler impact: none; runtime behavior. Pending conformance cases:
`tests/conformance/concurrency.md`.

## 19.12 Channel zero value — LOCKED

Channels have no `nil` state. The zero value of `channel<T>` (§41.4) is an
**always-closed, empty channel**. Every operation on it is already defined by
the closed-channel rules:

- `receive()` returns the zero value of `T` plus `false` immediately (§19.8);
- `send(value)` panics (§19.9);
- `close()` panics (§19.11).

Whether the runtime shares one such channel per element type or creates one
per zero value is an implementation detail; the behavior is identical. A
channel value is therefore always a real channel, and channel values are not
comparable (§6.6). Channels would be disabled in a `select` by an explicit case guard (not part
of §19.14), not by a `nil` channel.

Ownership, error, and async implications: the zero-value channel holds no
buffered values and needs no cleanup.

Compiler impact: reject `nil` as a channel value and in channel comparisons;
lower zero-value channels to an always-closed channel. Pending conformance
cases: `tests/conformance/concurrency.md` and
`tests/conformance/zero-values.md`.

## 19.13 Channel lifetime and buffered values — LOCKED

An underlying channel stays alive while any handle to it exists anywhere —
in a binding, a struct field, a task's captured values, or another channel's
buffer. When the last handle is gone, each value still in its buffer is
dropped exactly once; the order among them is unspecified, since the channel
queue implementation is runtime freedom (§36.2). The mechanism that detects
the last handle — for example internal reference counting, which §47.2 already
permits for channel handles — is an implementation detail. Handles stay Copy from the programmer's perspective,
following the same precedent as `string` (§41.5); the rule that a user-defined
`drop` forces Move (§8.3) applies to user-defined types, not to this runtime
bookkeeping.

**Stated limitation — handle cycles.** If channel handles form a cycle through
buffers — a channel's buffer holding a handle to that same channel, directly or
through other channels — the channels in the cycle are never freed while the
process runs, and their buffered values are never dropped. This is a
memory-safe leak, not undefined behavior. Detecting such cycles would require a
tracing collector, which §23.1 excludes; banning channel handles inside channel
messages would break the common pattern of sending a reply channel with a
request. Values in any channel buffer at process exit are also not dropped
(§18.11).

Ownership, error, and async implications: dropping buffered values uses
ordinary drop rules (§14). Deterministic cleanup (§1.1) covers the buffered
values of every channel that is not part of a handle cycle.

Compiler impact: none; runtime behavior. Pending conformance cases:
`tests/conformance/concurrency.md`.

## 19.14 `select` — LOCKED

`select` waits on several channel operations at once and runs the body of one
that can proceed.

```ore
select {
    case let job, ok = jobs.receive() {
        if !ok {
            return
        }
        process(job)
    }
    case results.send(summary) {
        println("sent")
    }
    default {
        println("nothing ready")
    }
}
```

Grammar: `select` `{` arm... `}`, where an arm is one of

- `case let targets = channel.receive() { body }`, which binds the received
  value and whether one was received exactly as `let targets = channel.receive()`
  does (§19.5, §5.4), including `_` targets;
- `case channel.receive() { body }`, which discards both results;
- `case channel.send(value) { body }`;
- `default { body }`.

`case` and `default` are special only at the start of an arm inside a `select`;
elsewhere they are ordinary identifiers, so `select` is the only new keyword
(§3.17). Each arm's body is an ordinary block with its own scope, written on the
same line as the end of its header like an `if` body (§5.9), and struct literals
in a header need parentheses for the same reason as in `if`. There is at least
one `case`, and at most one `default`. A `select` is a statement.

Semantics:

- When the `select` begins, the channel operand of every `case` and the value
  of every `send` case are evaluated once, in source order, before any case is
  chosen. A Move value of a `send` case is moved into the `select` at that
  point.
- A `receive` case **can proceed** when the channel has a buffered value, when a
  sender is waiting on it, or when it is closed or the zero-value channel
  (§19.8, §19.12). A `send` case can proceed when a receiver is waiting, when
  there is buffer space, or when the channel is closed or the zero-value
  channel; choosing it then panics as a send on a closed channel does (§19.9).
- If one or more cases can proceed, one of them is chosen and its operation is
  performed; which one is unspecified, and the implementation must not always
  prefer the same case. If none can proceed and there is a `default`, the
  `default` body runs without waiting. If none can proceed and there is no
  `default`, the task waits until one can, then performs it. Other tasks keep
  running while it waits (§17.3), and a `select` whose cases can never proceed
  is a deadlock when nothing else can run (§18.9).
- Exactly one case's operation happens, then its body runs. The values of
  `send` cases that were not chosen were never sent: they are dropped when the
  `select` ends, in reverse source order, before the chosen body runs.
- `break` and `continue` inside an arm refer to the innermost enclosing `for`
  loop, as they do inside an `if`: a `select` is not a loop. `return` and `?`
  inside an arm behave as anywhere else.
- A `select` can be used in `async` functions and ordinary functions alike.

Ownership, error, and async implications: a chosen `receive` transfers the
value to the arm's binding under ordinary rules; its Move value is dropped when
the arm's scope ends. A panic from a chosen send on a closed channel drops the
sent value first, then unwinds (§19.9). A task waiting in a `select` counts as
blocked for deadlock detection (§18.9).

Compiler impact: parse the new statement, accept only channel `send` and
`receive` calls as case operations, evaluate operands before the choice, bind
the arm's results in the arm's scope, drop unchosen send values, and keep
`break`/`continue` targeting the enclosing loop. Pending conformance cases:
`tests/conformance/select.md`.

---

# 20. Shared Mutable State

## 20.1 Direct unsynchronized shared mutation — LOCKED OUT

Zore should not permit ordinary data to become freely shared mutable state across tasks without an explicit synchronization mechanism.

## 20.2 Mutex — LOCKED

A `Mutex<T>` guards one value of type `T` that several tasks may use. It is a
built-in type form like `Array<T>` and `Task<...>` (§18.8): it introduces no
user-defined generics (§22.1), and `Mutex` and `mutex` are predeclared names
(§3.18).

```ore
let counter = mutex(0)                  // Mutex<int>

counter.withLock(func(value mut int) {
    value += 1
})

let total = counter.withLock(func(value mut int) int {
    return value
})
```

- `mutex(value)` creates a mutex holding `value`, whose type `T` the argument
  gives (an untyped constant takes its default type, §6.5). The value is
  moved into the mutex if it is a Move value.
- `m.withLock(f)` waits until no other task holds the lock, then calls `f` with
  a `mut T` borrow of the guarded value and releases the lock when `f` returns.
  `f` must be a function value of type `func(mut T) R1, ..., Rn`; the call has
  the type of `f`'s result list and returns its results. The borrow ends when
  `f` returns: `f` cannot keep or return a view of the value, and its results
  cannot contain a slice or a function value (§11.7).
- `m.isPoisoned() bool` tells whether a task panicked while holding the lock.
- A `Mutex<T>` handle is Copy and refers to one shared lock and value, as a
  channel handle does (§19.3); copying it copies no value. This revises the
  listing of `Mutex` among the Move types in §10.3: the resource is the shared
  cell, and handles are counted like channel handles (§19.13), so the guarded
  value is dropped once, when the last handle is gone. Handle cycles through
  guarded values leak, as for channels. `T` cannot contain a slice or a function
  value, so the guarded value never borrows from a task's locals.
- A task that waits for the lock suspends only itself (§17.3), and waiters are
  served in order of arrival. The lock is not reentrant: calling `withLock` on a
  mutex from inside its own `f` waits forever, which is a deadlock (§18.9). `f`
  may wait, for example on a channel, while it holds the lock; `f` is an ordinary
  closure, so such a wait blocks the thread (§17.3).
- If a task panics while `f` runs, the unwind releases the lock and marks the
  mutex **poisoned** (§18.10). Every later `withLock` on a poisoned mutex
  panics instead of handing out a value that may have been left half-updated;
  `isPoisoned` is the way to check first. Poison cannot be cleared.
- The zero value of `Mutex<T>` (§41.4) has no lock: `withLock` on it panics, and
  `isPoisoned` returns `false`. `nil` is not a mutex, and mutexes are not
  comparable.
- Accessing guarded state otherwise than through `withLock` is not possible, so
  ordinary data never becomes freely shared mutable state (§20.1) and no
  user-facing pointer or dereference syntax is needed.

```ore
func worker(counter Mutex<int>, done channel<bool>) {
    for var i = 0; i < 1000; i += 1 {
        counter.withLock(func(value mut int) { value += 1 })
    }
    done.send(true)
}
```

Ownership, error, and async implications: handles pass between tasks by copy
(§18.4), so `go worker(counter, done)` shares the mutex. The closure runs while
the lock is held and follows the ordinary closure rules (§16): it may borrow the
caller's locals for the call, but cannot move a captured Move value out, and may
use `?` only if it returns a trailing `error`. A panic inside `f` follows §15.4.

Compiler impact: type `Mutex<T>` and `mutex(value)`, check that `f` has the type
`func(mut T) R...` and that `T` and `R...` hold no views, lower `withLock` to a
lock, a call, and an unlock that marks the mutex poisoned when the call raises a
panic, and release a handle's share when it is dropped. Pending conformance
cases: `tests/conformance/mutex.md`.

---

# 21. Raw Pointers and Unsafe Code

## 21.1 Raw pointers — OUT OF MVP

The MVP does not expose user-facing raw pointers.

## 21.2 Pointer arithmetic — OUT OF MVP

Pointer arithmetic is not part of the MVP.

## 21.3 `unsafe` — OUT OF MVP

An `unsafe` facility may be introduced later for:

- raw pointers
- FFI
- manual memory operations
- architecture-specific operations

It is not part of the MVP.

Compiler/runtime internals may of course use low-level operations internally.

---

# 22. Generics and Interfaces

## 22.1 Generic functions — LOCKED

A generic function declares type parameters in angle brackets after its name.
Each type parameter has exactly one constraint, which says which types it can
stand for and what the body may do with its values:

```ore
func Max<T ordered>(a T, b T) T {
    if a > b {
        return a
    }
    return b
}

func Contains<T comparable>(items []T, target T) bool {
    for _, item in items {
        if item == target {
            return true
        }
    }
    return false
}
```

**Declaration.** The list `<T C, U D, ...>` follows the function name and holds
at least one parameter. Each parameter is a name followed by its constraint;
the names are in scope in the signature and the body, and they cannot shadow
predeclared names (§3.18). A type parameter can appear anywhere a type can,
including inside `[]T`, `Array<T>`, `map[K]V`, `channel<T>`, `Mutex<T>`, and
`[T; N]`. Methods cannot declare type parameters, a function declared without a
body (§37) cannot be generic, and `main` cannot be generic. An `async func` can
be generic. Generic types are not part of this decision; only functions are.

**Constraints.**

| Constraint | Allowed type arguments | What the body may do with a `T` value, besides passing, returning, storing, and moving it |
| --- | --- | --- |
| `any` | every type allowed below | nothing more |
| `copyable` | every Copy type (§10.2) | copy it |
| `comparable` | `bool`, integer types, `rune`, `string`, and named types built on one: exactly the map key types (§13.3) | copy it, compare it with `==` and `!=`, and use it as a map key |
| `ordered` | integer and float types, `rune`, `string`, and named types built on one | copy it, and compare it with `==`, `!=`, `<`, `<=`, `>`, and `>=` |
| an interface type (§22.2) | types that satisfy the interface, including interface types | call the interface's methods |

`any`, `copyable`, `comparable`, and `ordered` are predeclared names (§3.17)
usable only as constraints; none of them is a type. No type argument can be a
function type, a `mut []T`, a borrowed interface value, or a type that holds one
of these. Floats are not `comparable`, as they are not map keys.

**Calls.** A call writes no type arguments; they come from the argument types.
Each parameter type is matched against its argument's type, and a type
parameter takes the type found in its place, from the first argument that
decides it. An untyped constant decides a type parameter only when no typed
argument does; the first such constant then gives its default type (§6.7). Arguments are then
checked against the parameter types with the type arguments in place, as for
any call. A call is an error when a type parameter is not decided or a type
argument does not satisfy its constraint; the diagnostic names the type
parameter.

```ore
let biggest = Max(3, 9)                 // T = int
let found = Contains(names[:], "Ada")   // T = string
Max(1.5, 2)                             // T = float64; 2 converts
```

`Max<int>(1, 2)` is not a call syntax: it parses as comparisons. A generic
function cannot be used as a value. Inside a generic function, a type parameter
can be passed on to another generic call when its own constraint promises as
much as the callee needs; an interface constraint passes on to an interface it
satisfies.

**Checking.** The body is checked once, at the declaration, for every type its
constraints allow, whether or not the function is called. Under `copyable`,
`comparable`, and `ordered`, `T` values are copied; under `any` and an interface
constraint, `T` is treated as a Move type, so a `T` cannot be moved out of a
slice, field, or element, or used after it was moved. Each set of type arguments
then gets its own copy of the function, compiled like ordinary code. A generic
function whose calls would need endless new sets of type arguments, such as one
calling itself with `Array<T>`, is an error.

**Not yet supported.** Inside a generic function: function literals, `go`,
method values, converting a `T` value to an interface type, `clone` of a `T`,
and printing a `T`. These produce diagnostics.

Ownership, error, and async implications: generic code follows the ordinary
ownership, borrowing, view, and cleanup rules for each type parameter as its
constraint describes, and a copy for particular type arguments behaves exactly
like a function written for those types, including destroying values and running
custom `drop` methods. Calling a method through an interface constraint uses
the receiver with the entry's mode, as a call through an interface value does
(§22.2). Errors propagate with `?` as usual. A call to a generic `async func`
must be awaited or spawned (§17.8) and suspends like any async call.

Compiler impact: parse type parameter lists in both parsers; declare type
parameters and the four predeclared constraints in the resolver; add a type
parameter type kind classified by its constraint; infer type arguments at calls
and check them against constraints; check generic bodies once; make one copy per
set of type arguments before MIR, and check an extra copy with stand-in types
for ownership so errors in uncalled functions are found; compile only the
copies. Pending conformance cases: `tests/conformance/generics.md`.

## 22.2 Interfaces — LOCKED

An interface type is a list of methods. A type that has those methods satisfies
the interface without declaring it:

```ore
type Reader interface {
    Read(buf mut []byte) (int, error)
}

type Closer interface {
    own Close() error
}

type Counter interface {
    Count() int
    mut Add(n int)
}
```

**Declaration.** `type Name interface { ... }` declares an interface type in
package scope, exported by the usual rule (§4.1). Each entry is a method name,
its parameters with their names, types, and modes (§7.3), and its results, as in
a method declaration without a receiver. The receiver's mode comes before the
name: nothing for a shared borrow, `mut` for a mutable borrow, and `own` for
ownership transfer. An entry may start with `async` (§17.2). An interface
declares at least one method, each name once. `drop` and `clone` cannot be
entries, an interface does not list other interfaces, and methods cannot be
declared on an interface type.

**Satisfying an interface.** A type `T` satisfies interface `I` when, for every
entry of `I`, `T` has a method with the same name, the same parameter types and
modes in order, the same results, the same `async` property, and a receiver mode
the entry allows:

| Entry | Receiver modes of `T`'s method that satisfy it |
| --- | --- |
| shared | shared |
| `mut` | shared or `mut` |
| `own` | shared, `mut`, or `own` |

Only struct and named types (§8.5) have methods, so only they satisfy an
interface; predeclared types satisfy none. An entry whose name is not exported
is satisfied only by a method declared in the interface's package. An interface
type `J` satisfies `I` when `J` has an entry for every entry of `I` under the
same rules, with `J`'s entry in place of the method.

**Borrowed and owned values.** Where a value of interface type appears decides
what it is:

| Position | The value is | It can call |
| --- | --- | --- |
| A parameter `r Reader` | a shared borrow of the argument | shared entries |
| A parameter `r mut Reader` | an exclusive borrow of the argument | shared and `mut` entries |
| Anywhere else: an `own` parameter, a local initialized with a declared type, a field, a result, an element, a map value | an owned value | shared entries; `mut` entries through a mutable place (§11.6); `own` entries, which consume it |

A shared or `mut` interface parameter borrows its argument exactly as a
parameter of the argument's own type would: nothing is copied, moved, or
allocated, and a `mut` parameter needs a mutable place (§11.6). Inside the
function the parameter is a *borrowed interface value*, a view like a slice
(§12.1): copying it, passing it on, and capturing it follow the rules for
views, and it cannot be stored in a field, returned, or converted to an owned
value.

An owned interface value owns the value inside it. Converting a value to an
owned interface value moves a Move value (§10.3) or copies a Copy value into
storage the interface value owns. An owned interface value is always Move, even
when the value inside is Copy. The value inside cannot hold a view: converting a
value whose type contains a slice, a borrowed interface value, or a function
value to an owned interface value is an error.

**Conversion.** A value converts to an interface type implicitly wherever that
type is expected and the value's type satisfies it: an argument, an initializer
with a declared type, an assignment, a `return`, a field value, an element, a
map value, `push`, and a channel send. The value can be of a concrete type or
another interface type that satisfies the target. There is no conversion from an
interface type back to a concrete type, and no `T(v)` form for interfaces.

```ore
type File struct { name string }

func (f mut File) drop() {}         // a custom `drop` makes File a Move type
func (f File) Read(buf mut []byte) (int, error) { return 0, nil }
func (f own File) Close() error { return nil }

func fill(r mut Reader, buf mut []byte) (int, error) {
    return r.Read(buf)                 // calls File.Read through the interface
}

func main() {
    var file = File{name: "notes.txt"}
    var buf = [byte; 4]{0, 0, 0, 0}
    let n, err = fill(file, buf[:])    // borrows `file` exclusively for the call
    _ = n
    _ = err
    let closer Closer = file           // moves `file` into an owned value
    let closeErr = closer.Close()      // consumes `closer`
    _ = closeErr
}
```

**Calls.** `value.Method(args)` on an interface value calls the method of the
value inside. The receiver is used with the entry's mode, and the arguments
follow ordinary parameter rules (§7.3–§7.4). Calling an `own` entry consumes the
owned interface value: a method with an `own` receiver receives the value
inside, and a method with a weaker receiver borrows it, after which the value is
destroyed. A method value written on an interface value (`r.Read` without a
call, §16.2) and `go` on a call through an interface value are not part of this
decision.

**Restrictions.** Interface types are not comparable (§6.6), cannot be map keys
(§13.3), cannot be printed (§37.1), do not support `clone` (§10.7), and admit no
`nil` (§41.4). Their zero value is an *empty* value: destroying it does nothing,
and calling a method through it panics (§41.4).

Ownership, error, and async implications: destroying an owned interface value
destroys the value inside it exactly once, running its custom `drop` if it has
one (§14.3). A method result that holds a view is treated, at a call through an
interface value, as borrowing from the receiver and every borrowed argument
(§11.7). An owned interface value holds no views, so it can be passed to `go` as
an `own` argument, captured by a spawned closure, sent on a channel, and guarded
by a `Mutex` (§18.4, §19.4, §20.2); a borrowed interface value cannot. A call
through an interface adds no implicit error propagation or panic handling: `?`,
panics, and cleanup work as for a direct call. An `async` entry is satisfied
only by an `async` method, and a call through it must be awaited or spawned
(§17.8). A call through any other entry behaves exactly like a direct call of
the method inside: in an `async func` or a standard library function that
waits, a call that reaches a method that waits suspends the task (§17.7, §37.3),
and in a plain function it waits in place.

Compiler impact: declare interface types with their entries; check satisfaction
at each conversion and record which method serves each entry; represent a
borrowed or owned interface value as a pointer to the value inside, a pointer to
its drop flags, and a table of the methods that serve the entries, preceded by
the destructor of the value inside; give owned values heap storage; treat
borrowed values as views in the ownership and region analysis; generate the code
that adapts each method to its entry, including interface-to-interface
conversions, and for each entry a second adapter that starts the call for a
suspending caller: it builds the method's own resumable frame when the method can
wait, and a frame that is already finished otherwise. Pending conformance cases: `tests/conformance/interfaces.md`.

---

# 23. Memory and Resource Safety

## 23.1 No garbage collector — LOCKED

Zore's core runtime model does not depend on tracing garbage collection.

## 23.2 Ownership guarantees — LOCKED

The compiler must prevent, within safe Zore code:

- use after move
- use after explicit drop
- double drop
- overlapping mutable borrows
- mutation while incompatible shared access exists
- move while borrowed
- invalid borrowed access across task/async lifetimes

## 23.3 No separate concurrency ownership model — LOCKED

Tasks and channels must reuse ordinary ownership rules.

The compiler must not introduce a second set of move/borrow concepts that apply only to concurrency.

---

# 24. Compiler Diagnostics

## 24.1 Diagnostics are first-class — LOCKED

The compiler should produce high-quality, source-aware diagnostics.

Example:

```text
error[E0012]: use of moved value `user`

  --> main.ore:12:13
   |
10 | let user = loadUser()
   |     ---- value created here
11 |
12 | save(user)
   |      ---- value moved here
13 |
14 | println(user.Name)
   |         ^^^^ value used after move
   |
   = note: `User` is a move type
```

## 24.2 Ownership diagnostics — LOCKED EXPECTATION

Ownership-related diagnostics should identify, when available:

- where a value was created
- where it was moved
- where a borrow began
- where a conflicting use occurred
- why the type is Copy or Move
- what lifetime/task/await relationship caused the failure

Diagnostic wording may evolve; the quality requirement remains.

---

# 25. Compiler Semantic Model

This section describes the locked compiler architecture direction. These are implementation contracts, not necessarily visible language syntax.

## 25.1 Compiler stages — LOCKED DIRECTION

The compiler should conceptually contain:

```text
Source
  ↓
Lexer
  ↓
Parser
  ↓
AST
  ↓
Name Resolution
  ↓
Type Checking
  ↓
HIR
  ↓
Ownership / Borrow Analysis
  ↓
Async / Task Lowering
  ↓
MIR
  ↓
Drop Insertion
  ↓
LLVM
  ↓
Native Binary
```

The exact placement/order of async lowering relative to ownership checking and MIR is intentionally not frozen.

## 25.2 Separation of representations — LOCKED

The compiler should maintain the conceptual separation:

> **AST describes what the programmer wrote.**\
> **HIR describes what the program means.**\
> **MIR describes how the program executes.**

Do not put all compiler semantics directly into the AST.

---

# 26. Source Spans

## 26.1 Span model — LOCKED DIRECTION

Compiler nodes should retain source-location information sufficient for diagnostics.

Recommended internal representation:

```rust
struct Span {
    start: u32,
    end: u32,
}
```

Offsets should be byte offsets into a source file.

Line and column information may be computed by a source manager.

The exact Rust representation is not language semantics, but coding agents should preserve this architecture unless there is a strong implementation reason not to.

---

# 27. Symbol Identity

## 27.1 Strong IDs — LOCKED COMPILER DESIGN

After name resolution, semantic compiler passes should work with IDs rather than repeatedly resolving strings.

Examples:

```text
FunctionId
TypeId
StructId
FieldId
LocalId
PackageId
```

Conceptually:

```text
user       → LocalId(3)
loadUser   → FunctionId(8)
User.Name  → FieldId(2)
```

Strings remain useful for source display and diagnostics, but should not be the primary semantic identity after resolution.

---

# 28. Type Representation

## 28.1 Type IDs — LOCKED COMPILER DESIGN

Complex types should be represented through interned/stable compiler identities such as `TypeId`.

A type store may map:

```text
TypeId(0) → bool
TypeId(1) → int
TypeId(2) → string
TypeId(3) → User
TypeId(4) → File
```

This supports:

- equality
- Copy/Move classification
- layout
- code generation
- diagnostics

---

# 29. HIR

## 29.1 HIR purpose — LOCKED

HIR is a semantic representation produced after syntax/name resolution and used to express compiler-understood program structure.

HIR should contain resolved identities and type information.

It should not unnecessarily preserve source-level ambiguity.

## 29.2 Move/Copy in HIR — LOCKED DIRECTION

HIR may know ownership contracts, but explicit `Copy(place)` vs `Move(place)` operations are better represented in MIR.

Do not prematurely encode every ownership operation into the AST.

---

# 30. Place Model

## 30.1 Place abstraction — LOCKED COMPILER DESIGN

A `Place` identifies a storage location that may be:

- read
- copied
- moved
- borrowed
- mutably borrowed
- assigned
- dropped

Conceptually:

```text
user
user.Name
users[index]
```

may be represented as a base local plus projections.

Example:

```text
Place {
    local: user,
    projections: [
        Field(Name)
    ]
}
```

This abstraction is central to ownership and borrow analysis.

---

# 31. Ownership Analysis

## 31.1 Type classification vs value state — LOCKED

Do not confuse:

```text
this type is Move
```

with:

```text
this particular value has already been moved
```

These are different compiler concepts.

A type may have a classification such as:

```text
Copy
Move
```

while a value/place may have data-flow state such as:

```text
Available
Moved
PartiallyMoved
Borrowed
MutBorrowed
```

## 31.2 Partial moves and reinitialization — LOCKED

The internal ownership model must be capable of representing partial moves of
composite values even if the earliest implementation supports only
conservative cases. The compiler must not paint itself into an architecture
where only whole-variable ownership can ever be represented.

Source-level restriction: a move out of a proper subplace is rejected if any
containing value along its projection path defines a custom `drop`. This
includes nested fields and fixed-array elements inside that value. A destructor
must always receive its complete value; promising to reinitialize the field
later is insufficient because `?` or panic may run cleanup first.

Moving the entire destructor-bearing value is allowed. In particular, a field
whose own type defines `drop` may be moved whole out of an outer struct that
has no custom destructor, provided no other containing value along the path
defines one. Reading Copy fields and borrowing fields remain allowed under
ordinary mutability/exclusivity rules. Ordinary field replacement is allowed
only with a fully evaluated replacement: the old field remains initialized
while evaluating the RHS, and cleanup must never invoke the containing custom
destructor on a field that has already been destroyed if replacement cleanup
panics. A field becomes unavailable when its destruction begins; any panic
from that destruction therefore triggers this rule. That case aborts the
process, as does a panic during unwinding (§15.4).
This rule adds no field-extraction or replacement builtin.

For example, for `Guard` with a custom `drop` and a Move field `Resource`,
`let r = guard.Resource` is rejected, even if followed by reinitialization;
`let moved = guard` is allowed. For a destructor-free `Box` containing a
`Guard`, `let moved = box.Guard` is allowed, but
`let r = box.Guard.Resource` is rejected.

Source-level semantics for permitted partial moves: moving a single field out
of a struct (for example,
`let inner = outer.field` where `field` is Move-typed) leaves `outer` in the
`PartiallyMoved` value state (§31.1). While partially moved:

- still-available fields remain individually usable (read, borrowed, or
  moved) under ordinary rules;
- using the moved-out field again is an ordinary use-after-move error
  (§23.2);
- using `outer` as a whole value — passing it by borrow or by ownership
  transfer — is rejected until it is fully available again.

Assigning a new value to a moved-out field (`outer.field = newValue`)
reinitializes that field: the assignment does not attempt to drop the
previous value, since it was already moved out and no longer exists in that
slot, consistent with the existing rule that an already-moved value is never
dropped again (§5.6, §14.6). Once every field of `outer` is available —
whether never moved or reinitialized — `outer` is available as a whole again.
At scope exit, automatic cleanup (§14.1, §14.5) runs only for fields still
available; a moved-out field that was never reinitialized is skipped, per
§14.6's rule that only the current owner cleans up a value.

This tracking applies only to fixed field-access paths (struct fields, and a
constant-indexed element of a fixed array), matching the place abstraction in
§30.1. It does not extend to a computed/runtime index into a slice,
`Array<T>`, or map: those remain conservatively all-or-nothing for move
purposes, consistent with §5.6's existing refusal to prove indexed places
disjoint from each other. The builtin map `remove` (§13.3) is instead an
explicit operation that detaches an entry and transfers its value, leaving a
valid map rather than a partially moved entry; it does not relax indexed-move
restrictions.

Ownership, error, and async implications: partial-move tracking does not
change Copy/Move classification, cleanup order beyond field-level skipping, or
borrow exclusivity (§11.3). A partially moved value crossing an `await` is
subject to the same conservative borrowing-across-suspension rule as any other
borrowed or owned value (§17.6).

Compiler impact: represent field-level move state per place (§30.1), track
reinitialization as restoring per-field availability, and reject whole-value
uses while any reachable field is unavailable. Before accepting a projected
move, inspect every containing type on its path for custom `drop`; diagnose the
move and the destructor that requires an intact value. Preserve initialized
state during field replacement and enforce the abort rule above if destruction
fails after invalidating a field needed by a containing destructor. These rules
apply equally on normal, `?`, panic, and async paths. Pending conformance cases:
`tests/conformance/ownership.md`.

---

# 32. Borrow Representation

## 32.1 Borrow objects — LOCKED COMPILER DIRECTION

Internally, a borrow should be representable with information equivalent to:

```text
borrow id
borrowed place
borrow kind
region/lifetime
```

Borrow kinds:

```text
Shared
Mutable
```

Regions/lifetimes remain compiler-internal.

---

# 33. MIR

## 33.1 MIR form — LOCKED

MIR should be control-flow-graph based.

A function consists of:

- locals
- basic blocks
- statements
- terminators

## 33.2 MIR operands — LOCKED

MIR should be able to explicitly distinguish:

```text
Copy(place)
Move(place)
Constant(...)
```

This is critical for ownership checking and code generation.

## 33.3 MIR references — LOCKED

MIR should be able to represent explicit shared/mutable references/borrows internally even though Zore source does not use raw reference syntax.

## 33.4 MIR control flow — LOCKED

MIR should support terminators equivalent to:

- goto
- branch
- call
- return
- await/suspend
- spawn/task creation
- unreachable

The exact enum/API names are implementation details.

---

# 34. Drop Insertion

## 34.1 Separation from ownership checking — LOCKED DIRECTION

Ownership analysis answers:

> Is this value still alive and legally usable?

Drop analysis answers:

> Where must destruction occur?

These should be conceptually separate compiler responsibilities.

## 34.2 Explicit MIR drop — LOCKED DIRECTION

Final MIR should be capable of containing explicit drop operations.

This simplifies:

- deterministic cleanup
- early returns
- error propagation
- branch cleanup
- async state cleanup
- code generation

---

# 35. Async Lowering

## 35.1 State-machine model — LOCKED DIRECTION

Async functions lower to compiler-generated state machines.

A local needed after suspension becomes part of persistent async state.

Example:

```ore
async func process(file own File) error {
    let data = await read(file)?
    await upload(data)?
    return nil
}
```

Conceptually:

```text
State 0:
    file
    await read(file)

State 1:
    file
    data
    await upload(data)

State 2:
    cleanup
    return
```

This example is conceptual. The exact generated states are implementation details.

A waiting operation inside an async function (§17.3) is a suspension point, so
values still needed after it are part of the async state, whether or not it is
written with `await`.

## 35.2 Ownership of async state — LOCKED

The async state machine must correctly own or borrow every live value according to ordinary Zore ownership rules.

Suspension must not make otherwise-invalid borrowing legal.

---

# 36. Runtime Responsibilities

## 36.1 MVP runtime — LOCKED DIRECTION

The runtime must eventually provide the facilities required by locked language semantics, including:

- memory allocation for owned dynamic values
- destruction support
- tasks
- async scheduler
- channel synchronization
- async I/O integration
- panic behavior
- standard runtime primitives used by the standard library

## 36.2 Runtime implementation freedom

The exact scheduler algorithm, allocator, channel queue implementation, wake mechanism, and OS integration are implementation details unless later standardized.

These choices must still meet the runtime contracts locked elsewhere: the
progress guarantee for blocking waits (§18.9) and blocking `println` writes
(§37.1), per-task panic containment and
reporting (§18.10), immediate termination at exit (§18.11), wake-on-close and
buffered-value cleanup for channels (§19.11–19.13), and mutex poisoning
(§20.2).

---

# 37. Standard Library Requirements

The MVP language needs enough standard-library/runtime support to exercise its core semantics.

At minimum, the architecture must leave room for:

- strings
- dynamic arrays
- maps
- files
- sockets
- basic I/O
- tasks
- channels
- mutex/synchronization
- async I/O

Packages and imports are specified in §3.20. The standard packages are
specified in §37.2 (text, bytes, and errors), §37.3 (time, the operating
system, buffered I/O, and TCP), §37.4 (cancellation and task coordination), and
§37.5 (paths and sorting). Further packages are TBD.

## 37.1 `println` — LOCKED

`println` is a predeclared, compiler-known function (§3.17). It takes exactly
one argument and returns no results. It writes the argument's text followed by
a line feed (U+000A) to standard output.

The argument must have one of these types: `bool`; `int`, `int8`, `int16`,
`int32`, `int64`, `uint`, `uint8`, `uint16`, `uint32`, `uint64` (including the
aliases of §6.5); `float32`, `float64`; `rune`; or `string`. An untyped numeric
constant argument takes its default type (§6.5), so `println(42)` prints an
`int`. Other argument types, zero arguments, and more than one argument are
compile-time errors. There is no formatting directive, separator argument,
or implicit conversion.

| Argument type | Text written before the line feed |
| --- | --- |
| `string` | Its UTF-8 contents, unchanged and unquoted |
| `bool` | `true` or `false` |
| Integer types | Decimal digits of the value, with a leading `-` for negative values and no separators, prefixes, or leading zeros |
| `rune` | The UTF-8 encoding of the scalar value, unquoted |
| `float32`, `float64` | TBD: must be locked before float printing is implemented natively |

```ore
println("Hello, Zore!")   // Hello, Zore!
println(42)               // 42
println(int8(-5))         // -5
println(true)             // true
println('é')              // é
println(user.Name)        // the string field's contents
```

## 37.2 Text, byte, and error packages — LOCKED

The compiler ships the standard packages of §37.2–§37.5, imported by their
paths (§3.20): `"zore/strings"`, `"zore/strconv"`, `"zore/unicode"`,
`"zore/unicode/utf8"`, `"zore/bytes"`, and `"zore/errors"` here. They are
ordinary packages as far as users can tell: names are used with the qualifier
(the last segment of the path) and every function, type, method, and constant
listed is exported. A package whose path has several segments, such as
`"zore/unicode/utf8"`, is used as `utf8.Name`; it is a separate package from
`"zore/unicode"` and is imported on its own.

The packages follow one set of conventions:

- Exported functions start with an upper-case letter and use the same name for
  the same job across packages (`Index`, `HasPrefix`, `Count`, `Equal`).
- A function that can fail returns `error` last (§7.2). An error message begins
  with the qualified function name, such as `strconv.Atoi: `, except the fixed
  message `EOF`, which every reading function uses for the end of its input.
- A function that parses text names the text it rejected, quoted by
  `strconv.Quote`: `strconv.Atoi: parsing "x": invalid syntax`.
- Durations and instants are `int` nanoseconds (§37.3).
- A type whose fields are unexported is made with a constructor, `NewName`,
  since a struct literal needs every field (§8.4).

Their functions never mutate their arguments except a `mut` parameter; `string`
results may share storage with their arguments (§6.8, §41.5).

**`zore/strings`**

| Function | Behavior |
| --- | --- |
| `Contains(s string, sub string) bool` | Whether `sub` occurs in `s`; an empty `sub` is always found |
| `ContainsRune(s string, r rune) bool` | Whether `r` occurs in `s` |
| `HasPrefix(s string, prefix string) bool` | Whether `s` starts with `prefix` |
| `HasSuffix(s string, suffix string) bool` | Whether `s` ends with `suffix` |
| `Index(s string, sub string) int` | Byte index of the first occurrence of `sub`, or `-1`; an empty `sub` gives `0` |
| `LastIndex(s string, sub string) int` | Byte index of the last occurrence of `sub`, or `-1`; an empty `sub` gives `s.len()` |
| `IndexByte(s string, c byte) int` | Byte index of the first byte equal to `c`, or `-1` |
| `IndexRune(s string, r rune) int` | Byte index of the first occurrence of `r`, or `-1` |
| `Count(s string, sub string) int` | The number of non-overlapping occurrences of `sub`; an empty `sub` gives the character count plus one |
| `EqualFold(s string, t string) bool` | Whether the strings are equal character by character, where two characters also match when their Unicode lower-case mappings are equal |
| `ToUpper(s string) string` | `s` with Unicode default upper-case mapping applied |
| `ToLower(s string) string` | `s` with Unicode default lower-case mapping applied |
| `TrimSpace(s string) string` | `s` without leading and trailing Unicode white space |
| `Trim(s string, cutset string) string` | `s` without leading and trailing characters that occur in `cutset` |
| `TrimLeft(s string, cutset string) string` | `s` without leading characters that occur in `cutset` |
| `TrimRight(s string, cutset string) string` | `s` without trailing characters that occur in `cutset` |
| `TrimPrefix(s string, prefix string) string` | `s` without `prefix` when it starts with it, otherwise `s` |
| `TrimSuffix(s string, suffix string) string` | `s` without `suffix` when it ends with it, otherwise `s` |
| `Cut(s string, sep string) (string, string, bool)` | The text before and after the first `sep`, and `true`; without one, `s`, `""`, and `false` |
| `Repeat(s string, count int) string` | `count` copies of `s` joined; panics when `count` is negative or the result length overflows `int` |
| `Replace(s string, old string, new string, n int) string` | `s` with the first `n` non-overlapping occurrences of `old` replaced, or all of them when `n` is negative; an empty `old` matches before each character and at the end |
| `ReplaceAll(s string, old string, new string) string` | `Replace` with `n` of `-1` |
| `Split(s string, sep string) Array<string>` | The pieces of `s` between occurrences of `sep`; an empty `sep` splits into characters; an empty `s` gives one empty piece, except for an empty `sep`, which gives no pieces |
| `SplitN(s string, sep string, n int) Array<string>` | Like `Split`, but with a positive `n` at most `n` pieces, the last holding the rest of `s`; `n` of zero gives no pieces and a negative `n` is `Split` |
| `Fields(s string) Array<string>` | The pieces of `s` between runs of Unicode white space; no pieces when `s` holds only white space |
| `Join(parts []string, sep string) string` | The elements of `parts` joined with `sep` between them; no elements give `""` |
| `Bytes(s string) Array<byte>` | A new array holding the bytes of `s`; the empty string gives an empty array |
| `FromBytes(data []byte) (string, error)` | The string made of the bytes of `data`, copied; when they are not well-formed UTF-8, `""` and an error whose message is `strings.FromBytes: invalid UTF-8` (§41.5) |

`strings.Builder` collects text and joins it once:

| Function or method | Behavior |
| --- | --- |
| `NewBuilder() Builder` | An empty builder |
| `(b mut Builder) WriteString(s string)` | Appends `s` |
| `(b mut Builder) WriteRune(r rune)` | Appends the encoding of `r` |
| `(b Builder) Len() int` | The number of bytes written |
| `(b Builder) String() string` | Everything written, in order |
| `(b mut Builder) Reset()` | Empties the builder |

**`zore/strconv`**

| Function | Behavior |
| --- | --- |
| `Itoa(value int) string` | Decimal digits of `value`, with a leading `-` when negative |
| `Atoi(text string) (int, error)` | Parses an optional `+` or `-` followed by one or more decimal digits, with nothing else |
| `FormatInt(value int, base int) string` | The digits of `value` in `base` 2–36, with lower-case letters for digits above 9 and a leading `-` when negative; panics with `strconv.FormatInt: invalid base` for another base |
| `ParseInt(text string, base int, bitSize int) (int, error)` | Parses an optional sign and digits in `base` 2–36, either letter case; `base` 0 reads the prefixes and digit separators of an integer literal (§3.11–§3.12). The value must fit a signed integer of `bitSize` bits, 1–64, where 0 means 64 |
| `FormatBool(value bool) string` | `"true"` or `"false"` |
| `ParseBool(text string) (bool, error)` | `true` for `"true"`, `false` for `"false"` |
| `Quote(s string) string` | `s` as a double-quoted literal (§3.8): `"`, `\`, line feed, carriage return, and tab use their escapes; other characters below U+0020, U+007F, and U+0080–U+009F use `\uXXXX`; everything else is kept |
| `QuoteRune(r rune) string` | `r` as a rune literal (§3.10), escaped as `Quote` does, with `\'` for a single quote |
| `Unquote(s string) (string, error)` | The value of a double-quoted string, raw string, or rune literal, decoded as §3.8–§3.10 specify |

On failure the parsing functions return `0`, `false`, or `""` and an error whose
message is `strconv.Name: parsing Q: problem`, where `Q` is the input quoted by
`Quote` and `problem` is `invalid syntax`, `value out of range`, `invalid base
B`, or `invalid bit size B`.

Floating-point formatting and parsing are not included: the text format of
floats is still open (§37.1).

**`zore/unicode`**

| Name | Behavior |
| --- | --- |
| `const MaxRune`, `ReplacementChar`, `MaxASCII` | `'\U0010FFFF'`, `'�'`, and `'\u007F'` |
| `IsLetter(r rune) bool` | Whether `r` has the Unicode Alphabetic property |
| `IsDigit(r rune) bool` | Whether `r` is a decimal digit, general category Nd |
| `IsNumber(r rune) bool` | Whether `r` is in general category Nd, Nl, or No |
| `IsSpace(r rune) bool` | Whether `r` has the Unicode White_Space property |
| `IsUpper(r rune) bool`, `IsLower(r rune) bool` | Whether `r` has the Uppercase or Lowercase property |
| `IsControl(r rune) bool` | Whether `r` is in general category Cc |
| `ToUpper(r rune) rune`, `ToLower(r rune) rune` | The single-character case mapping of `r`, or `r` when the mapping is not one character |

**`zore/unicode/utf8`**

| Name | Behavior |
| --- | --- |
| `const RuneError`, `RuneSelf`, `MaxRune`, `UTFMax` | `'�'`, `0x80`, `'\U0010FFFF'`, and `4` |
| `RuneLen(r rune) int` | The number of bytes in the encoding of `r` |
| `EncodeRune(p mut []byte, r rune) int` | Writes the encoding of `r` at the start of `p` and returns its length; panics when `p` is too short |
| `RuneCountInString(s string) int` | The number of characters in `s` |
| `RuneCount(p []byte) int` | The number of characters in `p`, counting each byte of an invalid sequence as one |
| `Valid(p []byte) bool` | Whether `p` is well-formed UTF-8 |
| `RuneStart(b byte) bool` | Whether `b` can start an encoding, that is, is not a continuation byte |
| `FullRune(p []byte) bool` | Whether `p` begins with a whole encoding or with bytes that can never become one |
| `DecodeRune(p []byte) (rune, int)` | The first character of `p` and its length; `(RuneError, 1)` for an invalid sequence and `(RuneError, 0)` for an empty `p` |
| `DecodeLastRune(p []byte) (rune, int)` | The same for the last character of `p` |
| `DecodeRuneInString(s string) (rune, int)`, `DecodeLastRuneInString(s string) (rune, int)` | The same for a string, which is always well-formed |

**`zore/bytes`** works on `[]byte` the way `zore/strings` works on text:

| Function | Behavior |
| --- | --- |
| `Equal(a []byte, b []byte) bool` | Whether the two views hold the same bytes |
| `Compare(a []byte, b []byte) int` | `-1`, `0`, or `1` as `a` sorts before, equal to, or after `b` byte by byte |
| `HasPrefix`, `HasSuffix`, `Contains(s []byte, sub []byte) bool` | As in `zore/strings` |
| `Index`, `LastIndex(s []byte, sep []byte) int`, `IndexByte(s []byte, c byte) int` | As in `zore/strings`, in bytes |
| `Count(s []byte, sep []byte) int` | Non-overlapping occurrences of `sep`; an empty `sep` gives `s.len() + 1` |
| `Clone(s []byte) Array<byte>` | A new array holding the bytes of `s` |

`bytes.Buffer` is a growing byte queue: writes add at the end and reads take from
the front.

| Function or method | Behavior |
| --- | --- |
| `NewBuffer(data []byte) Buffer`, `NewBufferString(s string) Buffer` | A buffer holding a copy of `data` or the bytes of `s` |
| `(b mut Buffer) Write(p []byte) (int, error)`, `WriteString(s string) (int, error)`, `WriteRune(r rune) (int, error)`, `WriteByte(c byte) error` | Append and report the bytes added; the error is always `nil` |
| `(b mut Buffer) Read(p mut []byte) (int, error)` | Moves up to `p.len()` unread bytes into `p`; with nothing unread, `0` and `EOF` (`0` and `nil` for an empty `p`) |
| `(b mut Buffer) ReadByte() (byte, error)` | The next unread byte, or `0` and `EOF` |
| `(b Buffer) Len() int`, `(b Buffer) Bytes() Array<byte>` | The number of unread bytes, and a copy of them |
| `(b mut Buffer) Truncate(n int)`, `(b mut Buffer) Reset()` | Keeps the first `n` unread bytes, panicking when `n` is negative or above `Len()`; or empties the buffer |

**`zore/errors`**

| Function | Behavior |
| --- | --- |
| `New(text string) error` | `error(text)` (§15.1) |
| `Is(err error, target error) bool` | `err == target` |

Wrapping and cause chains remain open (Q05); `Is` compares messages, as `==`
does.

```ore
import "zore/strconv"
import "zore/strings"

func main() {
    let parts = strings.Split("a,b,c", ",")
    println(strings.Join(parts[:], "-"))     // a-b-c
    let key, value, found = strings.Cut("lang=zore", "=")
    if found { println(key + ": " + value) } // lang: zore
    let n, err = strconv.Atoi("42")
    if err == nil { println(strconv.FormatInt(n, 16)) }   // 2a
}
```

Ownership, error, and async implications: all functions here are synchronous.
`Split`, `SplitN`, `Fields`, `Bytes`, and `Clone` return owned arrays; a
`Builder` or `Buffer` owns its contents and is a Move value. Errors follow the
trailing-`error` rule (§7.2) and the error-use rules (§15). Argument validation
panics (`Repeat`, `FormatInt`, `EncodeRune`, `Truncate`) are ordinary runtime
panics.

Compiler impact: standard package sources are bundled with the compiler and
loaded like folders. Some function bodies are written in Zore and some are
provided by the runtime; a function declaration without a body is accepted only
in bundled sources and is rejected everywhere else. A runtime-provided function
takes strings, slices, integers, runes, and booleans. Pending conformance
cases: `tests/conformance/packages.md` and `tests/conformance/strings.md`.

## 37.3 Time, operating system, buffered I/O, and network packages — LOCKED

`"zore/time"`, `"zore/os"`, `"zore/os/exec"`, `"zore/io"`, `"zore/bufio"`, and
`"zore/net"` give tasks a way to wait for the clock, files, standard input and output, other
programs, and TCP connections without blocking other tasks. Functions are
ordinary calls: they are written without `await`, and may be called from
synchronous and `async` functions alike. In an `async func` body a call that
waits suspends only the calling task (§17.3); in a synchronous function it
blocks the calling thread. While one task waits, every other task keeps making
progress, in the same sense as the progress guarantee of §18.9. In the initial
task a wait blocks the entry point's thread but not the other tasks.

Files, standard input, and connections carry bytes. Turning bytes into text is
`strings.FromBytes` (§37.2), which checks UTF-8 (§6.8). An array argument is
passed to a `[]byte` parameter as a view, `data[:]`, and to a `mut []byte`
parameter the same way (§12.1–§12.2).

**`zore/time`** measures time in `int` nanoseconds.

| Name | Behavior |
| --- | --- |
| `const Nanosecond`, `Microsecond`, `Millisecond`, `Second`, `Minute`, `Hour` | `1`, `1000`, `1000000`, `1000000000`, `60 * Second`, and `60 * Minute` |
| `Sleep(d int)` | Suspends the calling task for at least `d` nanoseconds; zero or less returns at once |
| `Now() int` | A reading of a monotonic clock in nanoseconds; its starting point is unspecified, but every reading is positive and none is smaller than an earlier one |
| `Since(start int) int`, `Until(t int) int` | `Now() - start` and `t - Now()` |
| `After(d int) channel<bool>` | A channel that receives `true` once after at least `d` and is then closed; zero or less fires at once. The wait is a task that sleeps, so it is not a deadlock while it is pending. Use it as a `select` case to put a time limit on a channel operation |

A deadline is a `Now()` reading; zero means none.

```ore
time.Sleep(250 * time.Millisecond)
let started = time.Now()
println(time.Since(started) < time.Second)
```

**`zore/os`**

| Function | Behavior |
| --- | --- |
| `ReadFile(path string) (Array<byte>, error)` | The whole contents of the file |
| `WriteFile(path string, data []byte, perm int) error` | Creates or truncates the file and writes `data`; a new file gets the permission bits `perm`, such as `0o644`, on systems that have them |
| `ReadDir(path string) (Array<DirEntry>, error)` | The entries of a folder, sorted by name; `(e DirEntry) Name() string` and `(e DirEntry) IsDir() bool` describe each |
| `Stat(path string) (FileInfo, error)` | Facts about a file or folder, following links: `(i FileInfo) Name() string` (the last element of `path`), `Size() int` in bytes, `Mode() int` (the permission bits), and `IsDir() bool` |
| `MkdirAll(path string, perm int) error` | Creates the folder and any missing parents with the permission bits `perm`; nothing to do when it exists |
| `Remove(path string) error` | Removes a file or an empty folder |
| `RemoveAll(path string) error` | Removes a file or a folder and everything in it; a missing path is not an error |
| `Getenv(key string) string` | The value of the environment variable, or `""` when it is not set |
| `Getwd() (string, error)` | The current folder |
| `Args() Array<string>` | The program's command-line arguments, starting with the program name |
| `Exit(code int)` | Ends the process at once with the status `code`; other tasks are not waited for and nothing is dropped |

`os.File` is an open file. It is a struct type with a custom `drop` (§8.3), so it
is a Move value: dropping one closes it. Its field is not exported, and the zero
value is closed, so every operation on it fails.

| Function or method | Behavior |
| --- | --- |
| `Open(path string) (File, error)` | Opens a file for reading |
| `Create(path string) (File, error)` | Creates or truncates a file and opens it for reading and writing |
| `Stdin() File`, `Stdout() File`, `Stderr() File` | Standard input, output, and error; closing or dropping these does nothing |
| `(f File) Read(buf mut []byte) (int, error)` | Waits for at least one byte and moves up to `buf.len()` bytes into `buf`; at end of input, `0` and `EOF`; an empty `buf` gives `0` and `nil` at once |
| `(f File) Write(data []byte) (int, error)`, `(f File) WriteString(s string) (int, error)` | Writes everything and returns the byte count; on failure, the bytes written before it |
| `(f own File) Close() error` | Closes the file |

Error messages begin with the function name (`os.ReadFile: `, `os.Open: `,
`os.Read: `, `os.Write: `, `os.Stat: `, `os.ReadDir: `, and so on) and continue
with system-defined text.

**`zore/os/exec`** runs other programs.

| Name | Behavior |
| --- | --- |
| `type Cmd struct { Path string; Args Array<string>; Dir string }` | A program to run: `Path` is found through the `PATH` environment variable when it has no `/`; `Args` starts with the program name; a non-empty `Dir` is the folder it runs in |
| `Command(name string, args []string) Cmd` | A `Cmd` with `Path` set to `name` and `Args` to `name` followed by `args` |
| `(c Cmd) Run() error` | Runs the program and waits for it, with standard input, output, and error connected to nothing |
| `(c Cmd) Output() (Array<byte>, error)` | Runs it and returns what it wrote to standard output |
| `(c Cmd) CombinedOutput() (Array<byte>, error)` | Runs it and returns what it wrote to standard output and error, in the order written |

A program that ends with a nonzero status gives the error `exit status N`, one
ended by a signal gives `signal: N`, and one that cannot start gives an error
beginning `exec: `. `Output` and `CombinedOutput` return what was captured
together with the error.

**`zore/io`** names what files, connections, and buffers have in common, so
one function can work with any of them. Its interface types (§22.2) are:

| Interface | Entries |
| --- | --- |
| `Reader` | `mut Read(buf mut []byte) (int, error)` |
| `Writer` | `mut Write(data []byte) (int, error)` |
| `Closer` | `own Close() error` |
| `ReadWriter` | the entries of `Reader` and `Writer` |
| `ReadCloser` | the entries of `Reader` and `Closer` |
| `WriteCloser` | the entries of `Writer` and `Closer` |

`os.File`, `net.Conn`, `bytes.Buffer`, `bufio.Reader`, and `bufio.Writer`
satisfy the ones whose methods they have. A `Read` gives at least one byte, or
`0` and an error; the end of the input is the error `EOF`.

| Name | Behavior |
| --- | --- |
| `let EOF = error("EOF")` | The error a `Read` gives at the end of the input |
| `Copy(dst mut Writer, src mut Reader) (int, error)` | Writes everything `src` gives to `dst` until `EOF`, and returns the byte count; `EOF` itself is not an error, and the first other read or write error stops the copy |
| `ReadAll(r mut Reader) (Array<byte>, error)` | Every byte up to `EOF`, with `nil`; on another error, the bytes read before it and the error |
| `ReadFull(r mut Reader, buf mut []byte) (int, error)` | Reads until `buf` is full; `EOF` when nothing was read, `unexpected EOF` when the input ends part way |
| `WriteString(w mut Writer, s string) (int, error)` | Writes the bytes of `s` |

```ore
import "zore/io"
import "zore/os"

func save(dst mut io.Writer, path string) (int, error) {
    var src = os.Open(path)?
    return io.Copy(dst, src)
}
```

**`zore/bufio`** reads and writes any `io.Reader` or `io.Writer` in large
pieces. Each type owns the reader or writer it was made from (an `own`
parameter, so a file or connection moves into it), and that value is dropped,
closing a file or connection, when the reader, scanner, or writer is dropped.

| Function or method | Behavior |
| --- | --- |
| `NewReader(source own io.Reader) Reader` | A reader over `source` |
| `(r mut Reader) ReadBytes(delim byte) (Array<byte>, error)` | The bytes up to and including the next `delim`; at end of input, what is left and the error, `EOF` at the end |
| `(r mut Reader) ReadString(delim byte) (string, error)` | `ReadBytes` as text; bytes that are not UTF-8 give `""` and `bufio.Reader: invalid UTF-8` |
| `(r mut Reader) ReadByte() (byte, error)` | The next byte |
| `(r mut Reader) Read(buf mut []byte) (int, error)` | Up to `buf.len()` bytes, so a `Reader` is itself an `io.Reader` |
| `NewScanner(source own io.Reader) Scanner` | A scanner that reads `source` line by line |
| `(s mut Scanner) Scan() bool` | Reads the next line, without its terminator (`\n` or `\r\n`); a final line with no terminator counts. `false` at end of input or on an error, after which it stays `false` |
| `(s Scanner) Text() string` | The line the last `Scan` read |
| `(s Scanner) Err() error` | `nil` after end of input; otherwise the error that stopped the scanner, such as `bufio.Scanner: invalid UTF-8` for a line that is not text |
| `NewWriter(sink own io.Writer) Writer` | A writer that keeps up to 4096 bytes before writing them to `sink` |
| `(w mut Writer) Write(p []byte) (int, error)`, `WriteString(s string) (int, error)`, `WriteRune(r rune) (int, error)`, `WriteByte(c byte) error` | Add to the pending bytes, writing them when the limit is reached |
| `(w mut Writer) Flush() error`, `(w Writer) Buffered() int` | Write the pending bytes now; the number pending. Bytes still pending when a writer is dropped are lost |

```ore
import "zore/bufio"
import "zore/os"

func main() {
    var input = bufio.NewScanner(os.Stdin())
    for input.Scan() {
        println(input.Text())
    }
}
```

**`zore/net`** (TCP over IPv4 and IPv6)

`Listener` and `Conn` are struct types with a custom `drop` (§8.3), so they are
Move values: dropping one closes it, as does `Close`. Their fields are not
exported. The zero value of either is closed and every operation on it fails
with an error. The `network` argument is `"tcp"` for any address, `"tcp4"` for
IPv4 only, or `"tcp6"` for IPv6 only; anything else fails with `net.Listen:
unknown network N` or `net.Dial: unknown network N`.

| Function or method | Behavior |
| --- | --- |
| `Listen(network string, address string) (Listener, error)` | Listens on `host:port`; port `0` picks a free port |
| `Dial(network string, address string) (Conn, error)` | Connects to `host:port`, waiting for the connection |
| `DialTimeout(network string, address string, timeout int) (Conn, error)` | Like `Dial`, but gives up after `timeout` nanoseconds for each address tried; zero or less means no limit |
| `(l Listener) Accept() (Conn, error)` | Waits for and returns the next incoming connection |
| `(l Listener) Addr() string` | The address the listener is bound to, as `host:port` (`[host]:port` for IPv6), or `""` when closed |
| `(l Listener) SetDeadline(t int) error` | Makes `Accept` give up at the deadline `t` (§37.3, `zore/time`); zero removes it |
| `(c Conn) Read(buf mut []byte) (int, error)` | Waits until at least one byte is available and moves up to `buf.len()` bytes into `buf`; at end of stream, `0` and `EOF`; an empty `buf` gives `0` and `nil` at once |
| `(c Conn) Write(data []byte) (int, error)` | Waits until all of `data` is sent and returns its length; on failure, the bytes sent before it |
| `(c Conn) LocalAddr() string`, `(c Conn) RemoteAddr() string` | The two ends of the connection, or `""` when closed |
| `(c Conn) SetDeadline(t int) error` | Sets the read and the write deadline |
| `(c Conn) SetReadDeadline(t int) error`, `(c Conn) SetWriteDeadline(t int) error` | Makes `Read`, or `Write`, give up at the deadline `t`; zero removes it |
| `(c Conn) CloseWrite() error` | Ends the sending side so the peer reads `EOF`; reading continues to work |
| `(l own Listener) Close() error`, `(c own Conn) Close() error` | Closes the listener or connection |

Setting a deadline fails only for a closed handle. A wait that reaches its
deadline fails with `net.Accept: timed out`, `net.Read: timed out`, or
`net.Write: timed out`, and so does any later wait until the deadline is moved;
a deadline in the past makes the next wait fail at once. The connection stays
usable: a timed-out read loses nothing, and a timed-out write reports how much
it sent. A deadline applies to waits that begin after it is set.

Error messages begin `net.Listen: `, `net.Dial: `, `net.Accept: `, `net.Read: `,
`net.Write: `, `net.SetDeadline: `, `net.CloseWrite: `, or `net.Close: ` and
continue with system-defined text, except the fixed messages above.

```ore
import "zore/net"

func serve(conn own net.Conn) {
    var buf = Array<byte>{0, 0, 0, 0, 0, 0, 0, 0}
    for {
        let count, err = conn.Read(buf[:])
        if err != nil { return }
        let _, writeErr = conn.Write(buf[:count])
        if writeErr != nil { return }
    }
}

func main() {
    let listener, err = net.Listen("tcp", "127.0.0.1:0")
    if err != nil { return }
    println(listener.Addr())
    let conn, _ = listener.Accept()
    let done = go serve(conn)
    done.wait()
}
```

Ownership, error, and async implications: a file, reader, scanner, writer, or
connection is owned by one task at a time and moves to another with `own`
(§18.4); no two tasks use it at once, so reads and writes need no locking by the
program. Errors follow §15; a handle returned with a non-nil error is the zero
value. Dropping a handle while another task still waits on it cannot happen,
since waiting borrows it. `Close` takes its receiver with `own`, so a closed
handle cannot be used again.

Compiler impact: the packages are bundled sources whose function bodies call the
runtime (§37.2). The runtime suspends the waiting task, or blocks the waiting
thread in a synchronous function, and wakes it from a timer, a readiness event,
or a helper thread (§36.2); the mechanism is not specified. A bundled function
that calls a waiting function waits too, so it is lowered like any other
waiting call. Pending conformance cases: `tests/conformance/io.md`.

## 37.4 Standard packages `zore/context` and `zore/sync` — LOCKED

`"zore/context"` gives tasks a way to ask each other to stop. Cancellation is
cooperative: nothing is interrupted, and a task stops when it looks at its
context. A `Context` is a Copy value; every copy refers to the same context.

| Function or method | Behavior |
| --- | --- |
| `Background() Context`, `TODO() Context` | A context that is never cancelled and has no deadline |
| `WithCancel(parent Context) (Context, func())` | A child of `parent` and a function that cancels it; calling the function again does nothing |
| `WithDeadline(parent Context, deadline int) (Context, func())` | A child that is also cancelled at the deadline (a `time.Now` reading), or at the parent's deadline when that is earlier |
| `WithTimeout(parent Context, timeout int) (Context, func())` | `WithDeadline(parent, time.Now() + timeout)` |
| `(c Context) Done() channel<bool>` | A channel that is closed when the context is cancelled, for use as a `select` case |
| `(c Context) Err() error` | `nil` while active; `context canceled` after its cancel function ran or its parent was cancelled that way; `context deadline exceeded` after its deadline passed |
| `(c Context) Deadline() (int, bool)` | The deadline and `true`, or `0` and `false` when there is none |

Cancelling a context cancels every context made from it, with the same error;
cancelling a child does not affect its parent. A cancelled context stays
cancelled, and its error does not change. Call the cancel function when the
work is done, so the waiting described below can end early.

```ore
import "zore/context"
import "zore/time"

func worker(ctx context.Context, results channel<int>) {
    var steps = 0
    for {
        select {
            case ctx.Done().receive() {
                results.send(steps)
                return
            }
            case time.After(10 * time.Millisecond).receive() {
                steps += 1
            }
        }
    }
}
```

A task blocked in an operation with no deadline does not notice cancellation;
give it a deadline (§37.3) or wait on `Done()` in a `select`. Cancelling has no
effect on tasks that never look at the context, and a task is never stopped or
dropped because of one.

`"zore/sync"` coordinates tasks beyond what channels and `Mutex<T>` (§20.2) do
directly. Both types are Copy handles that share their state.

| Function or method | Behavior |
| --- | --- |
| `NewWaitGroup() WaitGroup` | A counter at zero |
| `(wg WaitGroup) Add(delta int)` | Adds `delta`; panics with `sync: negative WaitGroup counter` when the counter goes below zero |
| `(wg WaitGroup) Done()` | `Add(-1)` |
| `(wg WaitGroup) Wait()` | Waits until the counter is zero |
| `NewOnce() Once` | A `Once` that has not run |
| `(o Once) Do(f func())` | Calls `f` the first time `Do` is called on any copy; other calls wait until that call returns and then do nothing |

Ownership, error, and async implications: a context or wait group holds
channel and mutex handles, so copies are free and safe to pass to any task.
Each `WithCancel`, `WithDeadline`, and `WithTimeout` on a cancellable parent
starts a small task that waits for the parent or the child, and each deadline
starts one that waits for the clock or the child. `Wait` and `Do` suspend like
the channel and mutex operations they use.

Compiler impact: both packages are bundled Zore source built from channels,
`Mutex<T>`, `select`, and `time.After`. Pending conformance cases:
`tests/conformance/io.md`.

## 37.5 Standard packages `zore/path`, `zore/path/filepath`, and `zore/sort` — LOCKED

`"zore/path"` works on slash-separated paths as text, without looking at any
file. `"zore/path/filepath"` offers the same functions for paths of the host
system, which use `/` on every supported system, plus `Abs`.

| Function | Behavior |
| --- | --- |
| `Clean(p string) string` | The shortest equivalent path: repeated slashes become one, `.` elements are removed, an `..` element removes the element before it, `..` at the start of a rooted path is removed, and a trailing slash is dropped; the empty result is `"."` |
| `Join(elems []string) string` | The non-empty elements joined with `/` and cleaned; `""` when every element is empty |
| `Split(p string) (string, string)` | The text up to and including the last `/`, and the rest |
| `Base(p string) string` | The last element, ignoring trailing slashes; `"."` for `""` and `"/"` for a path of only slashes |
| `Dir(p string) string` | Everything but the last element, cleaned |
| `Ext(p string) string` | The text from the last `.` in the last element, or `""` |
| `IsAbs(p string) bool` | Whether `p` starts with `/` |
| `filepath.Abs(p string) (string, error)` | `p` cleaned when absolute, otherwise joined to the current folder |
| `const filepath.Separator`, `filepath.ListSeparator` | `'/'` and `':'` |

`"zore/sort"` sorts and searches in place.

| Function | Behavior |
| --- | --- |
| `Ints(x mut []int)`, `Strings(x mut []string)` | Sort ascending, strings by bytes (§6.6); equal elements may change order |
| `IntsAreSorted(x []int) bool`, `StringsAreSorted(x []string) bool` | Whether `x` is ascending |
| `SearchInts(a []int, x int) int`, `SearchStrings(a []string, x string) int` | The first index whose element is not less than `x` in sorted `a`, or `a.len()` |

Ownership, error, and async implications: all functions are synchronous;
`sort` writes only through its `mut` parameter, and `Abs` reports an error only
when the current folder cannot be read.

Compiler impact: the packages are bundled Zore source. Pending conformance
cases: `tests/conformance/packages.md`.

---

# 38. Self-Hosting Constraint

## 38.1 Long-term goal — LOCKED

The Zore compiler should eventually be writable in Zore.

Initial implementation:

```text
Zore source
    ↓
Rust bootstrap compiler
    ↓
LLVM
    ↓
native executable
```

Later:

```text
Zore compiler source (.ore)
    ↓
bootstrap compiler
    ↓
native Zore compiler
```

Eventually:

```text
Zore compiler
written in Zore
    ↓
compiles itself
```

## 38.2 Design constraint — LOCKED DIRECTION

Compiler subsystems should be designed around language-neutral concepts that can later be implemented in Zore itself.

Examples:

- SourceManager
- Token
- AST
- Symbol
- Type
- HIR
- Place
- Borrow
- Region
- MIR
- BasicBlock
- Diagnostic

Avoid unnecessarily coupling the conceptual compiler architecture to Rust-only mechanisms.

## 38.3 Required ecosystem capabilities for self-hosting — LOCKED DIRECTION

The Zore standard library/runtime must eventually be sufficient to implement compiler workloads, including:

- filesystem access
- strings
- dynamic arrays
- maps
- memory allocation
- process execution where needed
- error handling
- compiler data structures
- file I/O

Self-hosting does not need to be achieved in the first compiler milestone.

---

# 39. MVP Feature Set

The following features are part of the locked MVP.

## 39.1 Language

- `.ore` files
- packages
- imports
- uppercase export / lowercase package-private visibility
- `let`
- `var`
- `const`
- functions
- multiple return values
- structs
- methods
- primitive types
- fixed arrays
- borrowed slices
- `Array<T>`
- maps
- strings
- closures

## 39.2 Ownership

- Copy semantics
- Move semantics
- automatic Copy/Move classification
- borrow by default
- `mut` mutable borrowing
- `own` ownership transfer
- borrow checking
- inferred lifetimes
- deterministic destruction
- automatic drop insertion
- explicit `drop`
- explicit `clone`

## 39.3 Errors

- `error`
- multiple-return error style
- `?`
- `panic`
- no hidden ordinary exceptions

## 39.4 Concurrency / async

- `async`
- `await`
- `go`
- `Task`
- `task.wait()`
- async state-machine lowering
- runtime scheduler
- channels
- buffered channels
- channel close
- Copyable channel handles
- ownership transfer through channels
- Copy semantics through channels
- closure capture rules
- borrow checking across tasks
- borrow checking across `await`
- async I/O support

## 39.5 Compiler

- lexer
- parser
- AST
- name resolution
- type checker
- HIR
- ownership checker
- borrow checker
- inferred regions/lifetimes
- MIR
- control-flow graph
- async lowering
- drop insertion
- LLVM backend
- native executable
- high-quality diagnostics

---

# 40. Explicitly Out of MVP

The following features must not be treated as part of the MVP unless this specification is changed:

- generic types (generic functions are in §22.1)
- macros
- reflection
- raw pointers
- pointer arithmetic
- `unsafe`
- FFI
- pattern matching
- advanced type inference
- const generics
- decorators / annotations
- external package registry
- advanced dependency solver
- JIT
- garbage collector
- advanced optimizer
- cross-compilation
- IDE language server
- explicit source-level lifetime syntax

Some of these may be introduced later.

---

# 41. Syntax Not Yet Fully Locked

The following areas remain intentionally incomplete.

Coding agents must not silently choose permanent semantics for them.

## 41.1 Full expression grammar — PARTIALLY LOCKED / TBD DETAILS

Operator inventory, precedence, associativity, short-circuit logic, and the
`await operation()?` grouping rule are locked in §7.6. Operand/call evaluation
is left to right (§7.5). Closure literals and function types are locked in §16.
Remaining work includes the complete primary/postfix grammar for strings,
iteration, and full task expression grammar.
Array literals, array/slice indexing, and slicing are locked in §12.6. Map
construction, two-result lookup, assignment, and removal are locked in §13.3;
borrowed entry access and iteration remain separate Q02/Q05 API decisions.
Struct construction is specified in §8.4, assignments in §5.6, and calls/result
forwarding in §7.8. Numeric and comparison type rules are locked in §6.5–6.6.

## 41.2 Loop grammar — LOCKED

Infinite, conditional, counting, and collection loops and unlabelled
`break`/`continue` are specified in §5.10. Do not infer additional forms from another language.

## 41.3 Conditional grammar — LOCKED

`if`, `else if`, and `else` statement forms are specified in §5.9. Block scopes
are defined in §5.8; return syntax and completion requirements in §7.7.

## 41.4 Zero values and `nil` — LOCKED

Zero values are not a way to skip explicit initialization. Every `let`/`var`
binding requires an initializer (§5.4) and every struct literal names every
field exactly once (§8.4); there is no field-omission or uninitialized-binding
syntax. A zero value is instead the value produced by specific built-in runtime
operations that must yield a result for a type without the programmer supplying
one: draining a closed channel (§19.8), filling non-error result positions
on propagation through `?` (§15.2), and missing map lookup/removal (§13.3). This section defines what that produced value is
for every type, and which types additionally admit `nil` as a distinct "absent"
value.

The zero value per type:

| Type | Zero value |
| --- | --- |
| `bool` | `false` |
| `int`, `int8/16/32/64`, `uint`, `uint8/16/32/64`, `byte` | `0` |
| `float32`, `float64` | positive zero (`+0.0`) |
| `rune` | `U+0000` |
| `string` | `""` (valid, length zero) |
| struct | each field set to that field's own zero value, recursively |
| named type (§8.5) | the zero value of its base type |
| `[T; N]` | `N` elements, each the zero value of `T` |
| `[]T`, `mut []T` (slice) | an empty, valid view of length zero, with no backing-storage loan |
| `Array<T>` | an empty, valid, owned array with zero elements |
| `map[K]V` | an empty, valid, owned map with zero entries |
| `error` | `nil` |
| `Task<...>` | `nil` |
| `channel<T>` | an always-closed, empty channel (§19.12) |
| `Mutex<T>` | a mutex with no lock and no value: `withLock` panics and `isPoisoned` is `false` (§20.2) |
| interface type (§22.2) | an empty value that holds nothing: destroying it does nothing, and calling a method through it panics |

`nil` is a literal denoting the absent state of exactly two built-in types:
`error` (no error) and `Task<...>` (no associated work). No other type —
including `bool`, numeric types, `rune`, `string`, struct types, `[T; N]`,
`[]T`, `Array<T>`, `map[K]V`, `channel<T>`, `Mutex<T>`, and interface types — admits `nil` as a value or
literal target. Those types are always valid to use once produced; there is no
separate "nil" state distinct from "empty" for slices, `Array<T>`, or `map[K]V`,
and a channel value is always a real channel whose zero value is already
closed. `==`/`!=` against `nil` is permitted only for `error` and `Task<...>`
values; comparing `nil` against any other type is a compile-time error. `error`
additionally supports `==`/`!=` against another non-nil `error` value (§15.1,
§6.6); `Task<...>` is comparable only against `nil`.

`Task<...>` keeps `nil` rather than a "completed" zero value: a zero task that
yielded zero-valued results would look like success, including a `nil` error,
so retrieving from a `nil` task panics instead (§18.9). A zero-value channel
needs no such distinction, because its receive already reports `ok == false`.

**Resource zero-state contract — LOCKED.** Every resource-owning type,
including one with custom `drop`, must treat its recursive zero value as a
valid empty state that owns no acquired resource. Destruction of that state
must complete harmlessly: it must not release an unacquired resource, panic,
block waiting for work, or perform acquisition-dependent side effects. The
compiler still invokes custom `drop` and automatic field cleanup normally;
it does not suppress the destructor based on how a value was constructed.
Nested resource fields must satisfy the same contract.

Resource authors must represent acquisition explicitly when a raw handle's
numeric zero is not an unused sentinel. For example, the following illustrates
the contract; `releaseReservation` stands for a resource API whose declaration
belongs to its defining library, not a new predeclared function:

```ore
type Reservation struct {
    acquired bool
    id int
}

func (r mut Reservation) drop() {
    if r.acquired {
        releaseReservation(r.id)
        r.acquired = false
    }
}
```

The all-zero `Reservation` has `acquired == false` and releases nothing.
A successfully acquired resource may have `id == 0`, with `acquired == true`;
that value is distinct from the empty state. Failed acquisition must not mark
an unacquired resource as owned. Do not add a second manual cleanup path for a
field whose own destructor already releases it.

A valid empty state need not support every resource operation successfully:
operations requiring acquisition must report an explicit error or panic under
their documented contract. Empty-state destruction itself remains harmless.
The same rule applies to zero resources produced by closed-channel receive,
`?`, and missing-key lookup/removal; ignoring their accompanying status does
not fabricate an acquired resource. Field privacy does not prevent these
built-in operations from constructing the zero state.

This is a resource API obligation, not a claim that the compiler can prove
arbitrary destructor bodies harmless. Violating it does not waive memory
safety or authorize undefined behavior: ordinary checked operations and panic
rules still apply. Standard-library resource types must validate the contract
with runtime tests; user-defined resource types need equivalent tests.
Compiler ownership checking alone does not prove correct external-resource
bookkeeping, just as it does not prove that a custom clone acquires a resource.

This locks the absent/zero-value shape for `error`, `Task<...>`, and
`channel<T>`. `error`'s full representation, construction, and comparison
semantics are locked in §15.1.

```ore
let ch = channel<User>()
ch.close()
let user, ok = ch.receive()
// user is User{} field-wise (Name == ""), ok == false

var task Task = nil     // no work associated yet
task = go process(user)

var err error = nil     // no error
```

Ownership, error, and async implications: a zero-valued struct or array is
constructed field-by-field/element-by-element using each element's own zero
value, without invoking user-defined construction logic; Copy/Move
classification (§8.3, §10) is unaffected. A `nil` `Task<...>` and a zero-value
channel hold no buffered values or running work and require no cleanup; a
`nil` `error` represents no error condition. This does not change `?` propagation (§15.2) or the
error-result-use rule (§15.6): a `nil` error is still a value that must be
explicitly discarded or otherwise used, not a special case that is exempt from
that rule.

Compiler impact: implement zero-value construction recursively over field/element
types for the operations that need it (channel drain, `?` result filling, and
map lookup/removal misses under §13.3). Keep normal drop obligations
for zero-produced resource values and represent zero slices without a loan;
do not infer resource acquisition from zero-valued fields. Restrict `nil` as a
literal and as an equality operand to `error` and `Task<...>` at the type-checking stage; reject it
elsewhere, including for channels, with a clear diagnostic naming the offending
type. Pending conformance cases: `tests/conformance/zero-values.md`.

## 41.5 String value and encoding — LOCKED

A `string` value is, at all times, a sequence of bytes that forms well-formed
UTF-8: it decodes to a sequence of Unicode scalar values in U+0000–U+10FFFF
excluding surrogates (U+D800–U+DFFF). This holds for every `string` value that
exists while a program runs, not only for literals. No currently locked
operation can produce an invalid string: double-quoted and raw string literals
decode from UTF-8 source text with scalar-validated escapes (§3.8–3.9);
string `+` concatenation (§7.6) joins two already-valid UTF-8 byte sequences,
which is itself always valid UTF-8; and the zero value `""` (§41.4) is
trivially valid. `.ore` source files are UTF-8 text (§3.1), which is what
literal decoding already assumed.

This guarantee constrains future API design rather than introducing new syntax
here: any later operation that constructs a `string` from arbitrary bytes
(for example, a future `[]byte`-to-`string` conversion in the predeclared API,
Q05) must validate its input and reject invalid UTF-8 — a compile-time error
for a constant, a runtime error or panic otherwise — rather than silently
accepting or repairing invalid bytes, consistent with how numeric conversions
are checked rather than lossy (§6.6). The conversion API is `strings.Bytes` and
`strings.FromBytes` (§37.2, Q30); this section only constrains it.

`string` values are immutable. No operation modifies a string's bytes in
place; an operation that appears to change a string's contents produces a new
string value. Immutability is why the UTF-8 guarantee only needs checking at
construction time, never re-verified afterward, and why an implementation may
safely share an underlying byte buffer across logical copies without a copy
ever observing another copy's mutation.

Indexing, slicing, byte length, and iteration over a string's contents are
specified in §6.8, and the first conversion API in §6.8 (`string(rune)`) and
§37.2. Any future `string`/`[]byte` conversion API remains a Q05 decision,
constrained by this section's validity guarantee but not designed by it.

Ownership, error, and async implications: `string` remains Copy from the
programmer's perspective (§10.2); the encoding guarantee does not change
Copy/Move classification, cleanup, error propagation, or async ownership.
Comparison ordering by UTF-8 encoding bytes (§6.6) is consistent with this
guarantee: because every string is valid UTF-8, byte-order comparison agrees
with Unicode scalar-value order.

Compiler impact: validate UTF-8 wherever a `string` value is constructed from
raw bytes once such an API exists; no validation is needed for literal decoding
or concatenation, since both are constructive from already-valid inputs.
`string`'s internal buffer representation (unique heap allocation, reference-
counted sharing, small-string optimization, etc.) and concatenation's
allocation strategy remain unstandardized implementation details — they may
change without a language-level specification revision, as long as the value
guarantees above hold. The bootstrap compiler counts the owners of each buffer
built at run time: every copy of a `string` that is kept (a variable, field,
element, map entry, closure capture, argument, or result) is one owner, and the
buffer is freed when the last owner goes, whether by leaving scope, being
overwritten, or a panic unwinding. A slice shares its source's buffer and counts
as an owner. Appending to the newest text in a buffer grows that buffer in
place, so building one text in a loop uses memory proportional to its final
length (Q23). Pending conformance cases are in
`tests/conformance/strings.md`.

## 41.6 Async lowering order — IMPLEMENTATION DETAIL

The exact compiler-pass sequence for:

- ownership analysis
- MIR construction
- async transformation
- drop insertion

remains open.

## 41.7 Runtime scheduler algorithm — IMPLEMENTATION DETAIL

No specific scheduler algorithm is locked.

## 41.8 `defer` — TBD / NOT MVP-REQUIRED

Do not depend on it.

## 41.9 Generic type syntax — OUT OF MVP

Generic functions are locked in §22.1. Do not infer user-defined generic types
from `Array<T>`, `channel<T>`, or generic functions.

---

# 42. First Semantic Compiler Target

The first meaningful Zore program should compile:

```ore
package main

type User struct {
    Name string
}

func greet(user User) {
    println(user.Name)
}

func main() {
    let user = User{
        Name: "John",
    }

    greet(user)
}
```

This program proves:

- package parsing
- functions
- structs
- field access
- string literal
- local binding
- function call
- default borrow semantics

---

# 43. Recommended Implementation Sequence

This sequence is a project plan, not a source-language semantic requirement.

```text
M0   Compiler executable / CLI skeleton
M1   Source manager + spans
M2   Lexer
M3   Parser
M4   AST
M5   Hello World
M6   Variables
M7   Functions
M8   Structs
M9   Name resolution
M10  Type checking
M11  HIR
M12  Basic MIR / CFG
M13  Copy semantics
M14  Move semantics
M15  Shared borrowing
M16  Mutable borrowing
M17  Lifetime / region analysis
M18  Drop insertion
M19  Error handling + ?
M20  Arrays / slices
M21  Array<T>
M22  Maps
M23  Packages / imports
M24  Closures
M25  Task model
M26  go / spawn
M27  async state-machine model
M28  await
M29  Scheduler/runtime
M30  Channels
M31  Async I/O
M32  LLVM backend hardening
M33  Native executable/toolchain hardening
M34  Standard library growth
M35+ Self-hosting compiler work
```

The exact milestone numbering may evolve.

Important implementation rule:

> Build synchronous ownership correctly before adding full concurrency, but design MIR and ownership data structures from the beginning so async/state-machine storage can be represented later.

---

# 44. Initial Compiler Repository Direction

A simple starting repository may be:

```text
zore/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── source.rs
│   ├── span.rs
│   ├── token.rs
│   ├── lexer.rs
│   ├── ast.rs
│   ├── parser.rs
│   └── diagnostic.rs
├── tests/
│   ├── lexer/
│   └── parser/
└── examples/
    └── hello/
        ├── zore.toml
        └── main.ore
```

Do not prematurely split the bootstrap compiler into many Rust crates.

Begin with a coherent single compiler crate and extract crates/modules when boundaries become stable.

A future larger structure may separate:

```text
zore-cli
zore-lexer
zore-parser
zore-ast
zore-resolve
zore-typeck
zore-hir
zore-ownership
zore-mir
zore-async
zore-codegen
zore-runtime
```

This is organizational guidance, not language semantics.

---

# 45. CLI Direction

The intended CLI eventually includes:

```bash
zore check .
zore build .
zore run .
zore fmt .
zore test .
```

The first implementation should prioritize:

```bash
zore check main.ore
```

before implementing full code generation. Today's commands take a `.ore` file;
the file's folder is the entry package and its project is found as specified in
§3.20. Passing a folder instead of a file is a later addition.

Semantic analysis and diagnostics should work independently of LLVM code generation.

---

# 46. Testing Requirements

Compiler development should include several layers.

## 46.1 Lexer tests

```text
source → expected token stream
```

## 46.2 Parser tests

```text
source → expected AST
```

## 46.3 Semantic tests

```text
source → expected diagnostics
```

Example:

```ore
let user = loadUser()
save(user)
println(user.Name)
```

Expected:

```text
use of moved value `user`
```

## 46.4 Ownership tests

Test:

- Copy after assignment
- move after assignment
- use-after-move
- shared borrow
- mutable borrow
- conflicting borrow
- move while borrowed
- explicit drop
- double drop
- field/partial move behavior
- borrow across branches
- borrow across async suspension
- values moved into tasks

## 46.5 Code-generation tests

```text
source → executable → expected output
```

Example:

```ore
func main() {
    println("Hello, Zore!")
}
```

Expected output:

```text
Hello, Zore!
```

## 46.6 Async/concurrency tests

Test:

- async suspension/resume
- local values surviving `await`
- resources dropped after async completion
- task result handling
- task error handling
- detached task semantics
- channel ownership transfer
- shared channel handles
- buffered channel blocking
- closed channel receive
- send on closed channel panic
- double close panic and wake-on-close for blocked senders/receivers
- zero-value (always-closed) channel behavior
- single retrieval of task results and `.wait()`/`await task` placement
- blocking waits not starving other tasks
- per-task panic containment, re-raise at retrieval, and reporting
- process exit abandoning running tasks
- buffered-value cleanup when the last channel handle is gone

---

# 47. Coding Agent Rules

This section is especially important when this specification is supplied to an AI coding agent.

## 47.1 Do not redesign locked decisions

Do not replace:

```ore
func read(user User)
func update(user mut User)
func save(user own User)
```

with Rust-like:

```text
&T
&mut T
```

or any other syntax.

Do not add move markers at call sites.

## 47.2 Do not add garbage collection

Resource management must remain ownership-based and deterministic.

Reference counting may be used internally for specific safe implementation strategies such as string/channel handles if appropriate, but this must not turn the language into a general tracing-GC model or change source-level ownership semantics.

## 47.3 Do not expose compiler lifetimes

Compiler regions/lifetimes are internal.

Do not add source syntax for them.

## 47.4 Do not invent missing language features

When implementation requires a decision not present here:

1. isolate the dependency,
2. add a TODO/spec question,
3. choose the most conservative temporary internal behavior if necessary,
4. do not silently create permanent language syntax.

## 47.5 Prefer compiler errors over unsafe inference

When ownership/lifetime validity cannot be proven, reject the program with a useful diagnostic.

Do not make unsafe code compile merely for convenience.

## 47.6 Keep passes separated

Avoid implementing:

- parsing
- name resolution
- type checking
- ownership analysis
- code generation

inside one monolithic traversal.

Preserve clear compiler stages.

## 47.7 Preserve source spans

Do not discard source location data during lowering.

Diagnostics are a first-class feature.

## 47.8 Use stable semantic IDs

After resolution, use IDs for compiler entities instead of performing repeated string matching.

## 47.9 Design for self-hosting

Prefer compiler architecture that could later be ported from Rust to Zore.

Do not make Zore's semantic model depend on Rust-specific ownership semantics.

Rust is the bootstrap implementation language, not the definition of Zore.

---

# 48. Canonical Ownership Examples

## 48.1 Shared borrow

```ore
func read(user User) {
    println(user.Name)
}

func main() {
    let user = loadUser()

    read(user)

    // valid: read() borrowed user
    println(user.Name)
}
```

## 48.2 Mutable borrow

```ore
func rename(user mut User, name string) {
    user.Name = name
}

func main() {
    var user = loadUser()

    rename(user, "Alice")

    println(user.Name)
}
```

## 48.3 Ownership transfer

```ore
func save(user own User) {
    persist(user)
}

func main() {
    let user = loadUser()

    save(user)

    // compile error:
    // user was moved into save()
    println(user.Name)
}
```

## 48.4 Move assignment

```ore
let connection = openConnection()
let other = connection

// compile error if Connection is Move
use(connection)

use(other)
```

## 48.5 Copy assignment

```ore
let a = 10
let b = a

println(a)
println(b)
```

Both values remain valid because `int` is Copy.

## 48.6 Explicit clone

```ore
let a = loadResource()
let b = clone(a)

use(a)
use(b)
```

Whether a type supports meaningful cloning is type/library specific; `clone` represents explicit independent duplication rather than ownership transfer.

---

# 49. Canonical Error Example

```ore
func loadConfig(path string) (Config, error) {
    let file = open(path)?
    let content = read(file)?
    return parseConfig(content)?
}
```

Required semantics:

1. `file` is owned locally.
2. If `read(file)` fails and `?` returns early, `file` must be cleaned up.
3. If parsing fails after `file` is no longer required, the compiler must still maintain correct deterministic cleanup.
4. No hidden exception machinery is required by the source model.

---

# 50. Canonical Async Example

```ore
async func upload(path string) error {
    let file = open(path)?
    let content = await readAsync(file)?
    await storage.upload(content)?
    return nil
}
```

Required semantics:

- `file` is subject to ordinary ownership/drop rules.
- values needed after an `await` must survive suspension safely.
- borrowed values may cross `await` only if validity is proven.
- any required cleanup occurs on both success and error paths.
- async does not disable or weaken ownership checking.

---

# 51. Canonical Channel Example

```ore
func worker(ch channel<User>) {
    for {
        let user, ok = ch.receive()

        if !ok {
            return
        }

        process(user)
    }
}

func main() {
    let ch = channel<User>(10)

    let w1 = go worker(ch)
    let w2 = go worker(ch)
    let w3 = go worker(ch)

    ch.send(User{
        Name: "A",
    })

    ch.send(User{
        Name: "B",
    })

    ch.close()

    w1.wait()
    w2.wait()
    w3.wait()
}
```

Required semantics:

- each worker receives a Copyable handle to the same channel
- channel state is shared safely by the runtime
- Move messages transfer ownership
- channel close prevents future sends
- buffered values remain receivable
- receive eventually returns `ok == false` after close and drain
- `main` waits for the workers; without the waits, the process could exit and
  abandon them before they process the buffered values (§18.11)

---

# 52. Final MVP Invariants

A conforming Zore MVP implementation must preserve these invariants:

1. **Borrow by default.**
2. **Mutation is explicit with `mut`.**
3. **Ownership transfer is explicit in the callee contract with `own`.**
4. **Call sites do not require move syntax.**
5. **Copy vs Move is type-driven.**
6. **Struct Copy/Move behavior is derived automatically.**
7. **Resource cleanup is deterministic.**
8. **Use-after-move and double-drop are compile-time errors in safe code.**
9. **Lifetimes are inferred and hidden from ordinary source code.**
10. **Errors are explicit values.**
11. **`?` propagates errors while preserving cleanup guarantees.**
12. **Async uses the same ownership model as synchronous code.**
13. **Tasks use the same ownership model as ordinary calls.**
14. **Channels safely transfer or copy messages according to ordinary ownership semantics.**
15. **Channel handles themselves are Copyable shared capabilities.**
16. **No tracing GC is required by the language model.**
17. **No user-facing raw pointers or `unsafe` are part of the MVP.**
18. **The compiler must prioritize safe rejection over unsafe guessing.**
19. **AST, HIR, and MIR remain conceptually distinct compiler layers.**
20. **The architecture should support eventual self-hosting.**

---

# 53. Specification Change Policy

When adding or changing a language feature:

1. update this specification first,
2. explicitly mark the decision as locked,
3. add syntax and semantic examples,
4. define ownership implications,
5. define error/async implications where relevant,
6. define compiler impact,
7. add conformance tests,
8. only then treat the decision as implementation-ready.

A coding agent must not treat experimental implementation behavior as language specification.

---

# 54. Summary

Zore's central programming model is:

```text
simple syntax
    +
borrow by default
    +
explicit mutation
    +
explicit ownership transfer
    +
automatic Copy/Move classification
    +
inferred lifetimes
    +
deterministic cleanup
    +
explicit errors
    +
async/tasks/channels using the same ownership model
    =
Simple code. Strong guarantees.
```

This document defines the currently locked Zore MVP. Anything not defined here should be treated as unspecified rather than inferred from Go, Rust, Java, C++, or any other language.
