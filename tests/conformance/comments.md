# Comment conformance cases

Authority: spec §3.6. These are specified expectations awaiting executable lexer
tests at M2, not passing coverage. Statement termination is specified in §3.7
and covered in `statement-boundaries.md`.

| Input / scenario | Expected result |
| --- | --- |
| `// text` followed by line end | Skip comment; retain line boundary information |
| `// text` ending at EOF | Valid line comment |
| `//` at EOF | Valid empty line comment |
| `/**/` | Valid empty block comment |
| `/* first` then newline then `second */` | Valid multiline block comment; retain newline information |
| `/* outer /* inner */` | One complete block comment; no nesting |
| `/* outer /* inner */ tail` | `tail` is outside the comment |
| `/* // text */` | One block comment; `//` has no special effect |
| `// /* text` ending at EOF | One line comment; no unterminated block error |
| `/* " */` | Quote does not prevent block termination |
| `/* text` ending at EOF | Lexical error identifying the opening delimiter |
| `/*` ending at EOF | Lexical error identifying the opening delimiter |
| `user/* note */Name` | Two identifiers, never `userName` |
| `"https://example.test"`, `"/* text */"` | String contents, not comments |
| `// café 用户` and `/* café 用户 */` | Unicode comment contents accepted |

Verify source positions for tokens after comments, including multiline and
Unicode contents. Use `statement-boundaries.md` for line-ending and
statement-termination assertions.
