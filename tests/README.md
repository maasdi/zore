# Compiler testing

`tests/packages/local_dependencies.rs` checks manifests, transitive and diamond
graphs, direct visibility before cache reuse, root imports, conflicts, cycles,
canonical project aliases and package boundaries. `tests/codegen/local_dependencies.rs`
exercises check/build/run/test on separate projects with Unicode/space paths,
selected library tests, initialization once and async success/error/panic cleanup.

`tests/driver/release_metadata.rs` covers canonical numeric version/tag identities,
minimal SPDX/file licensing, optional metadata, compiler/platform compatibility,
portable/constrained file paths, Unix symlink escape, native recipe reuse and
metadata-only CLI behavior without Git or native tools. It preserves legacy
version strings and version-1 locks; ordinary omission warnings remain absent.
Run `cargo test --locked --test release_metadata`. Full Git and native suites
continue to cover commit/tree/cache integrity and exact-pin behavior.

CI runs host-applicable Rust checks on Linux, macOS and Windows x64 MSVC.
`tests/codegen/host_support.rs` covers native paths with spaces/Unicode, CLI
build/run/test, process arguments and working directories, captured bytes,
file-handle destruction, Windows exit statuses and filepath behavior. A runtime
helper test forces competing queued jobs before idle helpers can wake. Unix
shell and POSIX signal cases retain explicit host gates. Windows signal and HTTP
tests run real Ctrl+C/Break in isolated consoles; runtime tests invoke termination
callbacks directly to check mapping and bounded waiting during unsubscription.
Real close/logoff/shutdown delivery is not established by those synthetic tests. See [platform support](../docs/platform-support.md).

`tests/ownership/scoped_tasks.rs` and `tests/codegen/scoped_tasks.rs` cover the
plain lexical task-scope contract: shared/exclusive inputs, mixed captures,
dropped/aliased/repeated handles, view and output provenance, shorter storage,
all normal/error/panic exits, nested scopes, result destructors, bounded deadlock
and compensation on one worker. `tests/codegen/async_scopes.rs` covers direct
async scopes with parked-task checkpoints, pollable joins and retrieval,
suspended unwind, worker migration, one-worker progress and result destruction.
Frame tests check registry/referent/return persistence and prevent registry slot
reuse. Runtime units verify record/result ownership, failure ordering independent
of completion order and completion/wakeup races. Lexer/parser/formatter and
self-hosting comparisons include the syntax.
See [conformance coverage](conformance/scoped-tasks.md).

The native linkage/timer module checks optimized symbol visibility, referenced
initializers/callbacks/interfaces/generics/async cleanup, and diagnostics in
unused code. Controlled context tests force real Pending states and timer
expiry, then assert scheduler quiescence and zero live timer registrations after
cancellation, expiration and parent propagation. Runtime regressions cover
deadline/channel races, exact reference cleanup, and bounded stale timer records.
These are state assertions, not wall-time or RSS thresholds.

The performance workloads have behavioral coverage in the new native
performance module and the REST benchmark case in the HTTP tests. They check
repeated JSON success/rejection, owned payloads through borrowed async child
calls, REST success/error responses and clean drain. Resource checks are enabled;
they are skipped by the runtime while tasks remain, including detached HTTP
deadline timers. The performance report records that limitation.
The Python harness tests check real clang layout, warmed connection reuse,
wrong-response failure and invalid configuration. CI runs these checks without
performance thresholds. See the
[opt-in measurement guide](../benchmarks/performance/README.md).

`tests/ownership/map_entries.rs` and `tests/codegen/map_entries.rs` verify scoped
map-entry bindings, including headers, aliases, escaping views, generic
constraints, argument evaluation, Copy/Move/interface values, normal/error/panic
cleanup and unchanged lookup/removal. See [contract coverage](conformance/map-entries.md).

