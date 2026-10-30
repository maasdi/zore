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
| A record from `New` | Starts with `time=` and `time.UTC().RFC3339()` read around the call; calendar text itself is covered in `time.md` |
| `log.FormatTime(0)` | Rejected: the package no longer declares it (`packages/packages.rs`) |
| `WithLocalTime().Info(...)` under `UTC0`, `<+07>-7`, `<+0530>-5:30`, `<-0330>3:30`, and `EST5EDT,M3.2.0,M11.1.0`, next to a default logger | `time=` is the local wall time of the moment followed by its offset (`Z` for a zero offset, `+07:00`, `+05:30`, `-03:30`, `-04:00` or `-05:00`); the default record still ends in `Z` (`codegen/local_time.rs`) |

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

## `std/log/rolling`

Native cases are in `tests/codegen/rolling.rs`; checking cases are in
`tests/packages/packages.rs`; `examples/rolling-logs` rolls a small file.

| Scenario | Expected result |
| --- | --- |
| Ten 28-byte records with `MaxBytes` 100 and two backups | `app.log` holds record 9, `.1` records 6–8, `.2` records 3–5; there is no `.3` |
| `MaxBackups` zero, and a record longer than `MaxBytes` | The old file is removed; the long record gets a fresh file of its own |
| Three runs writing to one path, the third past the limit | The first two runs share a file; the third rolls it to `.1` |
| A 64-byte buffer with a one-hour interval | Nothing in the file until the buffer fills; `Flush` and `Close` write the rest |
| A 10 ms flush interval | The record reaches the file without `Flush` |
| A writer dropped at the end of a function | Its collected records are in the file |
| `main` panics while a record is only collected | The record is in the file after the panic |
| SIGINT through `signal.NotifyContext`, then `main` returns (Linux and macOS) | The file is empty before the signal and holds both records after exit |
| A logger writing after `Close` | `Err` is `rolling: closed` |
| Each invalid config field | `rolling.Open: ...` from `Open` and from the closed writer's `Write` |
| A roll whose oldest backup is a non-empty folder | `Err` starts `os.Remove: `; `Flush` and `Close` return the same error; the file keeps its records |
| Four plain and four async tasks through two loggers sharing one sink, 2,000-byte files | 400 whole, distinct records across more than five files; no file over 2,000 bytes |
| `Daily`, an existing file last changed on October 28, dated backups of October 20 and 25 (`.9`, `.10`), `MaxBytes` 60, four backups | The old file becomes `app.log.2026-10-28`; today's size rolls become `app.log.TODAY` and `.1`; October 20 and `.9` are pruned before `.10` |
| `Daily` with `app.log.notes`, numbered `app.log.1`, another file, and `.01` | Left alone; not dated backups |
| `Daily`, a restart on the same day under the size limit | The record is appended; nothing rolls |
| `DefaultConfig` | `Daily` and `Local` are `false` |
| `Daily` with `Local` under `TZ=EST5EDT,M3.2.0,M11.1.0`, an existing file last changed 2026-11-01 03:30 UTC (23:30 local the evening before) | It becomes `local.log.2026-10-31`; the same file without `Local` becomes `utc.log.2026-11-01` |
| `Open` given a string, `Close` then `Flush`, copying a `Writer` twice | Reject: mismatched types; use of moved value |
| `rolling.Sink{...}` | Reject: not exported |
