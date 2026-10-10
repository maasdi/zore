# Implementation roadmap

The implementation covers a subset that includes tasks and async functions:
bool, integers, floats, rune, string, `error`, and structs, including Move structs with
custom `drop` methods and exact untyped constants (§6.7). `zore check` runs lex → parse → resolve → type-check and MIR ownership analysis;
`zore build` and `zore run` lower to MIR, emit LLVM IR, and invoke clang and
rustc to link the Rust runtime (decision record 0001). Unsupported features
receive diagnostics. A separate MIR pass inserts deterministic drops and
cleanup edges for checked operations and calls. The runtime propagates panic
status through synchronous frames and reports an initial-task panic after
cleanup.

The Q32 implementation and the subsequent socket-test fix passed
[main CI on 2026-10-08](https://github.com/maasdi/zore/actions/runs/37726483242)
on Ubuntu and macOS: rustfmt, clippy with warnings denied, build, docs, and
`cargo test --locked --all-targets`. This validates the tested subset at that
commit; it does not establish full MVP coverage. Windows native linking has
not been validated here.

Canonical M IDs follow specification §43. Detailed phases and their mapping
to those IDs are below, followed by the active validation work package.

Broad specification preparation has ended: resolve further language questions
only when they block the active implementation milestone. No currently
recorded language question blocked M0–M4; Q13 and Q14 record the conservative
lexer and parser choices made where the spec is silent. Q15 is resolved by
§6.7 (Go's untyped-constant model), which the checker implements. Numbers refer to spec §43; the sequence
is guidance, not a language contract.

| Milestone | Deliverable and acceptance criteria |
| --- | --- |
| M0 — implemented and tested | CLI/driver: help, version, argument validation, unsupported-command errors, and subprocess exit/output tests. `check <file.ore>` works. |
| M1 — implemented and tested | Source manager, file IDs, byte spans, and diagnostic rendering, including UTF-8 and multiple-file cases. |
| M2 — implemented and tested | Lexer and agreed token rules, with span, invalid-input, EOF, and recovery tests. Q01 lexical choices are resolved. |
| M3–M4 — supported subset implemented and tested | AST and parser for the implemented language, including packages, functions, structs, collections, closures, and async syntax. Unsupported forms receive diagnostics. |
| M5–M8 — supported subset implemented and tested | Native hello program, variables, functions, structs, and control flow. Backend and output support run native examples. |
| M9–M12 — supported subset implemented and tested | Name resolution, types, HIR, MIR/CFG, semantic IDs, typed calls/fields, explicit control flow, and frontend-only checking. Package-level variables remain unsupported. |
| M13–M17 — partial | Copy/Move classification, mutable borrowing, whole-place move analysis, field-level partial moves (with reinitialization and the custom-`drop`-ancestor restriction), and stored borrows with region analysis exist in CLI checking and builds. Region analysis tracks slice loans per local, frees them at the holder's last use (backward liveness), checks exclusivity at the originating place, suspends sources during mutable reborrows, rejects views that outlive a local or temporary owner, and infers return-borrow contracts by fixpoint. `mut []T` may be a struct field or fixed-array element; copying such a value reborrows each view inside it (Q20). `mut []T` may also be held in `Array<T>`, map, and `mut []T` elements, and views may be stored through mutable slices (Q20b, Q22b). Tasks reject every borrowed input (Q25), so no borrow crosses a task boundary. Views may be stored through `mut` parameters and closure captures (Q22). A custom `drop` may read a contained view; its borrows last until the value is destroyed (Q21). Complete the §42 semantic target with paired acceptance/rejection tests. |
| M18–M19 — supported subset implemented and tested | Drop insertion and panic cleanup cover supported synchronous and async paths. The concrete Copy `error` type, `nil` in an error context, `error(message)`, content equality, explicit discards, rejection of silently ignored error results, and path-sensitive checks for named errors are implemented. `?` propagates call errors, zero-fills other return values, and runs cleanup, including after awaited async calls and task results. |
| M20–M23 — partial | Fixed-array types, typed literals, and indexing (read, mutable-place write/replacement, conservative-aliasing, and rejection of moving an element out through an index) are implemented end to end: `zore check`, and now `zore build`/`zore run` — LLVM `[N x T]` type emission, GEP-based indexed addressing, runtime bounds-check panics, and element cleanup without per-element drop flags (sound because element extraction stays rejected). Borrowed slices (`[]T`, `mut []T`, `base[low:high]`, contextual exclusive views, indexing and element writes through views) are implemented end to end: `zore check` with region analysis, and `zore build`/`zore run` with `{ ptr, i64 }` descriptors, slice-aware addressing, and runtime "slice bounds out of range" panics. Dynamic `Array<T>` is implemented end to end for literal-sized arrays: typed literals, indexing, element writes, slicing, borrow/`mut`/`own` passing, returns, the zero value, and heap storage freed after dropping elements in reverse order (Q17). `len`, `push`, and `pop` (§12.7) work, and `Array<T>` storage keeps a capacity so pushes are amortized. Finite recursive owned structs through `Array<T>` and maps are supported, including mutual recursion, structural/custom cloning, and cached out-of-line destruction; by-value layout cycles remain rejected. Runtime clone/drop depth is native-stack-bounded, not unbounded-stack-safe. Packages and imports work; ownership and visibility still need broader conformance coverage. Maps (`map[K]V`, §13.3) are implemented end to end: typed literals with static and runtime duplicate-key rejection, the two-result lookup for Copy values, `m[k] = v` insert/replace, and `m.remove(k)` transferring ownership (Q18). `m.len()` and `for key, value in m` loops work (§5.10, §12.7); borrowed entry access remains Q02/Q05. |
| M24 — partial | Closures (§16, Q02g, Q02i) work end to end in `zore check`, `zore build`, and `zore run`: closure literals, `func(T) R` types, calls through values, function-typed parameters, captures inferred per whole local as shared or exclusive loans checked by region analysis, nested captures, `?` and panic cleanup inside closures. Paired acceptance/rejection tests cover each rule. Owning closures (returned or stored, with heap environments and destructors) and call-once closures are inferred automatically. Declared synchronous functions, including package-qualified ones, are function values (Q33); `async func` names and methods are rejected as values. Remaining, each diagnosed as unsupported or rejected: storing a view of a closure's own parameter into a capture, and closures with tasks or `await`. |
| M25–M29 — tasks implemented; state machines with all supported waiting operations implemented | `async func`, `await`, `go`, `Task<...>`, `.wait()`, and `await task` work end to end in `zore check`, `zore build`, and `zore run` (§17–18, Q25). An async call must be awaited or spawned, `await` is valid only in an `async func` body, and `main` cannot be `async`. `go` takes a call to a declared function or method; its inputs are copied or moved into an owning closure that the runtime executes once, and spawned `mut`, view, and unmoved-Move inputs are rejected. `Task<...>` is a Move handle: dropping it detaches the task, `.wait()` and `await task` consume it, a task panic is raised again at retrieval, and a nil task panics. Text counts are shared across threads under one lock. Q32 adds a poll scheduler, persistent heap frames, async-call/task, channel/select, timer, mutex acquisition, and I/O/helper suspension, and polled `go` for eligible async functions. Waiting standard-library wrappers gain internal poll frames; plain user helpers remain synchronous. Plain-function tasks run once on the same worker pool as polled tasks on every host; blocking runtime calls start replacement workers. Async tasks need heap frames rather than private stacks, while plain functions hold an OS worker during a wait. Fibers, stack-switching assembly, and the per-task thread fallback are removed. `go` also takes a closure literal or a function-typed local as its callee (Q33): the callable moves into the task, runs once as a plain task, and is destroyed exactly once on a normal return, an error, or a panic; borrowed parameters, views, `mut` captures, and changes to a captured Copy value are rejected. Method values (`value.Method`) are closures over the receiver (Q34). Declared `async func`s are values of an `async func(...)` type (Q36): a call through one is awaited or spawned, and an await suspends the task like a declared call. Remaining: preemption, async closure literals and async method values, and the lifetime proof for borrows that cross a task boundary beyond the current rejections. Channels are M30–M31. |
| M30–M31 — channels and async I/O implemented | `channel<T>()` and `channel<T>(n)` with `send`, `receive`, and `close` work end to end (§19, Q26): handles are Copy and share one queue, a Move message is moved in and out, an unbuffered send waits for a receiver, a buffered send waits only when full, `receive` returns the value and whether one arrived, closing wakes every blocked task, a send on a closed channel panics (the runtime drops the unsent value first), closing twice panics, and the zero value is an always-closed empty channel. Buffered values are dropped when the last handle goes. Elements cannot hold slices or function values. An async task suspends on a channel; a plain task blocks its worker with compensation. Async I/O (§37.3, Q27): `zore/time` (`Sleep`, `Millis`), `zore/io` (`ReadLine`), `zore/os` (`ReadFile`, `WriteFile`), and `zore/net` (TCP `Listen`, `Dial`, `Accept`, `Read`, `Write`, `CloseWrite`, `Port`) suspend an async task or block a plain caller with compensation. Timers and socket readiness use an event loop (epoll on Linux, kqueue on macOS); files, standard input, connects, and name lookups run on a pool of helper threads. A program in which every task waits on a channel or another task, with no timer, descriptor, or helper thread that could wake one, stops with "all tasks are asleep" and exit status 2. `select` (§19.14, Q29) waits on several `receive` and `send` cases and runs the body of one that can proceed, with an optional `default`; ready cases are tried from a rotating start so none starves, and a task waiting in a `select` sits in the queue of every channel it names until one case completes. Byte arrays (§37.2–37.3, Q30): `strings.Bytes` and `strings.FromBytes` (checked UTF-8), `os.ReadBytes` and `os.WriteBytes`, and `Conn.ReadBytes` and `Conn.WriteBytes` read and write any bytes as an `Array<byte>`. Time limits and cancellation (§37.3–37.4, Q31): `time.After` is a channel that fires once, `net.DialTimeout` and `SetTimeout` on a `Listener` or `Conn` make accepts, reads, and writes give up with a `timed out` error (the event loop waits on the descriptor and a timer together), and the new `zore/cancel` package gives cooperative tokens (`New`, `WithTimeout`, `Cancel`, `Cancelled`, `Done`, `Child`, `Sleep`) written in Zore itself. Remaining: UDP, TLS, other file operations, and sharing one connection between tasks. The kqueue path runs in CI on macOS. |
| M32–M34 — in progress | LLVM/native toolchain hardening and library growth; native execution tests exist, but a full MVP coverage audit against §39/§46/§52 is pending. Standard library refactor (§37.2–§37.5, Q37): one naming and signature pattern across packages, nested import paths, byte-based files, input, and connections, nanosecond durations, network deadlines, `zore/context` in place of `zore/cancel`, `bufio` in place of `zore/io`, and new `bytes`, `errors`, `unicode`, `unicode/utf8`, `path`, `path/filepath`, `sort`, `sync`, and `os/exec` packages. `panic(message)` is callable from source (§15.4, Q38). Runes convert to and from integer types (§6.6, Q39). Named types such as `type Duration int` declare distinct types with methods (§8.5, Q40). Package-level `let` values are computed once before `main` (§3.21, Q41). Interfaces: borrowed and owned interface values, implicit conversions, and calls through method tables that suspend in async code when the method behind them waits (§22.2, Q42). Generic functions with `any`, `copyable`, `comparable`, `ordered`, and interface constraints, inferred type arguments, and one compiled copy per set of type arguments (§22.1, Q42); generic types are next. |
| M35+ | Self-hosting work after bootstrap and library capabilities are sufficient. Stages 1 and 2 of the self-hosting plan are done: a lexer and a parser written in Zore (`compiler-zore/`) match the Rust frontend in differential tests. |

The early native milestones and semantic milestones overlap: integrate the
resolution, type checks, and lowering required by a program before calling it
supported. Never bypass ownership rules just to make a demonstration execute.

The compiler lives in `compiler/`, alongside the `runtime/` Rust workspace
member, with stage folders and explicitly registered subsystem tests (see
`architecture.md`).
The layout refactor does not advance language milestones. The Rust runtime
migration has native regression coverage on Ubuntu and macOS in the CI run
linked above.

## Current implementation and next work

Q32 slices 1–6 are implemented. Plain-function tasks use the shared pool, fibers
and stack switching are removed, and the architecture and test guides describe
the current scheduler. Async-call/task, channel/select, timer, mutex acquisition,
and I/O/helper waits use persistent frames and task wakers.
Plain helpers and callbacks remain synchronous; blocking runtime calls compensate
workers. Internal waits retain deadlock detection; external waits do not count.
Cancellation's private timer and parent-following tasks use async frames.

The large task tests run 50,000 nonblocking computations in plain functions;
the 5,000-task join chain and thousands of channel and timer waiters use async
functions. Plain tasks that wait hold OS workers, so large sets of them can
exhaust host thread limits; use `async func` for those workloads. Compiler-inserted
cooperative budgets now yield CPU-heavy async loops. Still open: forced
preemption; frame shrinking by general liveness (poll-local storage and same-type
slot reuse are implemented); scoped tasks (unresolved design work, Q10); and
async closure literals. These are follow-ups outside the accepted Q32 slices. The next milestone is M32–M34's backend/toolchain
hardening and MVP coverage audit.

The supported baseline has native validation (see above). The detailed work
packages below retain historical decisions and tests; their status must be
read against the current milestone table and linked CI evidence.

The first Move type is a struct with a user-defined `drop` method (§8.3,
§14.3). MIR ownership analysis checks whole-place moves, borrowed parameters,
branch joins, and loop backedges, and tracks field-level partial moves: a
moved field's siblings stay individually usable, the whole value is unusable
until every moved field is reinitialized, and a move that would reach through
a custom-`drop`-bearing container is rejected (§31.2, Q07a/Q07b). Drop
insertion handles scope exits, replacements, owned parameters, and
synchronous panic cleanup; codegen's existing per-field drop-flag tracking
already drops only still-live fields with no further changes needed.
Synchronous `?` under M18–M19 now uses the return cleanup path.

Borrowed slices and region analysis now close the stored-borrows part of
M13–M17 for `zore check` (§11.3–11.7, §12, Q16). A loan comes from slicing,
from copying a `mut []T` (an exclusive reborrow), or from a call whose
inferred return contract says its result borrows from an argument; loans
cover storage paths that distinguish a descriptor from the storage it views.
Backward liveness frees a loan at its holder's last use, so a backing owner
is usable again afterwards. Shared parameters whose type contains a fixed
array are now passed by reference, so a callee can return a view of the
caller's array; a later argument that mutates an earlier by-reference
argument is rejected. Native code generation represents a view as a
`{ ptr, i64 }` descriptor; indexing a slice loads its data pointer mid-path,
and slicing checks `0 <= low <= high <= length` with unsigned comparisons
after widening each bound per its own signedness, so negative signed and huge
unsigned bounds both panic.

The first M18–M19 slices reject non-final and repeated `error` results, then
support explicit error values, returns, comparisons, and discards. Named local
`error` bindings and parameters are checked for use on every reachable path
before overwrite or normal scope exit. Synchronous `?` propagates from calls
and fills non-error return positions with zero values on failure. Awaited `?`
depends on the M25–M29 task model.

Fixed-array types `[T; N]`, typed literals, and indexing are implemented end
to end (§12.6): the `Place`/`FieldId` projection model generalized to a
`Projection` enum (`Field`/`Index`) across HIR and MIR, reusing the
partial-move `MovedSet` unchanged by truncating queries to their leading
field-only prefix, since array contents are not partial-move-tracked past the
array field itself. Moving a single element out through an index is
unconditionally rejected for now (diagnosed, not guessed); only the
constant-index carve-out and borrowed slices extend this later. Conservative
aliasing treats any two runtime indices as potentially overlapping, per spec.
A statically out-of-range constant index is rejected at check time; a
non-constant index gets a runtime bounds check (a new `Rvalue::BoundsCheck`,
reusing the existing `Terminator::Assert`/`panic_if` mechanism) that widens
the index per its own signedness before comparing — unsigned sources must
compare unsigned (`icmp uge`), never signed, or a huge unsigned index wraps
to a negative `int64` bit pattern and silently passes an `sge` check.
Codegen emits LLVM's native `[N x T]` array type and mixes literal `i32`
struct-field indices with a runtime `i64` array index in one `getelementptr`
instruction. Per-value `DropFlags` cleanup extends to arrays *without*
per-element flags — sound only because element extraction stays rejected, so
an array's contents are always either fully present or (the whole array
moved) irrelevant; a custom-`drop` call made on an element during cleanup
gets a fresh transient scratch flags block (`drop_value`/`drop_contents`/
`drop_unconditional`/`scratch_flags`) rather than nonexistent persistent
per-slot storage, which a naive copy-paste extension of the existing
struct-field GEP pattern would have corrupted.

Other open items: the `println` float text format (§37.1, TBD).
Integer-to-rune conversions are done (§6.6, Q39). Strings follow below. Runtime checks use MIR assert
terminators with cleanup paths.

Temporary limits that are not language rules: package-level `let`/`var` await
Q05, a project can import only its own packages and the standard ones, and the
standard library is only `zore/strings` and `zore/strconv` (floats and I/O are
not covered).

Routine driver, diagnostic presentation, and internal representation decisions
can be made during implementation and documented with tests. They do not require
new source-language proposals. New semantic gaps go in the question register;
isolate the affected feature and continue unrelated supported work.

## Decisions deferred to their consuming stage

| Decision | Resolve when needed |
| --- | --- |
| Remaining primary/postfix grammar | Before implementing the affected parser form; the M3–M4 subset is parsed and later forms are diagnosed as unsupported |
| Type identity/layout gaps, numeric typing corner cases | Before the corresponding resolver/type-checker accepts those programs; not before lexing |
| Minimal package/entry-point contract and `println` signature | Resolved in §3.19 and §37.1 (Q05a); `println` float text format stays open until native float printing |
| Import discovery, project mapping | Resolved in §3.20 (Q24); implemented. Package variables and initialization order remain open |
| String indexing/slicing/length | Resolved in §6.8 (Q23); implemented |
| Borrowed map-entry access, capacity APIs | Before implementing those operations; iteration, `len`, `push`, and `pop` are resolved in §5.10 and §12.7 |
| Closure types/captures/invocation | Resolved (§16, Q02g, Q02i); closures with tasks resolved in Q33 |
| Full go grammar, concurrency library APIs | Before affected M25–M31 work; preserve already locked ownership/runtime contracts |
| Scoped tasks (Q10) | Optional extension, not a current implementation prerequisite |
| LLVM version/ABI, allocation, scheduler | Internal choices when the relevant backend/runtime stage begins; document and test then |

Deferral does not mean exclusion from the MVP. Unsupported features must receive
honest diagnostics, and full MVP completion still requires their implementation.

## Semantic checkpoint

`examples/semantic-target/main.ore` is copied from §42. Acceptance requires a
successful semantic check and eventual native execution printing `John`, with
spans and diagnostics retained throughout. A parser-only pass is insufficient.
Semantic checks are covered by `tests/typecheck/check.rs` and
`tests/driver/cli.rs`; native execution is covered by `tests/codegen/native.rs`.
These suites run in the CI configuration linked above. Separate Move-resource
regressions test that ordinary calls borrow rather than consume values; the
semantic target's `User` contains only a Copy string.

## Completion policy

Mark milestones complete only with working behavior and relevant automated tests.
Track temporary restrictions and their spec basis. Do not count pending fixtures
as passing tests. Full MVP includes async, tasks, channels, runtime, native output,
and diagnostics; completing the synchronous subset does not complete the MVP.

## Detailed implementation plan

This section carries the former implementation plan in the same file as the
current milestone status. The Phase 0–43 descriptions include historical
starting targets and future acceptance criteria; they do not claim current
completion. Their numbers are workstream references, not M IDs. Apply the
specification and the current status table above when any example or older
phase instruction differs from a locked language rule or implemented scope.

### Agent execution rules

Coding agents MUST follow these rules.

1. Read `spec/language-spec.md` before implementing language semantics.
2. Read `compiler-structure.md` before creating or moving compiler modules.
3. Use the milestone table above for canonical IDs and the active work package below for current priorities.
   Phase numbers organize detailed work; they are not a second milestone numbering system.
4. Do not redesign locked syntax or semantics for convenience.
5. Do not add unspecified language features.
6. Do not create all future modules as empty scaffolding.
7. Every milestone must include tests.
8. Every semantic error must use the structured diagnostic subsystem.
9. Preserve source spans through all relevant compiler phases.
10. Keep `zore check` independent from LLVM/code generation.
11. Prefer a correct conservative compiler rejection over unsafe inference.
12. Do not begin full async/concurrency implementation until synchronous ownership semantics are reliable.

If an unresolved design question blocks implementation:

```text
STOP language design expansion
        ↓
document the exact unresolved question
        ↓
use the smallest temporary internal behavior if possible
        ↓
do not silently create new Zore syntax
```

---

### Definition of done

A milestone is complete only when:

- implementation compiles cleanly
- relevant unit tests pass
- relevant integration tests pass
- diagnostics are tested where applicable
- no previous milestone behavior regresses
- module boundaries still conform to `compiler-structure.md`
- no locked rule in `spec/language-spec.md` is violated
- examples for the milestone compile or fail as expected

Do not mark a milestone complete based only on “code exists.”

---

### Historical implementation baseline — 2026-09-28

Inspected commit: `39dde45` (folder refactor and Rust runtime migration);
CI-validated at `95110b1`. The table below records the implementation on that
date, before Q32 and later language work. Use the current milestone table above
for present status. CI passing establishes that existing tests pass, not that
any milestone is complete beyond the subset it covers.

| Canonical milestone | Status at 2026-09-28 |
| --- | --- |
| M0–M1 | Workspace, CLI, source manager, spans, labels, notes, and rendering exist. File-load errors are plain CLI messages; diagnostic codes are absent. |
| M2–M4 | Lexer and AST/parser implement the current subset. Async declarations have syntax representation but are rejected semantically; collections, indexing, and closures remain unsupported. |
| M5–M8 | Hello, variables, functions, structs, control flow, and multiple returns have implementations and native tests for the synchronous all-Copy subset. Methods with shared or `own` receivers resolve, type-check, and run natively; `mut` parameters and receivers require mutable places (§11.6) and are passed by reference. |
| M9–M10 | Resolution across packages, stable IDs, primitive/struct types, type checking, and exact constant evaluation exist. Package variables remain unsupported. |
| M11–M12 | Typed HIR, CFG MIR, and local/field places exist. MIR distinguishes Copy/Move operands; indexed places are absent. |
| M13–M17 | Recursive Copy classification, mutable-place checks, call-local exclusivity, and validated user-defined `drop` methods exist. The test-only entry point runs whole-place MIR move analysis and supports builtin `drop(value)`; field moves are rejected conservatively. Move values remain rejected by `zore check` and `zore build` until cleanup lands. Stored borrows and region analysis remain absent. Slice C passed Ubuntu and macOS CI on PR #6. |
| M18–M19 | No destruction/drop insertion or explicit error/propagation implementation. Multiple returns alone do not complete error handling. |
| M20–M24 | Collections, multi-file packages/imports, and closures remain unsupported. |
| M25–M31 | No task model, spawning, async lowering, scheduler, channels, or async I/O. |
| M32–M33 | An early native foundation exists: LLVM IR, clang object generation, rustc linking, and a Rust runtime for output, string comparison, and initial-task panic. Hardening and migration validation remain pending. |
| M34–M35+ | Standard-library growth and self-hosting remain future work. |

Evidence entry points:

- `compiler/src/driver/`, `source/`, and `diagnostic/`: commands, pipelines,
  source identity, and structured source diagnostics.
- `compiler/src/parser/mod.rs` and `resolve/mod.rs`: syntax support and
  explicit unsupported-feature diagnostics.
- `compiler/src/types/` and `hir/mod.rs`: supported types, constants, and
  the current recursive all-Copy classification.
- `compiler/src/mir/` and `codegen/`: places, executable control flow,
  Copy/Move operand variants, and LLVM emission.
- `runtime/src/`: the implemented Rust runtime. Exported ABI functions live
  in their implementation modules; a dedicated `abi.rs` is not yet present.
- `tests/` subsystem suites and runtime unit tests: existing executable test
  definitions. `tests/conformance/` also contains pending specification cases.

The current implementation uses cohesive `mod.rs` files in several stage
folders rather than every smaller file shown in the target structure. Keep
real phase boundaries; split files when their responsibilities justify it,
without generating empty modules.

### Phase 0 — Repository and Compiler Skeleton

#### Goal

Create the minimum viable Rust workspace and compiler entry point.

#### Required structure

```text
zore/
├── Cargo.toml
├── README.md
├── spec/
│   └── language-spec.md
├── compiler-structure.md
│
├── compiler/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── driver/
│       ├── source/
│       └── diagnostic/
│
└── examples/
    └── hello/
        └── main.ore
```

#### Deliverables

- Rust workspace
- `compiler` crate
- `zore` executable
- compiler version output
- basic command dispatch
- source-file argument parsing
- placeholder `check` command
- source manager skeleton
- diagnostic framework skeleton

#### CLI target

```bash
zore --version
zore check examples/hello/main.ore
```

`check` may initially only load the file successfully.

#### Acceptance criteria

```text
zore --version
```

prints a valid compiler version.

```text
zore check missing.ore
```

reports the missing file and path as a CLI load error. A source-aware
structured diagnostic can be added later when a source file exists.

#### Do not implement yet

- lexer
- parser
- type checker
- ownership
- LLVM

---

### Phase 1 — Source Manager and Spans

#### Goal

Build reliable source tracking before compiler logic grows.

#### Deliverables

Implement:

```text
SourceFile
SourceMap
FileId
Span
line/column lookup
source slicing
```

Recommended:

```rust
pub struct FileId(pub u32);

pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}
```

The exact representation may vary, but spans must uniquely identify a location within a file.

#### Required behavior

Given a byte offset, the compiler can determine:

- file
- line
- column
- source snippet

#### Tests

Test:

- ASCII
- UTF-8 source
- multiline source
- empty file
- final line without newline
- invalid span protection

#### Acceptance criteria

Diagnostic rendering can underline the exact source region.

---

### Phase 2 — Structured Diagnostics

#### Goal

Create diagnostics before semantic compiler errors begin.

#### Deliverables

Implement concepts equivalent to:

```text
Diagnostic
Severity
DiagnosticCode
Label
Note
DiagnosticRenderer
```

Example target:

```text
error[E0001]: unexpected character

  --> main.ore:3:5
   |
3  |     @
   |     ^ unexpected character
```

#### Rules

Compiler phases should create structured diagnostics.

Avoid final architecture such as:

```rust
Err("parser error".to_string())
```

#### Tests

Snapshot/golden tests for:

- one primary label
- multiple labels
- notes
- multiline ranges
- UTF-8 positioning

---

### Phase 3 — Lexer

#### Goal

Convert Zore source into a token stream.

#### Required module

```text
compiler/src/lexer/
├── mod.rs
├── lexer.rs
├── token.rs
└── token_kind.rs
```

#### Required token categories

Implement tokens required by currently locked MVP syntax, including:

- identifiers
- integer literals
- floating literals
- string literals
- rune literals when specified
- keywords
- punctuation
- operators
- EOF

Keywords should include at least:

```text
package
import
func
type
struct
let
var
const
mut
own
async
await
go
return
if
else
for
true
false
```

#### Responsibilities

Lexer handles:

- whitespace
- comments
- identifiers
- literals
- operators
- delimiters
- lexical diagnostics

Lexer must NOT perform semantic checks.

#### Tests

At minimum:

```text
lexer_identifiers
lexer_keywords
lexer_numbers
lexer_strings
lexer_operators
lexer_comments
lexer_utf8
lexer_invalid_character
lexer_unterminated_string
```

#### Acceptance criteria

This source:

```ore
package main

func main() {
    let value = 42
}
```

produces the expected token sequence with correct spans.

---

### Phase 4 — Parser Core and AST

#### Goal

Parse basic Zore source into an AST.

#### Required modules

```text
parser/
ast/
```

#### First syntax subset

Implement:

- package declaration
- function declarations
- parameter lists
- blocks
- `let`
- `var`
- `return`
- function calls
- identifiers
- primitive literals

Do not implement all language features at once.

#### AST rule

AST describes what the programmer wrote.

Do not attach:

- move state
- borrow state
- LLVM types
- async state-machine fields

to AST nodes.

#### First parser target

```ore
package main

func main() {
    println("Hello, Zore!")
}
```

#### Acceptance criteria

`zore check` can:

1. load source
2. lex source
3. parse AST
4. report syntax errors
5. succeed for syntactically valid input

Semantic name resolution is not required yet.

---

### Phase 5 — Parser Expansion

#### Goal

Expand parsing to the synchronous core language.

#### Add syntax for

- structs
- struct fields
- struct literals
- methods
- field access
- assignment
- arithmetic expressions
- comparison expressions
- logical expressions
- `if`
- `else`
- basic `for` forms only after their grammar is explicitly locked
- arrays
- slices
- maps
- multiple return types
- `?`
- `drop(...)`
- `clone(...)`
- closures

#### Important

Where grammar is marked TBD in `spec/language-spec.md`, do not invent final syntax.

Implement only grammar that is explicitly locked.

---

### Phase 6 — Name Resolution

#### Goal

Resolve source names to stable compiler identities.

#### Required module

```text
resolve/
```

#### Deliverables

Implement:

```text
Scope
Symbol
SymbolTable
FunctionId
StructId
FieldId
LocalId
```

Resolve:

- local variables
- functions
- structs
- fields
- methods
- package-level declarations

#### Rule

After resolution, semantic passes should not repeatedly resolve source strings.

Example:

```text
"user"       → LocalId(4)
"loadUser"   → FunctionId(7)
"User"       → StructId(3)
```

#### Errors

Support diagnostics for:

- unknown identifier
- duplicate declaration
- duplicate parameter
- duplicate field
- invalid field lookup

#### Acceptance criteria

Resolver successfully resolves the first semantic target program.

---

### Phase 7 — Type System Foundation

#### Goal

Build compiler type identities and checking.

#### Required module

```text
types/
```

#### Deliverables

Implement:

```text
Type
TypeId
TypeStore
FunctionType
```

Initial types:

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
struct types
function types
error
```

Add collection/core types as their milestone arrives.

#### Required checks

- variable initializer type
- assignment compatibility
- function argument count
- function argument types
- return type
- field types
- struct literal fields
- expression operands

#### Acceptance criteria

Compiler rejects obvious type mismatches with source-aware diagnostics.

---

### Phase 8 — Copy / Move Type Classification

#### Goal

Implement type-level ownership classification.

#### Deliverables

Every relevant type can be classified as:

```text
Copy
Move
```

Rules from `spec/language-spec.md`:

- primitive value types → Copy
- `string` → Copy from programmer perspective
- resource-owning values → Move
- `Array<T>` → Move
- maps → Move
- struct → Copy only when all fields are Copy
- struct → Move when any field is Move
- struct with a custom `drop` method → always Move, even with all-Copy fields
  (spec §8.3 and §14.3); this overrides field-derived classification

#### Important

Do not yet confuse this with whether a specific local has been moved.

Type classification is static type metadata.

Value move-state comes later.

#### Tests

Include nested structs.

---

### Phase 9 — HIR

#### Goal

Introduce a semantic representation between AST and MIR.

#### Required module

```text
hir/
```

#### HIR should contain

- resolved IDs
- resolved types
- normalized expression structure
- semantic function/method references
- parameter ownership contracts

#### Rule

HIR is not a duplicate AST.

AST:

```text
what the programmer wrote
```

HIR:

```text
what the compiler understands it to mean
```

#### Acceptance criteria

The synchronous core program can lower from AST to typed HIR.

---

### Phase 10 — MIR and Control-Flow Graph

#### Goal

Introduce explicit executable control flow.

#### Required module

```text
mir/
```

#### Required concepts

```text
MirBody
BasicBlock
Statement
Terminator
Operand
Rvalue
Place
Projection
```

#### MIR operands

Must distinguish:

```text
Copy(place)
Move(place)
Constant(...)
```

#### Basic control flow

Support:

- assignment
- calls
- return
- goto
- branch
- unreachable

Do not add runtime-specific LLVM details to MIR.

#### Acceptance criteria

A simple function can lower into stable basic blocks.

---

### Phase 11 — Place Model

#### Goal

Create the storage abstraction used by ownership checking.

#### Required concepts

```text
Place
Projection
```

Examples:

```text
user
user.Name
items[index]
```

Possible conceptual form:

```text
Place {
    local: LocalId,
    projections: [...]
}
```

#### Why this milestone matters

Ownership must eventually reason about:

- whole variables
- fields
- indexed values
- partial moves

Do not implement ownership using only variable names.

---

### Phase 12 — Move Analysis

#### Goal

Implement runtime/data-flow state for Move values.

#### Required states

The exact enum may evolve, but the compiler must be capable of representing:

```text
Available
Moved
PartiallyMoved
```

#### Required errors

Reject:

```ore
let connection = openConnection()
let other = connection

use(connection)
```

when `Connection` is Move.

#### Required success

Copy types remain usable:

```ore
let a = 10
let b = a

println(a)
println(b)
```

#### Acceptance criteria

Use-after-move produces a structured diagnostic showing:

- value creation
- move location
- invalid use

---

### Phase 13 — Shared Borrowing

#### Goal

Implement borrow-by-default function semantics.

#### Example

```ore
func read(user User) {
    println(user.Name)
}

read(user)
println(user.Name)
```

`read(user)` must not consume `user`.

#### Deliverables

Internal representation for:

```text
BorrowId
Borrow
BorrowKind::Shared
RegionId
```

#### Required checks

- borrow validity
- no move while actively borrowed

---

### Phase 14 — Mutable Borrowing

#### Goal

Implement:

```ore
func update(user mut User)
```

#### Required semantics

Mutable borrow requires exclusive access.

Reject overlapping:

- mutable + mutable
- mutable + conflicting shared borrow
- move during mutable borrow

#### Diagnostics

Errors should identify both:

- original active borrow
- conflicting operation

---

### Phase 15 — Lifetime / Region Analysis

#### Goal

Infer borrow validity without exposing source-level lifetimes.

#### Requirements

No Rust-style lifetime syntax.

Compiler-internal only:

```text
RegionId
borrow start
borrow last use/end
CFG relationships
```

#### Implementation direction

Prefer:

- control-flow-aware data flow
- last-use information
- local analysis
- conservative rejection when proof is unavailable

Do not require global explicit lifetime annotations.

---

### Phase 16 — Drop Analysis and Insertion

#### Goal

Implement deterministic resource cleanup.

#### Required module

```text
dropck/
```

#### Required behavior

Insert explicit cleanup for owned Move values when their lifetime ends.

Must handle:

- normal return
- branch exits
- early return
- moved values
- explicit `drop(value)`

#### Explicit drop

After:

```ore
drop(connection)
```

the value is consumed.

Any later use is an error.

#### Acceptance criteria

No double drop.

Moved values are not destroyed by their previous owner.

---

### Phase 17 — Error Type and Multiple Returns

#### Goal

Implement Zore's explicit error model.

#### Required support

```ore
func readFile(path string) (string, error)
```

Implement:

- multiple return types
- multiple return values
- `error`
- normal explicit error propagation

Do not introduce hidden exceptions.

---

### Phase 18 — `?` Error Propagation

#### Goal

Implement:

```ore
let file = open(path)?
```

#### Required semantics

If the operation returns an error:

1. identify values whose lifetime ends
2. insert required cleanup
3. return the error

#### Important

`?` and deterministic drop must be designed together.

#### Tests

Test:

- no resources live
- one resource live
- multiple resources live
- branch before `?`
- nested calls with `?`

---

### Phase 19 — Arrays and Slices

#### Goal

Implement fixed arrays and borrowed slices.

#### Required types

```text
[T; N]
[]T
mut []T
```

#### Required semantics

Slices are borrowed views.

Copying a slice copies the descriptor, not the backing elements.

Borrow checking must validate slice lifetimes.

---

### Phase 20 — Dynamic Arrays

#### Goal

Implement:

```text
Array<T>
```

as an owned Move collection.

#### Runtime requirements

Likely requires:

- allocation
- growth
- destruction
- element cleanup

Do not add general user-defined generics just to implement `Array<T>`.

It may initially be compiler/core-library special handling.

---

### Phase 21 — Maps

#### Goal

Implement:

```text
map[K]V
```

#### Semantics

Map is Move.

Normal parameter:

```ore
func read(m map[string]User)
```

borrows the map.

```ore
func update(m mut map[string]User)
```

mutably borrows.

```ore
func consume(m own map[string]User)
```

moves ownership.

---

### Phase 22 — Packages and Imports

#### Goal

Support multi-file packages and basic imports.

#### Required support

```ore
package main
import "zore/fmt"
```

#### Required compiler work

- package loading
- package symbol table
- file aggregation
- exported-name rules
- import resolution

Visibility:

```text
Uppercase → exported
lowercase → package-private
```

#### Out of scope

- package registry
- dependency solver
- complex version selection

---

### Phase 23 — Methods

#### Goal

Complete method semantics.

Support:

```ore
func (user User) greet()
func (user mut User) rename(name string)
func (user own User) save()
```

Receiver ownership must reuse ordinary parameter ownership rules.

No separate method ownership model.

---

### Phase 24 — Closures

#### Goal

Implement minimal closures.

#### Required syntax

```ore
let name = "Maas"

let greet = func() {
    println(name)
}
```

#### Capture analysis

Infer:

- Copy capture
- Move capture
- borrow capture

Compiler may initially reject complicated captures conservatively.

This milestone is important before tasks because spawned closures require capture ownership.

---

### Phase 25 — Runtime ABI Boundary

#### Goal

Define a stable boundary between generated Zore code and the Rust runtime.

#### Add

```text
runtime/src/abi.rs
```

#### Runtime exports

Use stable externally callable symbols for operations such as:

```text
zore_alloc
zore_dealloc
zore_panic
zore_task_*
zore_channel_*
zore_io_*
```

Exact function names may evolve before stabilization.

#### Important

Keep runtime ABI separate from compiler-internal Rust APIs.

Generated code must not depend on Rust name mangling.

---

### Phase 26 — LLVM Backend Foundation

#### Goal

Compile the synchronous subset into native code.

#### Required module

```text
codegen/
```

#### First targets

- primitive types
- local variables
- arithmetic
- function calls
- structs
- returns
- branches
- runtime calls

#### Architecture

```text
Final MIR
   ↓
LLVM IR
   ↓
object file
   ↓
link runtime
   ↓
native executable
```

#### Acceptance target

```ore
package main

func main() {
    println("Hello, Zore!")
}
```

produces a native executable.

---

### Phase 27 — Task Model

#### Goal

Introduce the runtime/compiler concept of concurrent work before full async lowering.

#### Required concepts

```text
Task
Task handle
spawn
wait
detach
```

#### Semantics

Dropping a task handle detaches.

It does not implicitly cancel running work.

---

### Phase 28 — `go`

#### Goal

Implement:

```ore
let task = go calculate()
```

and:

```ore
go process(user)
```

#### Ownership rules

- Copy values copied into task
- Move values transferred where required
- borrowed values only allowed when lifetime is proven
- mutable borrow gives exclusive access for its valid task lifetime

#### Acceptance criteria

Compiler rejects unsafe captured borrows.

---

### Phase 29 — Async Function Representation

#### Goal

Add compiler representation for:

```ore
async func ...
```

Do not yet focus on runtime optimization.

#### Required compiler capability

Identify:

- async functions
- suspension points
- values live across suspension

---

### Phase 30 — Async State-Machine Lowering

#### Goal

Lower async control flow into state machines.

#### Required module

```text
async_lowering/
```

#### Requirements

A local needed after `await` becomes part of persistent async state.

Owned values must remain safely owned.

Borrowed values crossing suspension require proof.

---

### Phase 31 — `await`

#### Goal

Implement suspension/resume semantics.

Example:

```ore
let response = await http.get(url)?
```

#### Required checks

- borrow validity across suspension
- resource liveness
- drop on success
- drop on error
- drop if async state is destroyed before completion, according to final runtime lifecycle rules

---

### Phase 32 — Scheduler

#### Goal

Implement the minimum async task scheduler in the Rust runtime.

#### Required components

```text
task queue
ready/wake mechanism
task state
poll/resume
completion
wait
```

The exact scheduling algorithm is implementation-specific.

Keep it minimal first.

---

### Phase 33 — Channels

#### Goal

Implement Zore channel semantics.

#### Required support

```ore
let ch = channel<User>()
let buffered = channel<User>(10)
```

Operations:

```ore
ch.send(value)
let value, ok = ch.receive()
ch.close()
```

#### Required semantics

- channel handles are Copy
- underlying channel state is shared
- Move values transfer ownership through send
- Copy values are copied
- unbuffered send waits for receiver
- buffered send waits when full
- receive waits when empty/open
- close prevents future sends
- buffered values remain receivable
- drained closed channel eventually returns `ok == false`
- send after close panics

---

### Phase 34 — Async I/O

#### Goal

Add useful I/O primitives integrated with scheduler/async.

Start small.

Potential initial targets:

- file read
- file write
- TCP socket connect/read/write

Do not attempt a huge standard networking library yet.

---

### Phase 35 — Runtime Hardening

#### Goal

Improve runtime correctness after async/channel functionality exists.

Focus on:

- task lifecycle
- wake correctness
- channel synchronization
- resource destruction
- panic boundaries
- allocation errors
- runtime ABI stability

Add stress tests.

---

### Phase 36 — Compiler Diagnostic Hardening

#### Goal

Upgrade semantic diagnostics from merely correct to useful.

Prioritize:

- use-after-move
- borrow conflict
- move while borrowed
- mutable aliasing
- lifetime across `await`
- invalid task capture
- type mismatch
- unknown symbol

Example quality target:

```text
error[E0012]: use of moved value `user`

  --> main.ore:14:13
   |
10 | let user = loadUser()
   |     ---- value created here
12 | save(user)
   |      ---- value moved here
14 | println(user.Name)
   |         ^^^^ used here after move
   |
   = note: `User` is a move type
```

---

### Phase 37 — Standard Library Foundation

#### Goal

Move higher-level facilities into Zore code where practical.

Expected structure:

```text
std/
├── core/
├── io/
├── collections/
└── sync/
```

Keep genuinely low-level operations in runtime.

Long-term direction:

```text
Zore stdlib (.ore)
       ↓
small native runtime
       ↓
OS
```

---

### Phase 38 — CLI Hardening

#### Goal

Provide useful developer workflow.

Target commands:

```bash
zore check .
zore build .
zore run .
zore test .
zore fmt .
```

Do not let formatting/tooling work block compiler correctness.

---

### Phase 39 — Conformance Test Suite

#### Goal

Create a language-level conformance suite independent of internal implementation details.

Group tests by language behavior:

```text
conformance/
├── variables/
├── functions/
├── structs/
├── methods/
├── ownership/
├── borrowing/
├── drop/
├── errors/
├── closures/
├── tasks/
├── async/
└── channels/
```

These tests should survive compiler refactors.

---

### Phase 40 — Self-Hosting Preparation

#### Goal

Ensure Zore itself has enough facilities to implement compiler workloads.

Required ecosystem capabilities include:

- strings
- arrays
- maps
- files
- error handling
- package system
- compiler-friendly data structures
- process/file access
- stable native code generation

Do not begin full compiler rewrite merely because the language can compile a few examples.

`docs/proposals/self-hosting-plan.md` maps current compiler and library
capabilities to bounded stages, beginning with a lexer oracle comparison. It
is a planning proposal, not an implemented compiler or a locked API change.
Stages 1 and 2 are done.

- Stage 1: `compiler-zore/lexer` is a Zore lexer. Its tokens, byte spans,
  decoded literals, and error codes match the Rust lexer
  (`cargo test --test selfhost_lexer`).
- Stage 2: `compiler-zore/parser` adds a parser, plus a source manager and
  diagnostic records with rendering. Its trees, diagnostics, and rendered
  messages match the Rust frontend (`cargo test --test selfhost_parser`).

Name resolution and type checking (stage 3) have not started.

---

### Phase 41 — Zore Compiler Skeleton Written in Zore

Create a separate compiler implementation in Zore.

Suggested future structure:

```text
compiler-zore/
├── main.ore
├── source/
├── lexer/
├── parser/
├── ast/
├── resolve/
├── types/
├── hir/
├── ownership/
├── mir/
└── codegen/
```

Initially compile it using the Rust bootstrap compiler.

---

### Phase 42 — Self-Hosting

Target:

```text
Rust bootstrap compiler
        ↓
Zore compiler written in Zore
        ↓
native Zore compiler
```

Then:

```text
Zore compiler v1
        ↓
Zore compiler source
        ↓
Zore compiler v2
```

Then repeat:

```text
v2 → v3
```

The goal is stable compiler bootstrapping convergence.

---

### Phase 43 — Bootstrap Independence

Only after the self-hosted compiler is reliable should the project consider making the Rust compiler optional for normal development.

Do not delete the Rust bootstrap compiler immediately.

Keep it available for:

- verification
- regression testing
- bootstrapping from scratch
- cross-checking generated behavior

---

### Phase to milestone mapping

Detailed phases map as follows. Cross-cutting work is performed with its
consuming feature; it does not require inventing a competing milestone ID.

| Phase | Workstream | Canonical milestone |
| --- | --- | --- |
| 0 | Repository + CLI skeleton | M0 |
| 1 | Source manager + spans | M1 |
| 2 | Diagnostics | M1; cross-cutting |
| 3 | Lexer | M2 |
| 4 | Parser + basic AST | M3–M4 |
| 5 | Parser expansion | M3–M4; feature-specific milestones |
| 6 | Name resolution | M9 |
| 7 | Type system | M10 |
| 8 | Copy/Move classification | M13–M14 |
| 9 | HIR | M11 |
| 10 | MIR / CFG | M12 |
| 11 | Place model | M12–M17 |
| 12 | Move analysis | M14 |
| 13 | Shared borrowing | M15 |
| 14 | Mutable borrowing | M16 |
| 15 | Lifetime/region inference | M17 |
| 16 | Drop insertion | M18 |
| 17 | Error + multiple returns | M7, M19 |
| 18 | Error propagation | M19 |
| 19 | Arrays/slices | M20 |
| 20 | Dynamic arrays | M21 |
| 21 | Maps | M22 |
| 22 | Packages/imports | M23 |
| 23 | Methods | M8, M13–M18 as needed |
| 24 | Closures | M24 |
| 25 | Runtime ABI | M5, M25–M33 as needed |
| 26 | LLVM backend foundation | M5–M8 foundation; M32–M33 hardening |
| 27 | Task model | M25 |
| 28 | go/spawn | M26 |
| 29 | Async representation | M27 |
| 30 | Async state machine | M27 |
| 31 | await | M28 |
| 32 | Scheduler | M29 |
| 33 | Channels | M30 |
| 34 | Async I/O | M31 |
| 35 | Runtime hardening | M29–M33 |
| 36 | Diagnostics hardening | Cross-cutting; M13–M19, M25–M31 |
| 37 | Standard library | M34 |
| 38 | CLI hardening | M0, M23, M33 |
| 39 | Conformance suite | Every milestone; full MVP audit |
| 40 | Self-hosting preparation | M35+ |
| 41 | Zore compiler skeleton | M35+ |
| 42 | Self-hosting | M35+ |
| 43 | Bootstrap independence | M35+ |

Methods have no separate M ID in §43. Associate basic methods with the struct
workstream and ownership-sensitive receivers/destructors with M13–M18. This
mapping does not imply they are already implemented.

---

### Active work package — validate the committed baseline

#### Scope

Validate the compiler layout and Rust runtime migration at `39dde45` before
starting new language functionality. Do not restart the completed skeleton
implementation or expand ownership while this validation is unresolved.

#### Required environment

Use the repository-pinned Rust toolchain (including rustfmt and Clippy), plus
clang with LLVM 15 or newer on a Linux/macOS host. Native builds invoke rustc
1.98+; `ZORE_CC` and `ZORE_RUSTC` select host-compatible executables.
No compiler or runtime crate requires third-party Rust dependencies.

#### Required checks

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
cargo test --locked --all-targets
```

Also run the existing CI documentation check:
`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked`.

The native suite must execute, including the semantic target printing
`John`, hello, printing boundaries, string ordering, panics, closed stdout,
output paths with spaces, missing-tool diagnostics, and frontend independence.
Verify example loading by running
`cargo test --test parser --test lexer --test check` from the `compiler/`
working directory as well. Fix discovered regressions within this work package
and rerun affected checks. Do not count pending conformance documents as
executed tests.

#### Current result

Validated locally at `bd6ece4` (2026-10-02) with the repository-pinned
toolchain (`rustc`/`cargo` 1.98.1, Apple clang 17 as `ZORE_CC`):
`cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D
warnings`, `cargo build --locked`, `cargo test --locked --all-targets`
(all ten suites, zero failures, including native codegen and runtime
unit tests), and `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked`
all pass with no warnings. `cargo test --test parser --test lexer --test
check` from `compiler/` also passes. This validates the baseline described
above, including the error-propagation work merged through PR #11
(`feat/error-propagation`). The host previously lacked `cargo`/`rustc` on
`PATH`; they are installed under `~/.cargo/bin` and must be added to
`PATH` explicitly in a fresh shell.

#### Next language work after validation

Custom-destructor Move types (M18), mutable-parameter exclusivity, and
whole-place plus field-level partial-move checking (M13–M17) are now
implemented with paired acceptance/rejection cases, as is synchronous `?`
propagation (M19). Fixed-array types, literals, and indexing (M20) are now
implemented end to end, including LLVM codegen (array type emission,
GEP-based indexed addressing, runtime bounds-check panics, and element
cleanup without per-element drop flags — see above). Borrowed slices and
the region analysis they require are implemented end to end (see above).

Dynamic `Array<T>` followed (Q17). It is stored as a `{ ptr, i64 }`
descriptor whose elements live in runtime heap storage
(`runtime/src/alloc.rs`) and are dropped by a reverse-order loop before the
storage is freed. Before it landed, three existing soundness holes were
closed. Borrows that reach storage through a view now keep that view's
backing borrowed. A drop inserted before a replacing store now acts on a
panic only after the store, which removes a double drop of indexed elements.
Scratch drop-flag allocas now go in the entry block, so loops no longer grow
the stack.

Maps (M22) followed (Q18). A map is a pointer to a type-erased runtime hash
table (`runtime/src/map.rs`); null is the empty zero map. Literal entries,
lookup, assignment, and removal are compiler-provided call terminators, so
they reuse argument evaluation order, move checking, unwind edges, and the
post-call panic check. Replacement detaches the old value and drops it before
storing the new one. If that drop panics, the key stays absent and the new
value is cleaned up by its temporary.

Non-escaping closures (M24) followed (§16, Q02g). Each literal is a separate
body whose captures are by-reference locals; creating a closure gives the
closure value shared or exclusive loans on the captured locals, so region
analysis enforces capture exclusivity and lifetimes without new machinery.
Calling a closure, and passing one to a function-typed parameter, use it
exclusively, which rules out reentrant calls through captured state. Codegen
passes a `{ code, environment }` pair; the environment of captured addresses
lives in the creating frame, which is sound because function-typed results,
fields, and elements are rejected.

`clone` (§10.7, Q19) followed for structs, fixed arrays, `Array<T>`, and maps.
A declared custom `clone` is validated at its declaration and called like any
method. The structural and built-in clones are one MIR call, `Callee::Clone`,
that borrows its argument and whose result is an owned value; code generation
clones inline, calling a custom clone for each non-Copy part that has one. After
each such call it checks for a pending panic and, on a panic, drops the parts
already cloned (a struct's earlier fields, an array's earlier elements, a map's
earlier values), frees the new storage, and lets the ordinary unwind path run.
A clone of a value holding views keeps those views' loans, so the clone cannot
outlive the owner. A new runtime function copies a map's keys and allocates
uninitialized value slots for the clone to fill. Valgrind shows no leaks or
invalid accesses for the success and panic paths.

Mutable views in struct fields and fixed arrays followed (Q20). Copying a
value now reborrows every `mut []T` inside it, at that view's field and index
path, so the source's other fields stay usable. Assigning a container ends
reborrows through its old views. A shared parameter cannot hold a nested
mutable view, which keeps shared borrows of containers from granting mutable
access without tracking it.

Destructors that read views followed (Q21). Region liveness now treats the
destruction of such a value as a use of it: at scope end, at `return`, and when
it is replaced. A scope's locals are checked in their real drop order, newest
first, and a value moved out whole stops holding borrows. A value whose `drop`
reads a view must be declared after the storage it views, which covers every
exit, including panic cleanup.

Output provenance followed (Q22). Contracts now record which inputs each
`mut` parameter or closure capture may receive views from, and calls apply them
to their arguments. Calls through function values assume the worst, which also
lets function types return views.

The last region-analysis restrictions are lifted: views can be stored through
mutable slices, and mutable views can live in `Array<T>`, map, and `mut []T`
elements (Q20b, Q22b).

Collection basics followed (§5.10, §12.7): `len` on every collection, `push`
and `pop` on `Array<T>`, and `for … in` loops over fixed arrays, slices,
`Array<T>`, and maps. A loop is a counting loop over a shared borrow of the
collection that stays live for the whole loop; the item is a by-reference
local rebound on each iteration.

Packages and imports followed (§3.20, Q24). A loader (`driver/project.rs`)
reads the entry file's folder, finds `zore.toml`, parses every file, and
follows imports depth first: `"project/dir"` maps to a folder below the root and
`"zore/name"` to a bundled package. It rejects bad paths, cycles, mismatched
package clauses, and importing `main`, and returns packages dependencies first.
The resolver then declares every package into one set of IDs, with a scope per
package and an import table per file, and checks each qualified use against the
export rule; unused and clashing imports are reported there. The checker
applies the same rule to fields and methods, and symbol names carry the
package path so equal names in different packages never collide.

The standard packages followed (§37.2). `zore/strings` and `zore/strconv` are
Zore source bundled in the compiler, with each function declared without a
body; the parser accepts that form only for bundled sources. Their code lives
in the runtime: for each bodyless function the code generator emits a shim that
unpacks strings and slices into pointer-and-length arguments, passes an out
pointer for strings, `Array<string>` results, and `(T, error)` results, and
calls `zore_native_<package>_<function>`. Split pieces and trimmed or joined
single parts share their source's storage.

Strings followed (§6.8, Q23). `len`, `s[i]` (a `byte`), `s[a:b]`, `for … in`
over characters, runtime `+`, and `string(rune)` all work. A slice is a value
of type `string` with no loan, because strings are immutable and Copy; its
bounds and character boundaries are checked at run time by a MIR assert. Text
built at run time is allocated by the runtime in reference-counted buffers: the
compiler retains a `string` when a copy is kept and releases it when its owner
is dropped, overwritten, or unwound by a panic, and the last release frees the
buffer. `a + b` appends in place when `a` ends where its buffer's text
ends, and doubles the buffer when it is full, so a loop that builds one text
uses memory proportional to its length. Setting `ZORE_CHECK_LEAKS` makes a
program exit with code 70 if any text is still owned when it ends; the test
suite sets it. Loops decode characters from a held copy of the
string with `StringChar` and `StringAdvance`.

Owning and call-once closures followed (§16.4, §16.6, Q02i). After a body is
checked, a pass marks closure literals in escaping positions owning, follows
`let g = f` rebinding, and checks that call-once closures are only called
directly; a direct call consumes the closure through an explicit `drop`. A
closure value is now `{ code, environment, destructor }`: an owning closure's
environment is heap storage holding the capture pointers, the captured values,
and their drop flags, so the closure body is the same either way.

Async functions followed (§17.2, §17.8). The resolver declares an `async func`
like any other function. The checker allows an async call only as the operand
of `await`, requires `await` to sit in an `async func` body, and gives an
awaited call the callee's result list, so `await f()?` and discards behave as
for a synchronous call. Q32 now lowers async functions to persistent frames after drop insertion;
awaited calls poll child frames and preserve the original cleanup edges.

Tasks followed (§18, Q25). The checker turns `go f(args)` into a spawn node
that carries the argument expressions and a synthetic closure whose captures
are those arguments and whose body makes the call. MIR evaluates each argument
into a temporary, builds an owning closure around the temporaries (reusing the
closure environment, drop flags, and call-once handling), and spawns it. The
code generator allocates a block holding the closure and room for the results,
and emits an entry function per result list that runs the closure once, keeps
its results unless it panicked, and then runs the environment destructor. The
runtime runs plain entries once on shared pool workers and async entries as
polled persistent frames, then records completion and any panic; `.wait()` waits, takes the results, and
re-raises a panic, and dropping a handle marks the task detached so whoever finishes second destroys its results and
frees the block. Waiting on a task moves its handle, so ownership analysis
needs no new rules. The shared text table is a locked map rather than a
per-thread one.

Q32 replaces the former fiber scheduler with `runtime/src/scheduler.rs`.
The shared FIFO queue retains runnable and suspended tasks, coalesces concurrent
wakes, and installs task-local panic state on each worker. Plain calls block in
`slot.rs` or a task join; a thread-local guard starts replacement workers and
surplus workers retire after calls resume. Async tasks keep all locals and wait
operation records in pinned heap frames. There is no stack-switching assembly or
per-task OS-thread fallback. See `architecture.md` for the completed design.

Channels followed (§19, Q26). `channel<T>` is a pointer to a runtime object
holding a lock, a queue of buffered messages, and lists of blocked senders and
receivers; null is the zero channel, which acts as closed and empty. Handles
are counted like text: copying one retains it and dropping one releases it, in
the same places the compiler already retains and releases strings, and the
last release destroys the buffered messages through a per-type function the
compiler generates. A message is copied into runtime storage on send and out on
receive; the sender's local is marked moved, so no value is dropped twice. A
plain sender or receiver blocks on a `Slot` with worker compensation; an async
caller registers a task waker in the same queue. Wake-before-park is latched. The four
operations are MIR calls with their own `Callee` entries; the channel argument
is passed by reference so no extra handle is made. Setting `ZORE_CHECK_LEAKS`
also fails a program that ends with a live channel while no task runs.

Async I/O followed (§37.3, Q27). The four packages are bundled Zore sources
whose functions are declared without bodies and call into the runtime through
generated shims; two new shim shapes carry `error` and `(string, error)`
results. `Conn` and `Listener` are structs holding a number, with a custom
`drop` that closes the handle, so they are Move and close when dropped. The
runtime keeps a table of handles that are never reused. A task that must wait
registers a one-shot interest with the event thread and waits through a slot or
task waker; a
nonblocking operation that reports "would block" waits and tries again. The
event thread owns one epoll or kqueue queue plus a wake-up pipe and a heap of
timers. Blocking calls run on helper threads that grow to 512 and exit after
five idle seconds. A connection keeps up to three bytes of an unfinished
character between reads so text is never split. On targets without an event
queue socket waits run on helpers for async callers, with compensated blocking
for plain callers.

Deadlock detection (`runtime/src/deadlock.rs`). A task that waits on a channel
or on another task is counted as blocked when it goes to sleep and uncounted by
whoever wakes it, under the same lock, so a task that has been woken is never
counted. A task that waits on a timer, a descriptor, or a helper thread is not
counted, because the clock or the system can still wake it. After a task blocks
and after a task finishes, the runtime compares the blocked count with the
number of live tasks (unfinished tasks plus the initial one); when they are
equal nothing can run again and the process exits with status 2. A deadlock
among some tasks while others still run is not reported.

Mutexes followed (§20.2, Q28). `Mutex<T>` is a pointer to a runtime cell holding
the lock, a queue of waiters, and the guarded value, with the same counted
handle as a channel, so copying a handle retains it and the last release drops
the value through a per-type function the compiler generates. `withLock` is
one MIR call whose code generation locks, calls the callback with the
value's address (and a scratch drop-flag block for a Move value), checks the
panic flag, and unlocks, passing the flag so a panicking callback poisons the
mutex before the unwind continues. The lock is handed directly to the
first waiter, which uses a deadlock-counted slot or task waker.

The limits list is now empty. Do not accept a
feature whose move/borrow checks and required cleanup are not yet
implemented. Any newly discovered semantic gap follows specification §53 and
`docs/spec-questions.md`.

---

### Coding agent completion report

At the end of every milestone, the coding agent should report:

```text
Milestone:
Implemented:
Files changed:
Tests added:
Tests passing:
Known limitations:
Spec questions discovered:
Next milestone:
```

Example:

```text
Milestone:
M2 Lexer

Implemented:
- identifiers
- keywords
- integer literals
- punctuation
- source spans

Files changed:
- compiler/src/lexer/mod.rs
- compiler/src/lexer/token.rs
- tests/lexer/lexer.rs

Tests added:
- identifiers
- keywords
- invalid characters

Tests passing:
- report the actual result after running the suite

Known limitations:
- list any remaining feature-specific limits

Spec questions discovered:
- none

Next milestone:
M3–M4 Parser + AST
```

Do not silently hide incomplete behavior.

---

### Change control

When a coding agent discovers that implementation requires a language-level change:

1. do not modify semantics silently
2. do not infer behavior from Rust or Go
3. document the issue
4. identify the relevant section of `spec/language-spec.md`
5. stop that specific semantic expansion
6. continue unrelated implementation where possible

Language design changes should update:

```text
spec/language-spec.md
```

before becoming implementation assumptions.

Compiler architecture changes should update:

```text
compiler-structure.md
```

before becoming project-wide assumptions.

Implementation sequencing changes should update:

```text
docs/roadmap.md
```

---

### Final implementation principle

The compiler should grow in this order:

```text
Understand source
     ↓
Understand syntax
     ↓
Understand names
     ↓
Understand types
     ↓
Understand ownership
     ↓
Understand control flow
     ↓
Guarantee cleanup
     ↓
Add concurrency
     ↓
Generate excellent native code
     ↓
Eventually compile itself
```

Do not optimize for feature count.

Optimize for:

- semantic correctness
- clear architecture
- safety
- useful diagnostics
- tests
- incremental progress
- future self-hosting

The long-term success condition is not merely:

```text
Zore programs compile
```

It is:

```text
Zore programs compile safely and predictably,
the compiler architecture remains understandable,
and Zore eventually becomes capable of compiling itself.
```