`tests/driver/testing.rs` verifies the application test runner: both signatures,
discovery and file selection, private helpers, hidden dispatcher imports,
fresh package initialization, ordinary ownership/error checking, imported-test
exclusion, preflight builds, no-test behavior, async work, returned errors,
panics, nonzero exits, continued execution, temporary artifact cleanup, and
deterministic recursive traversal with symlink exclusion. It executes the
multi-file `examples/application-tests` sample, including its opt-in failures.
See the [application testing guide](../docs/testing.md) and
[contract coverage](conformance/application-tests.md). Native cases require the
same clang/rustc toolchain as `zore build`.

`tests/format/format.rs` checks source formatting, idempotence, preservation of
literal/comment spellings and statement boundaries, and every repository `.ore`
source under examples, standard packages, the Zore frontend, and benchmarks.
`tests/driver/cli.rs` covers formatter check/write modes, preflight errors,
recursive traversal, symlink exclusion, and permission preservation.
The [MVP evidence inventory](../docs/conformance/mvp.md) distinguishes executed
assertions from pending conformance scenarios and historical test gaps.

`tests/codegen/conformance.rs` adds #114's nested return/clone, concrete
resource transfer, and view-observing destructor regressions. Timer, mutex and
socket cases verify actual Pending state on the controlled two-worker runtime
before releasing the wait, and exercise normal and panic cleanup. The timer
fixture expires its registered long timer through a test-only reactor hook;
real clock behavior remains covered by existing timer tests. Each process is
bounded by 30 seconds, and the state checkpoint by 10 seconds.

