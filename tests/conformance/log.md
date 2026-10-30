# Structured logging conformance cases

Authority: spec §37.12, §20.2, §22.2. Native cases are in
`tests/codegen/log.rs` and run with leak checking; checking cases are in
`tests/packages/packages.rs`; `examples/logging` and `examples/rest-api` use
the package end to end.

## Templates, records, and escaping

| Scenario | Expected result |
| --- | --- |
| `Info("ready")` on a logger at `LevelInfo` without time | `level=INFO msg=ready` and a line feed |
| `Info("listening on {addr} port {port}", "127.0.0.1", 8080)` | `msg="listening on 127.0.0.1 port 8080" addr=127.0.0.1 port=8080` |
| `Debug` on a logger at `LevelInfo` | Nothing is written, but its values are still evaluated; `Enabled(LevelDebug)` is `false` |
| `WithLevel(LevelDebug).Debug(...)` | The record is written at `DEBUG` |
| `With` pairs, then `With` again, then a template | Context fields in the order added, then one field per hole |
| The parent of a `With` copy | Its records do not gain the copy's fields |
| `int8`, `uint64`, `float32`, `rune`, a `String() string` type, a nil `error` | Their `fmt` `%v` text, `<nil>` for the error |
| Two calls with side effects as values | Each evaluated once, left to right |
| `{{` and `}}`, including around a hole | Literal braces in the message |
| A `With` key and a later hole with the same name | Both fields, in order |
| An empty message or value | `""` |
| Values with space, `=`, `"`, `\`, line feed, carriage return, tab, other control bytes, or UTF-8 | Quoted with `\"`, `\\`, `\n`, `\r`, `\t`, `\u00xx`, and UTF-8 unchanged, in both the message and the field |
| Plain punctuation such as `!#$~` | Bare |
| `Level(7)`, `Level(-1)` | `LEVEL(7)`, `LEVEL(-1)` |

## Timestamps

| Scenario | Expected result |
| --- | --- |
| `FormatTime(0)`, `FormatTime(-1)` | `1970-01-01T00:00:00Z`, `1969-12-31T23:59:59Z` |
| A leap day and a 2026 time | `2000-02-29T00:00:00Z`, `2026-10-29T11:23:49Z` |
| The first and last representable seconds | `0001-01-01T00:00:00Z`, `9999-12-31T23:59:59Z` |
| One second outside either end | The decimal Unix seconds |
| A record from `New` | Starts with `time=` and `FormatTime(time.UnixSeconds())` read around the call |

## Sharing, blocking, and cleanup

| Scenario | Expected result |
| --- | --- |
| Four plain and four async tasks log 25 records each through copies, with a sink that takes one byte per call and sleeps | 200 whole lines, each task's records in its own order |
| The last copy of a logger is dropped in a task | The sink's custom `drop` runs once, after that task ends |
| A logger over `os.Create` dropped at the end of a function | The file holds the record and can be read back |
| A sink's `Write` panics in a task | The next record from another copy panics with the poisoned-mutex message |

## Output failures

| Scenario | Expected result |
| --- | --- |
| A sink that accepts three bytes per call | The record is written whole; `Err` is `nil` |
| A sink that fails part way with its own error value | The partial bytes stay; `Err` is that value (`==` holds) on every copy |
| Records after a failure, at any enabled level | The sink is not called again |
| A disabled record after a failure | Not written, and not an error |
| `0` with `nil` | `log: no write progress`; later records do not call the sink |
| A count past the input or below zero with `nil` | `log: invalid write count` |
| A count below zero with the sink's own error | That error |
| The zero `Logger` | `Enabled` is `false`, records are dropped, `Err` is `log: no output` |

## Checking

| Scenario | Expected result |
| --- | --- |
| `With` pairs, templates with holes and `{{c}}`, `Log` with a `Level` and an untyped constant | Accepted |
| `New("text", LevelInfo)` | Reject: `string` does not satisfy `io.Writer` |
| An `int` variable as the `Log` level | Reject: mismatched types |
| More holes than values, or more values than holes | Reject: the counts are named |
| A template or `With` key that is not a constant string | Reject: must be a constant string |
| `{1x}`, an unclosed `{`, a lone `}` | Reject: not a hole name; not closed; must be written `}}` |
| An `Array<int>` value | Reject: cannot format a value of that type |
| No template, or an odd number of `With` arguments | Reject: needs a message template; takes keys and values in pairs |
| A `With` key with a space | Reject: not a log key |
| `logger.Info` as a value | Reject: a logging method cannot be used as a value |
| Using the result of `Info` | Reject: the call has no value |
| Calling the private `emit` helper | Reject: not exported |
| A hole or `With` key named `msg` or `level` | Reject: cannot use the key |
| The same hole twice, or the same key twice in one `With` | Reject: uses the key twice |
| A `log.Logger` converted to a user interface with `Info(template string)` | Reject: a checked logging call |
