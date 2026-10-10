# Rune-literal conformance cases

Authority: spec §3.10, §3.9, §3.7, and §10.2. These are specified expectations
awaiting lexer/literal-decoding implementation, not executable or passing tests.

| Source / scenario | Expected result |
| --- | --- |
| `'A'`, `'é'`, `'😀'` | One scalar each; valid rune literals despite differing encoded lengths |
| `'\n'`, `'\r'`, `'\t'` | U+000A, U+000D, U+0009 respectively |
| `'\\'` and `'\u005C'` | One backslash; no recursive decoding |
| `'\''` | One single quote |
| `'"'` and `'\"'` | Same double-quote scalar |
| `'\u0041'`, `'\U00000041'`, `'A'` | Same rune value |
| `'\u00e9'` and `'\u00E9'` | Same scalar; either hex case accepted |
| `'\U0001F600'` and `'😀'` | Same rune value |
| `'\u0000'` | Valid NUL scalar, not empty |
| `'\uD7FF'`, `'\uE000'`, `'\U0010FFFF'` | Valid scalar boundaries |
| `''`, `'ab'`, `'\u0041B'` | Reject zero or multiple decoded scalars |
| `'e\u0301'` | Reject two scalars even if rendered as one accented character |
| `'\uD800'`, `'\uDFFF'`, `'\U0000D800'` | Reject surrogates |
| `'\uD83D\uDE00'` | Reject surrogate-pair escapes |
| `'\U00110000'`, `'\UFFFFFFFF'` | Reject values above U+10FFFF |
| `'\u123'`, `'\U0000041'`, `'\u12G4'`, `'\u{0041}'`, `'\u0_41'` | Reject malformed escapes |
| `'\q'`, `'\x41'`, `'\101'`, `'\0'`, `'\a'`, `'\b'`, `'\f'`, `'\v'` | Reject unsupported escapes |
| `'\\n'`, `'\u005Cn'` | Reject two decoded scalars; do not decode again |
| `'/'` and `'*'` | Ordinary rune values |
| Physical LF or CRLF inside single quotes | Reject multiline rune literal |
| Backslash followed by a physical newline | Reject continuation |
| EOF before closing quote | Error identifying opening quote |
| Newline/EOF after a complete rune literal | Insert semicolon per §3.7 |
| `'\n'` | One literal token; decoded LF does not insert an internal semicolon |

Assert source byte spans for Unicode and escaped contents and source-aware
diagnostics for malformed input. Once typing exists, test that rune values are
Copy and distinct from string values without presupposing an integer alias.

## Conversions (§6.6)

Covered by `tests/typecheck/check.rs` and `tests/codegen/native.rs`.

| Input / scenario | Expected result |
| --- | --- |
| `int('A')`, `uint8('é')`, `uint32('😀')` | `65`, `233`, `128512`; constants fold |
| `int(r)` for a runtime rune | Its scalar value |
| `int8(r)` for a rune above 127 at run time | Panics: integer conversion out of range |
| `uint8('ł')` | Reject: the constant does not fit |
| `rune(65)`, `rune(int32(955))` | `'A'`, `'λ'` |
| `rune(n)` for a runtime integer that is negative, above `0x10FFFF`, or in `0xD800`–`0xDFFF` | Panics: integer is not a valid rune |
| `rune(-1)`, `rune(0x110000)`, `rune(0xD800)` | Reject: not a Unicode scalar value |
| `rune(r)` of a rune | The same rune |
| `rune(1.5)`, `rune(f)` for a float, `float64('a')`, `rune(true)`, `rune("a")` | Reject: only integers and runes convert to `rune`, and a rune converts only to integers and `string` |