`tests/driver/cli.rs` contains executable subprocess tests for the M0 driver: help,
version, usage errors, unsupported commands, native paths, and option delimiters.
`tests/diagnostics/source_diagnostics.rs` tests M1 UTF-8 loading, file IDs, byte spans,
line/column lookup, EOF, and multi-file diagnostic rendering. `tests/lexer/lexer.rs`
tests M2 token kinds, spans, literal validation and decoding, semicolon
insertion, diagnostics, and recovery progress; its cases come from the lexical
rows of the conformance documents below. `tests/selfhost/lexer.rs` builds the
Zore lexer in `compiler-zore/` and compares its tokens, byte spans, decoded
literals, and error codes with the Rust lexer. The inputs are targeted cases,
each line of the lexer tests, the lexical conformance documents, every `.ore`
file in the repository, and seeded generated text and bytes.
`tests/selfhost/harness.rs` verifies large-output capture, failure diagnostics,
and timeout kill/reap behavior. Each oracle batch has a 60-second execution
deadline with file-backed stdout/stderr; timeout failures name the unfinished
case and include output tails. Compilation is outside this execution deadline.
`tests/selfhost/parser.rs` does the same for the Zore parser, source manager,
and diagnostics. It compares complete syntax trees with byte spans, every
parser diagnostic with its labels, notes, and rendered text, and line and
column lookups. The inputs are the string literals of the parser, lexer, and
source tests, conformance code spans, specification code blocks, repository
sources, and seeded token sequences, expressions, and edits of the examples. `tests/parser/parser.rs` tests M3–M4 AST
shape (via an S-expression rendering), spans, syntax rejection, unsupported
later-milestone syntax, recovery, and termination on generated input.
`tests/typecheck/check.rs` tests resolution, type checking,
§6.7 constant evaluation, float typing and conversions, HIR shape, the
entry-point and `println` contracts, and that unsupported features are rejected
rather than accepted. Error-value tests cover `nil`, construction, equality,
explicit discard, ignored-result diagnostics, and path-sensitive checks for named
error bindings and parameters. Synchronous `?` tests cover typing, early return,
zero-filled results, evaluation order, and cleanup; awaited calls cover async
function declarations, the await-or-spawn rule, `await` outside async bodies,
and awaited `?`; task cases cover `Task<...>` types and annotations, `go`
typing and its input rules, Move classification, `nil`, `.wait()`, and
`await task`. The native tests run tasks to completion, including results and
errors, ownership transfer, panics (waited and detached), an endless detached
task, `nil` waits, async functions awaiting tasks, many tasks sharing text, and tens of thousands
of tasks running and waiting on each other, and channels: ordering, ownership and
drops of messages, close and drain, the zero-value channel, blocked operations
woken by close, reply channels, worker pools, and a chain of a thousand tasks. Deadlock tests cover a receive or send nobody answers, tasks waiting on each other, and a wait on a task that waits forever, and check that timers, helper threads, a pending accept, and busy tasks are not mistaken for one. `conformance/io.md` also covers the byte functions: text and bytes converting both ways with the UTF-8 check, files holding any bytes, and a connection moving raw bytes. `conformance/io.md` also covers deadlines and cancellation: `After` firing once, sockets giving up at their deadline without losing data, contexts cancelling their children, and wait groups. `conformance/select.md` covers `select`: native tests pick the ready case, skip a full buffer, wake on a producer or a close, treat a zero-value channel as ready, panic on a send to a closed channel, and report a `select` nobody can complete as a deadlock. `conformance/mutex.md` covers `Mutex<T>`: its native tests count 100,000 updates from 100 tasks, drop a guarded Move value once, return results from the callback, make waiters wait in arrival order without blocking other tasks, poison the mutex when the callback panics, and report a re-lock and an opposite-order pair as deadlocks. `conformance/io.md` covers `std/time`, `std/os`, `std/os/exec`,
`std/bufio`, `std/net`, `std/context`, and `std/sync`; its native tests
sleep, scan piped standard input, round-trip files, list and remove folders, run
child processes, echo bytes through a loopback TCP server (including a peer
written in Rust that sends bytes that are not text), and check that a task
waiting on input, a timer, or `Accept` never stops another task.
`conformance/packages.md` lists the text, byte, path, and sorting packages,
whose native tests compare each function with its documented result.
`conformance/interfaces.md` lists interface declarations, satisfaction,
borrowed and owned values, and their restrictions; the checker, package, and
native tests cover each row, and the native tests run with the leak checker to
show each value inside an interface is destroyed exactly once, and that async
code reaching a connection through an interface suspends on the connection's
own reads and writes.
`conformance/generics.md` lists generic function declarations, constraints,
type argument inference, the checks on generic bodies, and the copies made per
set of type arguments; the checker, package, parser, and native tests cover
each row. `tests/ownership/ownership.rs` tests the MIR ownership pass through the
full frontend: whole-place and partial moves, reinitialization, call-local
borrows, the custom-`drop`-ancestor restriction, array- and slice-element move
rules, and region analysis of stored slice borrows (aliasing at the
originating place, reborrow suspension, last-use release, scope-end and
loop-carried escapes, and return-borrow contracts).
Unit tests in `compiler/src/types/bignum.rs`
and `compiler/src/types/constant.rs` check
big-number arithmetic and float rounding against Rust's `i128` and correctly
rounded `str::parse` as oracles. `tests/codegen/native.rs` builds programs with clang,
runs them, and compares stdout, stderr, and exit status, including every §6.6
runtime panic, evaluation order, a closed standard output, and the CLI
`build`/`run` commands. It requires clang with LLVM 15 or newer (or
`ZORE_CC`) and rustc 1.98+ (or `ZORE_RUSTC`); without them tests fail rather than being
skipped. Add tests
alongside each stage; do not create ignored tests to imply that pending features
have coverage.

The `zore-runtime` library has unit tests for integer formatting and string ABI
boundaries. Root-level Cargo checks include this crate. The native suite also
covers empty and long strings, embedded NUL bytes, output paths with spaces,
missing rustc diagnostics, and checking without either native tool.

