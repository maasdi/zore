# Statement-boundary conformance cases

Authority: spec §3.7. These are pending M2/M3–M4 lexer/parser tests, not executable
coverage. In this table, `\n` means a physical LF, `\r` means CR, and `<EOF>`
marks the end of input; these are test notation, not source syntax.

| Input / scenario | Expected result |
| --- | --- |
| `name\n`, `1\n`, `"text"\n`, `return\n`, `)\n`, `]\n`, `}\n`, `?\n` | Insert `;` after the last token; token snippets need not be complete programs |
| `break\n`, `continue\n`, `true\n`, `false\n`, `nil\n` | Insert `;` per §3.17; grammar/typing checked separately |
| Each eligible ending immediately followed by EOF | Insert `;` at EOF |
| `name\r\n` | Insert one `;` at LF |
| `name\r other` | CR alone does not insert `;` |
| `name;\n<EOF>` | No synthetic separator after the explicit one |
| `name\n\n<EOF>` | Exactly one inserted separator |
| Empty, whitespace-only, or comment-only file | No inserted separator |
| `let total = first +\nsecond` | No insertion after `+`; initializer continues |
| `let total = first\n+ second` | Insert after `first`; not one continued initializer |
| `read(file)?\nnext()` | Insert after `?` |
| `return\nvalue` | Insert after `return`; `value` is not its return operand |
| `await\noperation()` or `go\nwork()` | No insertion after `await`/`go`; expression legality is checked separately |
| `name // note\nother` | Insert at the line-comment newline |
| `name // note<EOF>` | Insert at EOF |
| `name /* note */ other` | No insertion inside the comment |
| `name /* first\nsecond */ other` | Insert at the first comment newline |
| `name /* first\nsecond\nthird */\n<EOF>` | Only one inserted separator |
| `greet(\nuser,\n)` | No separators after `(` or `,`; valid multiline call |
| `greet(\nuser\n)` | Insert after `user`; reject call with missing trailing comma |
| `func main() { greet(user) }` | Final statement separator may be omitted before `}` |
| `func main()\n{}` | Reject: inserted separator between signature and body |
| `let a = 1; let b = 2` | Explicit separator permits two statements on one line |
| Struct initializer with each field followed by a comma | No insertion after field commas |

Assert inserted-token source locations at the triggering LF or EOF, including
the first LF inside a multiline comment. When additional literal forms and
control-flow keywords are locked, extend this suite. Newlines inside a supported
multiline literal must not generate semicolon tokens within the literal.
