# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Zore has no releases
yet; version numbers will follow [Semantic Versioning](https://semver.org/)
once it does.

## [Unreleased]

### Scoped tasks

- Added opt-in taskScope blocks in plain functions, with scoped borrowing and
  child registration independent of handle drop, plus mandatory joins before
  parent storage cleanup on normal, error, loop-transfer and panic exits.
- Added scoped handle/view bounds, mixed owned/borrowed capture environments,
  deterministic panic priority and reverse result cleanup. Ordinary task APIs
  and detach behavior are unchanged; direct async scopes remain rejected.
- Added syntax/tooling, ownership, native lifecycle and runtime regressions,
  a runnable example and the locked plain-function contract.

### Performance evidence

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
- Function literals are allowed inside generic functions and methods (§22.1).

### Compiler

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