`tests/async/lowering.rs` tests the backend-independent suspension plan for async functions,
cleanup edges, retained source spans, recursive calls, and synchronous native
compatibility. The native suite verifies heap-frame IR and real suspensions through
nested calls and task joins, mutable borrows and views, closure environments,
loops and one-time argument evaluation, Move/partial-move cleanup, returned
collections, errors, nil tasks, panic containment, detached results, and a chain
of 5,000 polled tasks. The runtime poll-entry test exercises repeated
wake-before-Pending through the generated-code ABI and existing blocking handle
retrieval. The channel tests run the same programs as plain pool jobs and polled async
tasks, including buffered/unbuffered loops, mixed waiters, select/default/zero
channels, duplicate cases, single operand evaluation, reverse send cleanup,
mutable borrows, partial moves, close panics, deadlocks, and 5,000 channel waiters.
Runtime ABI regressions force repeated polls, wake-before-Pending, pending close,
and concurrent select winners with duplicate registrations. Timer tests also
compare plain and async execution for positive/nonpositive sleeps, live views and mutable borrows, Move cleanup, one-time loop arguments,
After/select, panic unwinding, detached results, transitions back to internal
deadlock detection, and 2,000 sleepers with plain blocking helpers.
Runtime reactor tests cover the start/poll ABI, actual timer completion,
wake-before-Pending descriptor readiness, deadline/registration errors, concurrent
completion, readiness/timeout races, and released completion storage. Mutex and
I/O poll forms are implemented and covered below.

`conformance/` records spec-level cases awaiting executable coverage. In
particular, `conformance/identifiers.md` covers the locked ASCII identifier rules,
`conformance/comments.md` covers line and non-nesting block comments, and
`conformance/statement-boundaries.md` covers automatic semicolon insertion.
`conformance/strings.md` covers quoted and raw string literal forms and the
string operations of §6.8. `conformance/packages.md` covers projects, packages,
and imports (§3.20); its executable cases are in `tests/packages/packages.rs`
(in-memory projects), the driver tests, and the native tests. The runtime
crate's unit tests cover the native string functions directly.
`conformance/runes.md` covers single-scalar rune literals and escapes.
`conformance/integers.md` covers integer bases and prefix validation.
`conformance/floats.md` covers decimal fractions and scientific notation.
`conformance/fmt.md` lists the `std/fmt` functions and directives, the
compile-time checks on formats, and `Errorf` causes; the checker and native
tests cover each row.
`conformance/keywords.md` covers MVP keywords and future-reserved words.
`conformance/entry-point.md` covers the `main` entry point and exit status (§3.19).
`conformance/println.md` covers `println` arguments, output text, and misuse (§37.1).
`conformance/discards.md` covers `_` targets and their ownership implications.
`conformance/errors.md` covers explicit error discards and ignored-result diagnostics.
`conformance/evaluation-order.md` covers left-to-right operand and call evaluation.
`conformance/expressions.md` covers operators, grouping, and short-circuit behavior.
`conformance/bindings-assignments.md` covers declarations, updates, swaps, and scopes.
`conformance/control-flow.md` covers blocks, branches, loops, returns, and exit cleanup.
`conformance/functions-structs.md` covers calls, forwarding, members, and construction.
`conformance/zero-values.md` covers per-type zero values and `nil` restriction.
`conformance/constant-expressions.md` covers the constant-expression subset and
forward-reference/cycle rules.
`conformance/ownership.md` covers return-borrow contracts, caller mutability,
partial moves/reinitialization, custom-destructor restrictions, recursive borrow
provenance, and exclusive mutable-slice reborrows.
`conformance/destruction.md` covers drop receiver rules, Copy/clone
interaction, clone availability and cleanup after a panicking clone, resource
zero states, and panic unwinding.
`conformance/concurrency.md` covers async calls, task typing and retrieval,
task panics, process exit, all-exit spawn lifetime proof, and channel message
escape/close/zero-value/cleanup behavior.
`conformance/arrays-slices.md` covers typed array literals, bounds, indexing,
contextual mutable slices, and partial-construction cleanup (§12.6).
`conformance/maps.md` covers map literals, key restrictions, presence-first lookup
and removal, mutation, and entry cleanup (§13.3).
`conformance/closures.md` covers closure literals, function types, inferred
captures, exclusive calls, the non-escaping rule, and cleanup (§16); every row
except those needing tasks or `await` has a counterpart in the parser, check,
ownership, and native suites.
These documents do not count as passing tests. Lexical rows in `identifiers`,
`comments`, `statement-boundaries`, `strings`, `runes`, `integers`, `floats`,
and `keywords` now have executable counterparts in `tests/lexer/lexer.rs`. Syntax rows
in `statement-boundaries`, `expressions`, `bindings-assignments`,
`functions-structs`, `control-flow`, and `discards` have parser counterparts in
`tests/parser/parser.rs`. Resolution and typing rows for the checker subset in
`entry-point`, `println`, `numerics`, `constant-expressions`,
`bindings-assignments`, `functions-structs`, `control-flow`, and `keywords` have
counterparts in `tests/typecheck/check.rs`. Field-level partial moves,
reinitialization, and the custom-`drop`-ancestor restriction in `ownership`
have counterparts in both `tests/ownership/ownership.rs` and
`tests/codegen/native.rs`. Fixed-array literal/arity typing, indexing
(including static out-of-range rejection), and mutable-place and
conservative-aliasing rules in `arrays-slices` have parser counterparts in
`tests/parser/parser.rs` and checker counterparts in
`tests/typecheck/check.rs`; index-move rejection is in
`tests/ownership/ownership.rs`; LLVM codegen (construction, index read/write,
runtime bounds-check panics, and element cleanup) has native counterparts in
`tests/codegen/native.rs`. Slice types, slicing, contextual mutable views,
static slice-bound rejection, and slice-element mutability rows in
`arrays-slices` and `ownership` have parser and checker counterparts; the
region, aliasing, reborrow, and return-contract rows have counterparts in
`tests/ownership/ownership.rs`, and native slicing, element access, and slice
bounds panics have counterparts in `tests/codegen/native.rs`. `Array<T>`
literals, typing, element mutability, slicing, move and view rules, and
native construction, access, drop order, and panics have counterparts in the
parser, check, ownership, and native suites, as do the `maps` rows for
literals, key types, lookup, assignment, removal, provenance, and cleanup.
Growth and length APIs, map iteration, and rows that depend on async, tasks, or channels
remain pending, as do the remaining runtime and native rows elsewhere.

