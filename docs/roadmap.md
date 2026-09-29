# Implementation roadmap

The implementation covers an initial synchronous, single-file, all-Copy subset:
bool, integers, floats, rune, string, and structs of those, with exact untyped
constants (§6.7). `zore check` runs lex → parse → resolve → type-check;
`zore build` and `zore run` lower to MIR, emit LLVM IR, and invoke clang and
rustc to link the Rust runtime (decision record 0001). Unsupported features
receive diagnostics; ownership analysis and drop insertion do not exist yet.

Current baseline: commit `95110b1`. GitHub Actions passed on ubuntu-latest and
macos-latest for that commit: rustfmt, clippy with warnings denied, build, docs,
and `cargo test --locked --all-targets`, including the native tests. The
folder refactor and Rust runtime migration are validated by that run. Local
Windows builds are not covered.

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
| M0 — implemented; validation pending | CLI/driver: help, version, argument validation, honest unsupported-command errors; subprocess tests for exit status and output. Prioritize `check <file.ore>`. |
| M1 — implemented; validation pending | Source manager, file IDs, byte spans, diagnostic rendering; test empty input, UTF-8 boundaries, line endings, EOF, and multiple files. |
| M2 — implemented; validation pending | Tokens and lexer for agreed lexical rules; test spans, valid tokens, invalid input, EOF, and progress after errors. Q01 lexical choices are resolved; use the locked rules. |
| M3–M4 — subset implemented; validation pending | AST and parser together for package/functions/structs/bindings/calls; test shape, spans, recovery, and rejection. Resolve relevant grammar questions first. |
| M5–M8 — subset implemented; validation pending | Hello program, variables, functions, structs. Establish the minimal native backend and builtin output support needed to run examples. Use resolution/type work below as prerequisites where needed. |
| M9–M12 — subset implemented; validation pending | Name resolution, types, HIR, MIR/CFG; semantic IDs, typed calls/fields, explicit control flow, frontend-only checking. |
| M13–M17 | Copy/Move, shared/mutable borrowing, regions; paired acceptance/rejection tests including branches and projected places. Complete the §42 semantic target. |
| M18–M19 | Drop insertion and explicit errors/`?`; verify exactly-once cleanup on normal, branch, and early-return paths. |
| M20–M23 | Fixed arrays, borrowed slices, owned arrays, maps, packages/imports; validate ownership and package visibility. |
| M24 | Closures with capture analysis; reject captures that cannot remain valid. |
| M25–M29 | Task model, `go`, async states, `await`, scheduler; test lifetime proof, suspension, completion, error results, and detach behavior. |
| M30–M31 | Channels and async I/O; test copying handles, message ownership, buffering, close/drain, and panic on closed send. |
| M32–M34 | LLVM/native toolchain hardening and library growth; native execution tests and full MVP coverage audit against §39/§46/§52. |
| M35+ | Self-hosting work after bootstrap and library capabilities are sufficient. |

The early native milestones and semantic milestones overlap: integrate the
resolution, type checks, and lowering required by a program before calling it
supported. Never bypass ownership rules just to make a demonstration execute.

The compiler lives in `compiler/`, alongside the `runtime/` Rust workspace
member, with stage folders and explicitly registered subsystem tests (see
`architecture.md`).
The layout refactor does not advance language milestones.
The Rust runtime migration preserves the existing subset; native validation of
the migration is pending on a host with Rust and clang installed.

## Next implementation session

The refactor/runtime migration baseline is validated (see above). Its first CI
run failed on rustfmt drift and on a native test that declared `var` without
the initializer that the specification requires; both were fixed. The
language-level work below may proceed.

The next language-level step after validation is ownership (M13–M17):
Copy/Move classification in MIR,
use-after-move and drop insertion (`mut` parameters and receivers are done). It needs
a first Move type to be meaningful; the smallest candidates are a struct with a
user-defined `drop` method (§8.3, §14.3), which needs the methods now
implemented (§9.1), or owned arrays (M21). Ownership analysis and the required deterministic
cleanup must land before any Move type is accepted. The all-Copy restriction is what currently makes
ownership checks vacuous, and `typeck` enforces it.

Other open items: `error`/`?` (M18–M19), rune conversions, the `println` float
text format (§37.1, TBD), and runtime string concatenation, which needs a
string-buffer ownership decision (§41.5). Runtime checks panic from code
generation today; they move to explicit MIR assert terminators with cleanup
paths when drop insertion (M18) arrives.

Temporary limits that are not language rules: `check <file.ore>` treats the one
file as the whole package and diagnoses `import` until package discovery (Q05,
M23); package-level `let`/`var` await Q05.

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
| Import discovery, project mapping, package initialization | Before supporting imports, project checking, or package variables; an explicitly limited single-file milestone need not resolve the whole package system |
| String indexing/slicing/length | Before implementing those operations; literal decoding and immutable string values are already specified |
| Collection iteration, borrowed map-entry access, remaining collection APIs | Before implementing those operations at M20–M24; accepted array/map forms remain usable as their stages arrive |
| Closure types/captures/invocation | Before the affected M24 parser and semantic work |
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
Both need execution against the refactored baseline before current success is
claimed.
Because its `User` contains only a Copy string, add a separate Move-resource test
when available to prove that ordinary calls borrow rather than consume values.

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

### Current implementation baseline — 2026-09-28

Inspected commit: `39dde45` (folder refactor and Rust runtime migration);
CI-validated at `95110b1`. The table below is a source-inspection summary of
scope. CI passing establishes that existing tests pass, not that any milestone
is complete beyond the subset it covers.

| Canonical milestone | Current code and remaining scope |
| --- | --- |
| M0–M1 | Workspace, CLI, source manager, spans, labels, notes, and rendering exist. File-load errors are plain CLI messages; diagnostic codes are absent. |
| M2–M4 | Lexer and AST/parser implement the current subset. Async declarations have syntax representation but are rejected semantically; collections, indexing, and closures remain unsupported. |
| M5–M8 | Hello, variables, functions, structs, control flow, and multiple returns have implementations and native tests for the synchronous all-Copy subset. Methods with shared or `own` receivers resolve, type-check, and run natively; `mut` parameters and receivers require mutable places (§11.6) and are passed by reference. |
| M9–M10 | Single-file resolution, stable IDs, primitive/struct types, type checking, and exact constant evaluation exist. Function types/values, error, imports, and package variables remain unsupported. |
| M11–M12 | Typed HIR, CFG MIR, and local/field places exist. MIR distinguishes Copy/Move operands structurally; Move validation and indexed places are absent. |
| M13–M17 | Recursive Copy classification exists for supported types in HIR. `mut` parameters and receivers check mutable places and call-local exclusivity and run natively. No accepted Move types, move-state analysis, stored borrows, or region analysis. |
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

Blocked locally: Cargo and rustc are unavailable on this host. No passing
formatting, Clippy, build, test, or documentation result is claimed for the
refactor/runtime migration. Static file/ABI-symbol inspections are not a
substitute. Record actual command results and the validated commit when a
suitable host or CI run is available.

#### Next language work after validation

Proceed toward M13–M17 ownership with paired acceptance/rejection cases.
Choose a first Move-bearing feature explicitly: owned arrays (M21), or a
struct with a custom destructor, requiring methods and drop support (M18).
Basic methods on existing Copy types can be a preparatory slice. Do not accept
a Move type until its move/borrow checks and required cleanup are implemented;
do not accept mutable parameters before exclusivity is enforced. Any newly
discovered semantic gap follows specification §53 and `docs/spec-questions.md`.

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
