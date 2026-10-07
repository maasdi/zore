# String-form conformance cases

Authority: spec §3.7–3.9, §10.2, and §41.5. These are pending lexer/literal-decoding
and runtime-value tests, not executable coverage. Examples below use source
spelling unless stated.

| Input / scenario | Expected result |
| --- | --- |
| `"Hello"` and `""` | Ordinary and empty double-quoted strings |
| `"café 用户"` | Unicode contents accepted |
| Physical LF or CRLF inside double quotes | Lexical error; no multiline quoted string |
| Backslash followed by a physical newline inside double quotes | Lexical error; no line continuation |
| EOF before closing double quote | Lexical error identifying opening delimiter |
| `"/* text */ // text"` | Comment delimiters remain contents |
| `"${name} {name}"` | No interpolation |
| Raw string containing backslashes | Preserve them literally; no escape decoding |
| Raw string containing physical newlines and indentation | Preserve contents; no trimming or dedenting |
| Raw string containing CRLF or standalone CR | Preserve CR and LF in the value |
| Raw string containing double quotes or comment delimiters | Preserve as contents |
| Raw string containing Unicode text | Accept |
| Raw string containing `${name}` | Literal text; no interpolation |
| Adjacent opening/closing backticks | Empty raw string |
| Backslash immediately before closing backtick | Backslash remains in value; backtick closes the literal |
| EOF before closing backtick | Lexical error identifying opening delimiter |
| Newlines within a raw string | One literal token; no internal semicolon insertion |
| Newline/EOF immediately after either closed string form | Insert semicolon according to §3.7 |

## Escape decoding cases

| Source / scenario | Expected result |
| --- | --- |
| `"\n\r\t"` | Three characters: U+000A, U+000D, U+0009 |
| `"\\"` | One backslash |
| `"\""` | One double quote; escaped quote does not close the literal |
| `"\u0041"`, `"\U00000041"`, `"A"` | Same value |
| `"\u00e9"`, `"\u00E9"`, `"é"` | Same value |
| `"\U0001F600"`, `"😀"` | Same value |
| `"\u0041B"` | Two characters, `AB`; consume exactly four digits |
| `"\U00000041B"` | Two characters, `AB`; consume exactly eight digits |
| `"\u0000"` | One NUL character, not an empty string |
| `"\uD7FF"`, `"\uE000"`, `"\U0010FFFF"` | Accepted scalar boundaries |
| `"\uD800"`, `"\uDFFF"`, `"\U0000D800"` | Reject surrogate values |
| `"\uD83D\uDE00"` | Reject surrogate pair escapes |
| `"\U00110000"`, `"\UFFFFFFFF"` | Reject values above U+10FFFF |
| `"\u123"`, `"\U0000041"` | Reject missing digits |
| `"\u12G4"`, `"\u{0041}"`, `"\u0_41"` | Reject malformed digits / unsupported forms |
| `"\q"`, `"\a"`, `"\b"`, `"\f"`, `"\v"`, `"\x41"`, `"\101"`, `"\0"`, `"\'"` | Reject unsupported escapes |
| `"'"` | Single quote accepted directly |
| `"\\n"`, `"\u005Cn"` | Backslash followed by `n`; no recursive decoding |
| Raw string containing `\q`, `\uD800`, or `\n` | Literal text; no escape validation or decoding |
| `"\n"` | One literal token; no semicolon inserted by decoded newline |

Verify malformed-escape diagnostic source spans against the original spelling,
not offsets in the decoded value.

Raw source examples:

~~~ore
let path = `C:\projects\zore`
let message = `first line
    indented second line`
let empty = ``
let backtick = "`"
~~~

Verify byte spans and following-token positions for multiline/Unicode contents.
At semantic milestones, verify both literal forms yield ordinary Copy strings.
These cases do not define adjacent-literal concatenation. Byte escapes are
excluded by §3.9.

## Value and encoding cases (§41.5)