Use Rust unit tests for small source/IR utilities and pass algorithms. Use Cargo
integration tests in subsystem folders under `tests/` for public compiler APIs
and CLI subprocess behavior. Each target is registered in `compiler/Cargo.toml`
with its original name (`cli`, `source_diagnostics`, `lexer`, `parser`, `check`,
`ownership`, and `native`), so commands such as `cargo test --test lexer` still
work.
Resolve examples from `env!("CARGO_MANIFEST_DIR")` plus `../examples/`, not the
process working directory. Every directory under `examples/` is checked by
`tests/driver/cli.rs`, and built and run by `tests/codegen/native.rs` against
its `expected-output.txt`. A directory of fixtures alone is not an executable test.

Introduce fixture suites as the corresponding stage is implemented:

| Suite | Check |
| --- | --- |
| Lexer | Token kind/text/span; invalid input, UTF-8, EOF, recovery progress |
| Parser | AST structure/spans; syntax rejection and recovery |
| Semantic | Resolution/types; diagnostics with primary and related locations |
| Ownership | Copy/Move, projected places, aliases, branches, explicit drop, use-after-move, double drop, task/await borrow validity |
| Native | Compile, execute, compare stdout/stderr/exit status; cleanup on each control-flow path |
| Async/runtime | Suspension/resume, live values, destruction, results/errors, detachment, buffering, close/drain, message transfer |

When a fixture runner is added, place inputs under `tests/<suite>/fixtures/` and
declare each case's expected outcome explicitly in the runner or case metadata.
Cover both accepted and rejected cases. References to spec sections should explain
why each semantic result is expected. Choose the metadata format with the runner,
not by inventing source annotations or language syntax now.

Avoid absolute-path and platform-dependent expectations. Verify diagnostic
locations and meaningful content; snapshot presentation only when useful. Native
and concurrency tests need bounded execution and isolated temporary directories;
prefer explicit synchronization over timing sleeps. Keep frontend suites runnable
without LLVM. Explain any backend-specific prerequisites and how CI invokes them.

