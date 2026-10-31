# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Zore has no releases
yet; version numbers will follow [Semantic Versioning](https://semver.org/)
once it does.

## [Unreleased]

### Curated native libraries

- Added manifest-registered native adapters, generated private bindings and
  recipe fingerprints in dependency locks, with linker-independent checking.
- Added package-owned, reviewed C/Rust Adler-32 sources and an external Zore wrapper,
  with owned cleanup, validated handles and binary buffers, async/scoped result
  coverage and local/pinned Git consumption.
- The curated `tls` package accepts IPv4 and IPv6 server names, matched only
  against IP-address certificate entries and sent without SNI.
- The `tls` package now trusts the platform roots by default through
  `tls.DefaultClientConfig()` and `https.NewDefaultClient`, using
  `tls.SystemRoots()` on Linux, macOS and Windows and honoring `SSL_CERT_FILE`.
  Explicit roots (`NewClientConfig`, `https.NewClient`) override them, and empty
  roots are still rejected. The roots are loaded into Mbed TLS, so OS trust
  settings are not applied.
- The curated `tls` package negotiates TLS 1.3 by default and still accepts
  TLS 1.2 peers. TLS 1.3 uses ephemeral ECDHE, AES-GCM and ECDSA or RSA-PSS
  signatures, without PSK, 0-RTT, session tickets or KeyUpdate. The pinned Mbed
  TLS closure adds the PSA crypto core, HKDF and the TLS 1.3 sources, and TLS
  provider calls are serialized process-wide.
- Added SQLite `Database.Transaction` callback transactions: they work on `let`
  databases, commit on success, roll back on callback errors, failed commits and
  panics, and rely on SQLite's busy timeout to retry a busy commit.
- SQLite `Reset` now keeps bound values; the new `ClearBindings` sets every
  parameter to NULL.
- SQLite transactions that SQLite rolled back by itself now reject further
  statements until `Rollback`.
- Breaking: crypto `SHA256` and `HMACSHA256` return only the digest or tag and
  panic on a provider failure.
- Arbitrary native project sources, dynamic loading, general FFI, full database
  and TLS adapters remain separate work.

### Pinned Git dependencies

- Added exact-commit Git dependencies with repository subdirectories and confined
  path dependencies within fetched repositories.
- Added explicit install/lock, deterministic entry lockfiles, cache integrity
  validation and locked/offline options for check/build/run/test.
- Added atomic concurrent cache publication, lockfile guards, credential-safe
  diagnostics and cross-platform Git workflow regression coverage.

### Local library dependencies

- Added manifest-declared local project dependencies, including root and
  transitive package imports, canonical identity and direct import visibility.
- Added strict manifest validation and dependency/name/cycle/boundary diagnostics,
  plus a multi-project example and native initialization/cleanup coverage.
- Registry distribution remains a separate follow-up.

### Native platforms

- Added Windows x64 MSVC linking, `.exe` output names, a local application cache,
  and native host smoke coverage for paths, process arguments, bytes and cleanup.
- Added Windows `filepath` drive/UNC rules and explicit unsupported namespace
  diagnostics; helper-based socket waits now honor deadlines.
- Added Windows console signals and automatic HTTP signal shutdown: Ctrl+C/Break
  support orderly draining; close/logoff/shutdown cleanup is best effort.
- Added Windows host CI; native validation evidence is recorded in the platform guide.

### Scoped tasks

- Added opt-in scope blocks in plain functions, with scoped borrowing and
  child registration independent of handle drop, plus mandatory joins before
  parent storage cleanup on normal, error, loop-transfer and panic exits.
- Added scoped handle/view bounds, mixed owned/borrowed capture environments,
  deterministic panic priority and reverse result cleanup. Ordinary task APIs
  and detach behavior are unchanged.
- Added syntax/tooling, ownership, native lifecycle and runtime regressions,
  a runnable example and the locked plain-function contract.
- Added direct scopes in async functions/literals under §18.13, with persistent
  registry/borrow/return storage, pollable joins and `await` retrieval, and
  resumable unwind cleanup that preserves panic priority across worker migration.
  Completion-race and one-worker lifecycle tests cover the extension.
- Allowed awaited task retrieval as a statement, including tasks with no results
  and error-only `await child?`; unused error results still require explicit use.

### Performance evidence

- String retain and release no longer take a process-wide lock or search a
  shared map: each text buffer keeps an atomic owner count in a header, and a
  lock-free granule table finds that header from any address inside the buffer,
  including slices. Static text is still never freed, the last owner still
  frees exactly once, and `ZORE_CHECK_LEAKS` still reports unreleased text.
  On one Linux host the discarding `std/log` benchmark fell from about 13 to
  8 µs per record, and four tasks building strings no longer serialize
  (see `benchmarks/logging/README.md`).
- The string table keeps one entry per 4 KiB page instead of one per 32-byte
  granule (about 0.1% of text memory instead of 12.5%). Buffers up to 32 KiB
  are slots of 38 sizes carved from shared spans and reused through per-thread
  free lists. On one Linux host, holding a million short strings took 65 MiB
  instead of 157 MiB and 4,000 strings of 20 KB took 88 MiB instead of 90 MiB,
  with logging and four-task string building within noise of before (#237).

- Refreshed Linux/macOS runtime population evidence with compiler and source
  identities. Preserved the original three-host reports; Windows recapture is
  deferred and requires an explicit opt-in in the measurement workflow.
- Overlapped clang object generation with runtime-library preparation while
  preserving optimization flags, cache publication and compiler error priority.
  Published alternating Linux warm/cold measurements and raw evidence.
- Marked generated Zore functions internal to LLVM while preserving the exported
  entry shim and runtime ABI, allowing unused bodies to be eliminated.
- Context deadline watchers now own cancellable timer/receive registrations;
  completed HTTP requests release their deadline waits and shutdown uses a direct
  notification instead of spawning poll-alarm tasks.
- Added alternating compiler/object-size comparisons and fixed-case REST
  retention profiling with test-only runtime counters and deterministic cleanup
  regressions. Public context, time and HTTP APIs remain unchanged.

- Added opt-in compiler check/warm/cold build, REST/JSON latency/throughput,
  pinned-frame layout and settled memory/resource measurements. Preserved
  existing comparisons and added raw frame-analysis timings.
  Behavioral harness checks run in CI without host-sensitive thresholds.

### Language specification

- Locked async closure literals and receiver-capturing async method values
  (§16.1–§16.7), with ordinary captures, exclusive awaited calls, independent
  spawn inputs and deterministic cleanup.

- Fixed concrete method-value capture layouts inside generic factories, and
  returned-result destruction when a plain or async task's capture cleanup panics.

- Locked conditional borrowed map entries (§13.4): `if name = map.entry(key)`
  and `if mut name = map.entry(key)`, optional missing branches, ordinary control
  flow and scoped borrowing/cleanup. Mutable view-containing entries remain excluded.

- Locked application test discovery, signatures, package access, native process
  isolation, failure reporting and cleanup in §45.1. Ordinary builds and imports
  exclude `_test.ore` files.

- Standard packages are now imported as `"std/<path>"` (`"std/strings"`,
  `"std/os/exec"`) instead of `"zore/<path>"`; `std` is the reserved project
  name (§3.20, Q45).
- Locked the program entry point: `package main` with exactly one
  `func main()`; exit status 0 on return, nonzero after a panic (§3.19).
- Locked `println`: one printable argument, one line per call (§37.1).
- Locked the `panic` call: one `string` message reported with its location;
  a `panic` statement ends a path, so no `return` is needed after it (§15.4,
  §7.7, Q38).
- Locked rune conversions: `int(r)`, `rune(n)`, and the other integer types,
  checked at run time and folded for constants (§6.6, Q39).
- Locked named types: `type Duration int` declares a distinct type with its
  base type's operations, explicit conversions, and methods (§8.5, Q40).
- Locked package-level `let`: immutable Copy values computed once before
  `main`, in dependency and source order, with use of later values rejected
  (§3.21, Q41).
- Locked interfaces: `type Reader interface { ... }` lists methods a type
  satisfies by having them; shared and `mut` interface parameters borrow their
  argument, and other interface values own the value inside (§22.2, Q42).
  `interface` is now a keyword. In async code a call through an interface
  suspends when the method behind it waits, and interfaces may list `async`
  methods, which are awaited.
- Locked generic functions: `func Max<T ordered>(a T, b T) T` declares type
  parameters with the constraints `any`, `copyable`, `comparable`, `ordered`,
  or an interface type; calls take their type arguments from the arguments,
  bodies are checked once for every allowed type, and each set of type
  arguments gets its own compiled copy (§22.1, Q42). The four constraint names
  are now predeclared.
- Locked generic struct types: `type Stack<T any> struct { ... }` with methods
  on `Stack<T>`, used as `Stack<int>`; each set of type arguments is its own
  struct with its own method copies, and instances satisfy interfaces. A call
  whose arguments do not decide a type parameter takes it from the expected
  type, as in `var s Stack<int> = NewStack()` (§22.1, Q42).
- Locked untyped constants following Go's model: integer and float kinds,
  exact arithmetic, representability rules, and at least 256-bit precision
  (§6.7).

### Standard library

- Added `std/encoding/hex`, `std/encoding/base64`, and `std/encoding/binary`
  (§37.14). Hex and the four RFC 4648 base64 encodings (`Standard`, `URL`, and
  unpadded `RawStandard` and `RawURL`) decode strictly: anything their encoder
  would not produce is rejected, and the error names the offset of the first
  bad byte without repeating the input. `binary.BigEndian` and
  `binary.LittleEndian` read, write, and append 16-, 32-, and 64-bit signed and
  unsigned integers; a slice of the wrong length is an error, not a panic.
- **Breaking:** `std/time` has separate types (§37.3). `Duration` is a
  length of time (`5 * time.Second` keeps working), `time.Now()` returns an
  `Instant` (a stopwatch reading used for deadlines: `time.Now().Add(d)`,
  `Before`, `Sub`), and `time.UTC()` returns a `Timestamp`, a calendar time in
  a fixed-offset `Zone` with `Date`, `Unix`, `ParseRFC3339`, `RFC3339`,
  `RFC3339Nano`, calendar fields, `In`, and `Local`. `time.NewTimer` and
  `time.NewTicker` can be stopped and reset, and dropping one ends its task at
  once. `context.WithTimeout`, `context.WithDeadline`, `Context.Deadline`,
  `net.DialTimeout`, the `net` deadline setters, `http.Config`,
  `http.ClientConfig`, `http.ClientConn`, and `rolling.Config.FlushInterval`
  use the new types; `net` gains `ClearDeadline()` in place of a zero deadline.
  `time.FormatHTTPDate` became `http.FormatDate(t time.Timestamp)`, and
  `log.FormatTime` was removed in favor of `time.Unix(seconds, 0).RFC3339()`.
- Added the `package-boundaries` agent skill, required by `AGENTS.md` for any
  change to the public names or imports of `std/`.
- Local time (§37.3, §37.12, §37.13): `time.LocalOffset` reports the operating
  system's offset from UTC at any instant, daylight saving included, following
  `TZ` on Linux and macOS and the system zone on Windows.
  `Logger.WithLocalTime()` opts records into local RFC 3339 stamps with a
  numeric offset (`2026-11-01T01:00:00-05:00`), and `rolling.Config.Local` makes daily rolling split at local
  midnight with backups named by local date. UTC stays the default.
- `std/log/rolling` can also roll daily (§37.13): with `Config.Daily`, a new
  file starts when the UTC day changes, older files are named by day
  (`app.log.2026-10-29`, `app.log.2026-10-29.1` for a second file that day),
  and the oldest dated backups beyond `MaxBackups` are removed. A restart
  continues the current day's file. `os.FileInfo` has `ModTime()` (§37.3).
- Added `std/log/rolling` (§37.13): a size-limited log file that rolls into
  numbered backups, keeps the newest `MaxBackups`, and collects records in a
  64 KiB buffer flushed by a background task every second. `Writer` owns the
  file and flushes and closes it on `Close` or drop; `Sink()` gives the logger a
  shared handle. In the recorded benchmark a buffered rolling sink costs about
  the same as discarding records, roughly five times faster than writing each
  record to a file.
- Added `os.Append` to open a file for writing at its end and `os.Rename`
  (§37.3).
- Added `std/log` (§37.12): a Copy `Logger` over any `io.Writer` with
  `DEBUG`/`INFO`/`WARN`/`ERROR` levels and compiler-checked message templates:
  `logger.Info("listening on {addr}", addr)` fills the message and adds the
  field `addr`, and `logger.With("key", value)` adds fields to every record.
  Records are one-line `key=value` text with stable quoting and UTC RFC 3339
  second timestamps (`FormatTime`). Copies share one
  lock, so records from different tasks never interleave; the first output
  failure is kept, stops later records, and is reported by `Err`.
- Fixed `bytes.Buffer` retaining consumed history across repeated writes and
  reads. Writes now compact consumed prefixes and reuse existing capacity.
- Refactored the standard packages to one naming and signature pattern
  (§37.2–§37.5): `strings.ToUpper`/`ToLower`/`ReplaceAll`, byte-based
  `os.ReadFile`/`WriteFile`, `net.Conn.Read`/`Write` into byte buffers,
  nanosecond durations with `time.Millisecond` and friends, network deadlines,
  and error messages that name the rejected input.
- Replaced `zore/io` with `zore/bufio` and `zore/cancel` with `zore/context`.
- Added `zore/bytes`, `zore/errors`, `zore/unicode`, `zore/unicode/utf8`,
  `zore/path`, `zore/path/filepath`, `zore/sort`, `zore/sync`, and
  `zore/os/exec`, and more functions in `strings`, `strconv`, and `os`.
- Added `zore/io` with the `Reader`, `Writer`, `Closer`, `ReadWriter`,
  `ReadCloser`, and `WriteCloser` interfaces, `EOF`, `Copy`, `ReadAll`,
  `ReadFull`, and `WriteString` (§37.3). `bufio` now reads and writes any
  `io.Reader` or `io.Writer`, so it works over connections and buffers as well
  as files, and `bufio.Reader` has `Read`.
- Added `sort.Slice` and `sort.SliceIsSorted`, and the generic `zore/slices`
  (`Contains`, `Index`, `Equal`, `Sort`, `IsSorted`, `Reverse`, `Max`, `Min`,
  `Clone`) and `zore/maps` (`Keys`, `Values`, `Clone`) packages (§37.5).
- Locked the float text form (§37.1, Q43): `println` of a `float32` or
  `float64` writes the shortest decimal that reads back to the same value,
  always with a point (`3.0`), in exponent form below 1e-4 or from 1e21 up
  (`1e+21`), with `NaN`, `+Inf`, `-Inf`, and `-0.0`. Added
  `strconv.FormatFloat` and `strconv.ParseFloat`, and a `zore/math` package
  with constants, `Sqrt`, `Floor`, `Ceil`, `Trunc`, `Round`, `Pow`, `Mod`,
  `Exp`, `Log`, `Abs`, `Max`, `Min`, `Inf`, `NaN`, `IsNaN`, and `IsInf`.
- Added `zore/fmt` (§37.2, Q44): `Print`, `Println`, `Printf`, `Sprint`,
  `Sprintln`, `Sprintf`, and `Errorf`, with Go's verbs, widths, and float
  precision. The compiler checks each format against its arguments, and types
  with a `String() string` method print through it.
- Errors can carry a cause (§15.1, Q44): `errors.Wrap`, `errors.Unwrap`,
  `errors.Message`, `fmt.Errorf` with `%w`, and an `errors.Is` that searches
  the whole chain.
- Added `net.EmptyConn`, a closed connection for packages that need an empty
  connection value (§37.3).
- Function literals are allowed inside generic functions and methods (§22.1).

### Compiler

- A panic is now reported on standard error when it is raised, before the
  task unwinds (§18.10). Before, a spawned task's report came after its
  unwinding, so another task could see the poisoned mutex it left and end the
  process before the cause was printed; programs could show only
  `withLock on a poisoned mutex`. This also fixes the occasional Windows
  failure of `log_sink_panic_poisons_every_copy_of_the_logger`.
- Added direct conformance coverage for nested return/clone provenance,
  primitive/library resource transfer, lifetime-syntax rejection, and
  collection-field resurrection. Destructor-observed views now have bounded
  normal/panic regressions across observed timer, mutex and socket suspension.
  A test-only timer-expiry hook keeps the timer fixture independent of timing.

- Added `zore fmt <file|directory>` and `--check`, with syntax validation,
  comment/literal and statement-boundary preservation, recursive source
  discovery, and atomic replacement of each changed file.
- Added conditional map-entry bindings for Copy/Move values, using existing map
  lookup and ownership checking, with receiver borrows, ordinary block control flow
  and initialized-part cleanup on mutable panic. Added paired regressions and a Move example.
- Added `zore test <file|directory>`, using ordinary checking and native linking,
  one build per package and one process per test. Tests can return nothing or
  `error`; errors and panics fail their test while later tests continue.
- Added the multi-file application test sample and driver regressions for
  discovery, isolation, failure reporting and deterministic cleanup.
- Audited MVP implementation/test evidence and recorded focused conformance,
  tooling, library, performance, portability, and task follow-ups. Further
  self-hosting is deferred.

The remaining compiler entries record the initial bootstrap implementation;
current feature support is documented in the README and roadmap.

- `zore` command-line driver with `check`, `build`, and `run`; `fmt` and
  `test` report that they are not implemented.
- Source manager with stable file IDs, byte spans, and source-aware diagnostics.
- Lexer for the locked lexical rules, including automatic semicolon insertion,
  string/rune escapes, numeric literals, and error recovery.
- Parser for packages, functions, structs, bindings, statements, and the full
  operator table, with recovery; later-milestone syntax is reported as
  unsupported.
- Name resolution with stable IDs, and type checking into typed HIR for a
  single-file, synchronous, all-Copy subset: `bool`, integer and float types,
  `rune`, `string`, and structs of those.
- Exact constant evaluation with hand-written arbitrary-precision arithmetic
  and correctly rounded float conversion.
- MIR lowering and an LLVM IR backend compiled by clang, with runtime checks
  for overflow, division by zero, shift counts, and conversions; a minimal C
  runtime (decision record 0001).
- The first semantic target (spec §42) builds and prints `John`.

### Initial bootstrap limitations

The initial bootstrap did not yet support ownership and borrow checking,
drop insertion, `error` and `?`,
collections, closures, async/await, tasks, channels, imports, runtime string
concatenation, or printing floats. These features are now implemented for the
supported subset; see [the roadmap](docs/roadmap.md).

[Unreleased]: https://github.com/maasdi/zore/commits/main