| Scenario | Expected result |
| --- | --- |
| Any accepted double-quoted or raw string literal | Runtime value is well-formed UTF-8 |
| `"" ` (zero value, §41.4) | Well-formed UTF-8 (trivially, zero length) |
| `"café" + " 用户"` | Concatenation of two valid UTF-8 values is itself valid UTF-8, with no extra validation pass |
| Repeated concatenation building a longer string | Remains valid UTF-8 at every step |
| Two `string` values holding equal Unicode content, compared with `==` | Equal, regardless of any internal buffer sharing |
| `let b = a` where `a` is `string` | `b` is an independent Copy value; no operation lets mutating through one name affect the other, since strings are immutable |
| String ordering via `<`, `<=`, `>`, `>=` | Byte-wise UTF-8 order (§6.6), consistent with Unicode scalar-value order since every string is valid UTF-8 |

These cases test the language-level value guarantee only. They do not test
indexing, slicing, length, or iteration (open under Q02), nor any
bytes-to-string conversion API (open under Q05) — a future such API must
validate and reject invalid UTF-8 rather than accept it, but no conversion
syntax exists yet to test. Internal buffer representation and concatenation's
allocation strategy are implementation details, not conformance targets.

## String operations (§6.8)

Executable counterparts: `tests/typecheck/check.rs` and `tests/codegen/native.rs`;
`examples/strings` runs natively.

| Scenario | Expected result |
| --- | --- |
| `"héllo".len()` | `6`; length counts bytes |
| `s.len()` with arguments | Reject |
| `s[0]` | The first byte as a `byte`; prints as an integer |
| Index below zero or at or past the length | Runtime panic "index out of range" |
| `s[0] = 1`, `s[0] += 1`, or `s[0]` passed to a `mut` parameter | Reject; strings are immutable |
| `s[1:3]` on `"héllo"` | `"é"` |
| `s[:]`, `s[2:]`, `s[:2]`, `s[1:1]` | Whole, tail, head, empty string |
| Bound past the length, or `low` above `high` | Runtime panic "slice bounds out of range" |
| Bound inside a character (`"é"[1:]`) | Runtime panic "string slice not on a character boundary" |
| Constant negative or reversed bounds | Reject at compile time |
| `for ch in s` | Visits each character as a `rune` |
| `for i, ch in s` | `i` is the byte index where the character starts |
| Loop over an empty string | Zero iterations |
| Assigning the loop `ch` | Reject; loop names are not assignable |
| `a + b` where either is built at run time | Concatenation; operands unchanged |
| `s += t` on a `var` | Appends; the old value is unchanged for other copies |
| `string('é')` | `"é"`; a rune converts to its UTF-8 text |
| `string(65)`, `string(s)`, `string(byte)` | Reject; only a `rune` converts to `string` |
| Comparing built and literal strings with `==` and `<` | Compares bytes, not identity |
| Slice or concatenation result kept after its source variable goes out of scope | Valid; strings are Copy and never dangle, because each kept copy owns a share |
| Map keyed by a built string | Lookup by content finds a literal key |
| `text += piece` repeated 200,000 times | Completes using memory proportional to the final length |
| Appending to a text that an older string still shows | The older string is unchanged |
| Appending to a text that is not at the end of its buffer, such as a prefix slice | A new text is built; the source is unchanged |

| Text built in a loop and overwritten or dropped each round | Memory is returned when the last owner goes; usage stays bounded |
| Text kept in a struct, fixed array, `Array<string>`, map entry, or closure capture | Lives as long as that owner; copies of the owner keep it alive independently |
| Slice of a built text kept after the text's own variable is gone | Still valid; the slice is an owner of the shared buffer |
| `error` built from a built message, then ignored or propagated | The message is freed with the last copy of the error |
| Panic while built text is held by locals, arrays, or a custom `drop` value | Cleanup frees the text exactly once; the panic report is unchanged |