The examples are future acceptance inputs. §46 defines the required coverage for
the full MVP; expand this plan and the executable suites as features arrive.

Mutex poll regressions cover FIFO grants across slots and task wakers, repeated
polls, wake-before-Pending, poisoning, zero handles, and concurrent exclusive
access. Native parity cases compare plain and poll execution with blocking
callbacks, Move results, contention, panic cleanup, and heap-only poll storage.

I/O poll regressions compare plain and async file, stdin, networking, and
context APIs. They cover byte views and Move results, buffered reads, EOF and
invalid input, deadline recovery, error propagation and cleanup, shared socket
registration, partial writes, and heap-only async storage. Runtime helper tests
use a single poll worker to check progress while work is pending, completion
before Pending, nested helper calls, and task-local panic transfer. Socket tests
use loopback peers and bounded waits, including backpressure and reads after a
deadline has passed.

The async runtime uses one pool for plain entries and async polls on every host.
The plain-entry ABI test verifies pool worker execution, one-time result retrieval,
and completion before joining. Mixed native tests combine plain channel waits,
polled children, and nested blocking joins; existing panic, detach, mutex,
deadlock, and I/O parity tests now exercise the shared pool. Slot regressions race
parking with simultaneous wakes and verify latched completion and balanced internal
wait accounting. Compensation tests saturate a small pool and verify replacement
worker progress, nested guards, and retirement back to its original capacity.

Large waiting workloads use `async func` task functions, including the 5,000-task
join chain, the 1,000-link channel chain, and timer waiters. The 50,000 computation
tasks do not wait and remain plain functions. A waiting plain function holds an OS
worker; replacement threads maintain progress but remain subject to host thread
limits. A 1,000-child context regression checks the private async expiry
and parent-following tasks.

## Controlled suspension regressions

`codegen/controlled_runtime.rs` builds a separate `cfg(test)` runtime once for
focused native regressions. Each generated program runs in its own process with
a two-worker pool. Only the bodies of the test fixture's `waitPending` and
`waitBlocked` functions are replaced in emitted LLVM, to call bounded Rust state
observers. Generated async bodies, ownership/drop lowering, and wait operations
remain the compiler's real output. These hooks do not exist in production builds
and add no Zore builtin or runtime configuration option.

The observers clone the task list before acquiring per-task scheduling locks,
preserving the scheduler's lock order. Idle status establishes that a task has
returned Pending; the blocked-worker count establishes compensation is needed.
The fixture has no unrelated waiters that could satisfy its expected count.

`codegen/review_regressions.rs` covers empty interface adapter cleanup, float32
formatting overflow/underflow, empty-message error causes, and partial I/O.
The filesystem fixture uses its allocated temporary directory and tests that
requested permissions are not widened, allowing the inherited umask to remove
permission bits.

`conformance/http.md` maps the HTTP/URL contract to native loopback and ownership
regressions. It distinguishes tested behavior from additional fault-injection
coverage. The REST example codec is also exercised as executable Zore source.

`conformance/json.md` maps the JSON token, tree, and generated-codec contract to
`codegen/json.rs`, JSON package-checking tests, and the console/REST examples.
Native JSON programs run with leak checking, including partial-decode errors.

- `driver/git_dependencies.rs`: local Git fixtures exercise fresh/repeated install,
  explicit lock regeneration, locked/offline commands, integrity failures, source conflicts,
  transitive graphs, confinement, concurrent publication and native diamond reuse.

- `packages/native_adapters.rs`: curated registration, root-only private bindings,
  malformed metadata, unchanged bodyless-source rejection and Move/async checking.
- `driver/native_adapters.rs`: static C ABI, package-owned source integrity and
  confinement, missing-source rejection after cache population, local/Git consumption,
  recipe drift, cache selection and owned resource cleanup across async and scoped
  results, including normal/error/panic paths.
