# Bootstrap architecture

Status: The compiler supports a synchronous subset through parts of M18–M20,
including Move structs, deterministic drops, concrete error values, fixed
arrays, dynamic `Array<T>`, maps, borrowed slices with region analysis, and
non-escaping closures. Paths below are relative to `compiler/src/`. `main.rs` delegates to
`driver`; `driver/command.rs` and `driver/session.rs` handle CLI arguments and
exit status; `driver/check.rs` orchestrates the frontend pipeline and
`driver/build.rs` the native one. `source/` stores UTF-8 text under stable
file IDs and validated byte spans, and `diagnostic/` renders primary and
related locations. `lexer/` produces tokens; `parser/` produces the `ast/`
syntax tree. `resolve/` binds names to IDs, `types/` interns types, and
`hir/lower.rs` type-checks the resolved syntax while lowering it to the typed
HIR defined in `hir/`. `mir/lower.rs` lowers HIR to the MIR in `mir/`;
`ownership/` (including `ownership/error_use.rs`) validates it, and `dropck/` inserts
deterministic drops before `codegen/` emits LLVM IR, which `driver/build.rs`
compiles with clang and links with the Rust sources in `runtime/src/` using
rustc (decision record 0001). Integration tests under `tests/<subsystem>/`
exercise each contract. Rust is the bootstrap implementation language; use one
crate until stable boundaries justify extraction.

## Repository organization

The root Cargo manifest contains the `compiler/` and `runtime/` members.
Root-level checks build and test both; `cargo run` selects the compiler's sole
binary. The compiler has no dependency on the runtime crate, and `Cargo.lock` stays
at the repository root. The layout follows
[`compiler-structure.md`](../compiler-structure.md), creating a named file only
when existing code belongs in it (rule 11: no empty scaffolding):

- `driver/`: `command.rs`, `session.rs`, plus `check.rs` and `build.rs`, the
  frontend and native pass orchestration.
- `source/` (`span.rs`, `source_file.rs`, `source_map.rs`), `diagnostic/`
  (`diagnostic.rs`, `label.rs`, `renderer.rs`).
- `lexer/` (`lexer.rs`, `token.rs`, `token_kind.rs`), `parser/` (`parser.rs`,
  `declaration.rs`, `statement.rs`, `expression.rs`, `type_syntax.rs`), `ast/`
  (`node.rs`, `decl.rs`, `stmt.rs`, `expr.rs`, `types.rs`).
- `resolve/` (`resolver.rs`, `scope.rs`, `symbol.rs`, `ids.rs`, and `units.rs`,
  the package and file units handed to resolution), `types/` (`type_id.rs`,
  `ty.rs`, `type_store.rs`, plus `constant.rs` and `bignum.rs` for exact
  constant evaluation), `hir/` (`expr.rs`, `stmt.rs`, `function.rs`, `lower.rs`,
  and `lower/closure_kind.rs`, which decides which closures own their captures).
- `ownership/` (`checker.rs`, `move_state.rs`, `borrow.rs`, `region.rs`, and
  `error_use.rs`, the check that every `error` value is read or discarded),
  `mir/` (`body.rs`, `block.rs`, `statement.rs`, `terminator.rs`,
  `operand.rs`, `rvalue.rs`, `lower.rs`), `dropck/` (`insertion.rs`).
- `codegen/` (`llvm.rs`, `layout.rs`, `abi.rs`, plus `clone.rs`, `channel.rs`,
  `mutex.rs`, and `task.rs`, which lower `clone` and the channel, `Mutex`, and
  task operations, and `native.rs`, the shims that call the runtime for library
  functions declared without a body).

Each stage's `mod.rs` re-exports its own submodules, so stage paths such as
`zore::ast::Expr` or `zore::hir::ExprKind` do not expose the file split.
`zore::check` and `zore::build` remain as crate-root shortcuts to the driver.
The files the structure guide names but that have nothing to hold yet stay
uncreated: `ownership/{place,projection}.rs` (MIR places serve both
purposes), `dropck/analysis.rs`, `types/{function_type,classify}.rs`,
`diagnostic/code.rs` (no diagnostic codes yet), and `context/`.
`async_lowering/` stays uncreated for a different reason: tasks are stackful
fibers in the runtime, so an async function is an ordinary function that runs on
a fiber's stack and nothing lowers it to a state machine. The guide's
`runtime/scheduler.rs` is likewise `fiber.rs` and `task.rs`. The structure guide
lists these choices as approved deviations.

Dependencies point only from later stages to earlier ones, with no cycles,
apart from three deliberate choices. `resolve` depends on `types` because
binding type names to `TypeId`s is name resolution; `types` no longer depends
on any later stage. The type checker lives in `hir/lower.rs` rather than
`types/`, because it builds HIR: placing it in `types/` made `types` and `hir`
(and `types` and `resolve`) depend on each other. `Package::is_copy` stays in
`hir/` because Copy/Move classification needs struct fields and `drop`
methods, which live in the HIR package; `types/classify.rs` would recreate the
cycle. `Place` and `Projection` stay in `mir/` (the ownership checker reads
them), since moving them into `ownership/` would make `mir` depend on
`ownership`.

Integration tests live in repository-level subsystem folders, explicitly
registered in `compiler/Cargo.toml` with their existing target names. Example
paths are anchored to the compiler manifest directory, so tests work from
either the workspace root or the compiler directory.

The authoritative specification stays at `spec/language-spec.md`. The Rust
runtime in `runtime/src/` holds allocation (`alloc.rs`), text (`string.rs`,
`strings.rs`, `strconv.rs`), maps (`map.rs`), panics (`panic.rs`), and output
(`io.rs`). It also holds tasks and their stack-switching fibers (`task.rs`,
`fiber.rs`), channels and `select` (`channel.rs`), `Mutex` (`mutex.rs`),
deadlock detection (`deadlock.rs`), and async I/O: the event loop and timers
(`reactor.rs`), helper threads for blocking calls (`blocking.rs`), and the
natives behind `zore/time`, `zore/io`, `zore/os`, and `zore/net` (`sys.rs`,
`net.rs`). The driver compiles it once into a cached library, and `main.rs` is
the native entry shim compiled with each program; the Cargo library target
enables runtime unit tests without a generated entry.

| Area | Responsibility | Spec |
| --- | --- | --- |
| CLI/driver | Arguments, source loading, pipeline orchestration, exit status | §45 |
| Source/spans/diagnostics | File identity, byte ranges, source rendering | §24, §26 |
| Tokens/lexer | Source to tokens with spans and lexical errors | §46.1 |
| Parser/AST | Source structure, recovery, syntax diagnostics | §25, §46.2 |
| Resolution/types/HIR | Package scope, semantic IDs, typed meaning | §27–29 |
| MIR/CFG | Places, explicit Copy/Move/borrow operations, control flow | §30–33 |
| Ownership/regions | Availability, aliasing, borrow validity, lifetime proof | §31–32 |
| Drop analysis | Explicit destruction on all required paths | §34 |
| Async/task lowering | Persistent state, suspension/resumption, task creation | §17–18, §35 |
| Code generation | LLVM lowering, object generation, native linking | §2.2, §39.5 |
| Runtime/library | Allocation, I/O, tasks, scheduler, channels, panic | §36–37 |

M1's `source/` keeps `Span`, `FileId`, and `LineColumn` in `span.rs`, separate
from the file and manager that issue them. The library target exposes these
APIs to integration tests and future stages. Do not stub future modules.

The lexer (M2) always consumes the whole file and ends with one `Eof` token.
It inserts semicolons itself (§3.7), marking each as explicit, newline, or EOF,
with inserted ones given an empty span at the start of the line terminator (the
first one inside a multiline block comment) or at EOF. Malformed input yields a
diagnostic plus a recovery token: `MalformedLiteral` for bad literals, `Unknown`
for stray characters and `++`/`--` (both end a statement, so recovery stays
line-based), and a single `Ident` for a name containing non-ASCII letters. A file is lexically
valid only when no diagnostics are reported; later stages must not treat
recovery tokens as accepted source. String and rune literals are validated and
decoded in the lexer, because the one-scalar rune rule requires decoding.
Numeric tokens carry only their kind and base: values are decoded later from
the span, since exact untyped-constant values and range checks belong to typing.
The parser must split `>>` when closing nested type arguments such as
`Array<Task<int>>` once that syntax is parsed; the lexer always takes the
longest operator.

The parser (M3–M4) is hand-written recursive descent. `parser::parse` lexes
and parses one file, returning a `File` AST and all lexical and syntax
diagnostics. The AST keeps written structure (parentheses, `_` targets,
unresolved names, and numeric spellings via spans); meaning belongs to HIR.
Expressions use precedence climbing over the §7.6 levels, with comparisons
non-associative and `await x?` built as `(await x)?`. A flag disables struct
literals in `if`/`for` headers (§8.4); a counting-loop initializer is re-parsed
with literals enabled when it turns out to be an assignment. Newline-dependent
errors (missing trailing comma, body brace or `else` on the next line) get
dedicated messages. On an error the parser records a diagnostic and skips to
the end of the statement or declaration, tracking bracket depth; between
declarations it also stops before a line-initial `func`/`async`/`type`/`import`.
Recovery always consumes input (debug assertions check this). Syntax owned by
later milestones is reported as unsupported, naming the planned milestone,
rather than parsed speculatively. As with lexing, a file is syntactically valid
only when no diagnostics are reported.

Semantic checking (M9–M10) runs only on a syntactically valid file, so parser
recovery artifacts cannot cause cascades. `resolve` first collects package
declarations (so functions, types, and constants may be referenced before they
are declared), assigns `StructId`/`FunctionId`/`ConstId`/`LocalId`, and records
what every name and type name refers to in side tables keyed by the name's
span. It enforces duplicate, scope-entry, and predeclared-shadowing rules and
rejects structs that contain themselves by value. `hir/lower.rs` then checks
types without re-resolving strings and builds HIR: every expression has a
type, constant expressions are folded, assignment targets are places (a local
plus field and index projections), and locals carry their declaration kind. Typed constant
overflow, division by zero, and invalid constant shift counts are compile-time
errors. Return completeness is checked
conservatively on the AST. HIR is produced only when there are no diagnostics,
and diagnostics are reported in source order.

Constant evaluation (§6.6–6.7) lives in `compiler/src/types/constant.rs`, separate from type
checking. Untyped constants have an integer or float kind and are exact:
integers are arbitrary-precision, and floats are exact rationals. Both use the
hand-written `compiler/src/types/bignum.rs` (no numeric dependency, and easy to port to Zore);
its unit tests use Rust's `i128` arithmetic and correctly rounded
`str::parse::<f32/f64>` as oracles. Implementation limits, all above the §6.7
minimums: untyped integers up to 4096 bits (larger is an error); floats keep
exact rationals until numerator or denominator exceeds 40,000 bits, then round
to 512 significant bits; float magnitudes must stay below 2^32767 (overflow is
an error; far smaller magnitudes round to zero). Typing a constant applies
§6.7 representability: integers need an integral value in range, and floats
round to nearest, ties to even, without overflow. Typed float constants hold
the exact value of their `float32`/`float64` and fold by exact arithmetic
followed by one rounding, which equals the correctly rounded IEEE result.

The checker accepts a deliberately small subset: primitive values,
`error`, structs including Move structs with custom `drop` methods, fixed
arrays, literal-sized dynamic arrays (`Array<T>`), maps (`map[K]V`), borrowed slices (`[]T`,
`mut []T`, `base[low:high]`), functions, methods, non-escaping closures with
function types (§16), and `println`. Ownership analysis (`ownership/`) checks whole-place
and field-level partial moves, reinitialization, and call-local borrows over
MIR (`checker.rs`), then runs region analysis (`region.rs`) over the loans
described in `borrow.rs`; error-use analysis checks named `error` bindings
and parameters on normal control-flow paths. Awaited `?`, `async`/`await`, package variables,
integer-to-rune conversions, and declared functions used as values remain
unsupported. `println` of a float
type-checks, but its text format is still TBD (§37.1).

A program is more than one file. `driver/project.rs` loads the entry file's
folder and every package it imports, through a `FileSystem` trait so tests can
use memory instead of disk, and hands the resolver a list of packages with
dependencies first. Spans carry their file, so diagnostics point into any
package; the source text of the bundled standard packages lives in a separate
map that the checker and code generator read through `Sources`.

The bundled standard packages (`driver/stdlib/*.ore`) declare functions
without bodies. The checker lowers them like any function but marks them
native; MIR gives them no blocks, and code generation emits a shim that calls
the runtime symbol `zore_native_<package>_<function>` (`HasPrefix` in
`strings` is `zore_native_strings_has_prefix`).

AST preserves written structure; HIR records resolved meaning; MIR describes
execution. Source identity and spans survive transformations. Use typed IDs for
semantic entities and place projections for fields/indexes. Ownership data-flow
handles branches and partial moves, and must eventually handle async state. Track
recursive borrow provenance independently of Copy/Move classification, including
exclusive reborrow relationships and input-to-result contracts (§11.7, §12.3).
Projected moves must respect custom-destructor boundaries (§31.2). Task/channel
escape checks need independent backing lifetime proofs across error and unwind
paths (§18.4, §19.4). Async lifetime proofs remain future work.

Region analysis is an NLL-style pass. A loan is created by slicing, by copying
a `mut []T` (an exclusive reborrow, §12.3), or by a call whose result borrows
from an argument. Loans cover storage paths: a MIR place with an explicit
`Deref` step wherever a slice descriptor is indexed, so a loan through a
descriptor never blocks reassigning or ending the descriptor itself. Each
local that can hold a view carries a may-set of loans (forward data flow;
whole-local assignment replaces it and ends loans through the replaced
descriptor). Backward liveness decides where each local's value can still be
read; a loan is live exactly while a live local holds it. Every access is
checked against the live loans (reads conflict only with exclusive loans;
writes, moves, and scope ends with any), and a slice of any part of an owner
borrows the whole originating place (§12.5). Return contracts record, per
result and parameter, whether the result views the argument's own storage or
forwards views it already holds; they are computed as a least fixpoint over
all functions, so recursion is handled, and substituted at each call. A
returned view must be backed by a by-reference parameter or forwarded from a
parameter's views (§11.7). Shared parameters whose type contains a fixed array
are passed by reference so such views can be returned. Temporary
`Array<T>` indexing takes no `Deref` step: its elements are the owning
local's storage, so replacing the array conflicts with live views of it.

Native code represents `Array<T>` as a `{ ptr, i64 }` descriptor over heap
storage from the runtime's `zore_alloc`. Dropping it drops the elements in
reverse index order with an IR loop, then calls `zore_free`. The drop helpers
take addresses rather than MIR places so that loop can address elements by a
runtime index. Scratch drop-flag allocas are hoisted into the entry block. A
drop inserted before a replacing store acts on a panic only after the store.
Maps are pointers to a type-erased hash table in `runtime/src/map.rs`; null is
the empty zero map. MIR expresses literal entries, lookup, `m[k] = v`, and
`m.remove(k)` as compiler-provided `Callee`s. Codegen passes the key through
a hoisted slot with a runtime kind code. A stored value's loans join the
map's holdings, and a value read or removed from the map carries them out.

Closures (§16) reuse this machinery. The resolver gives each closure literal
its own `FunctionId` (after the declared functions) and locals; a name that
resolves to a local of an enclosing body becomes a `LocalKind::Capture` local,
chained through every closure in between. The checker types a literal's body in
place, with its captures typed from the enclosing locals, and marks a capture
exclusive when the body writes through it (which requires the original binding
to be a mutable place) or when it holds a closure or `mut []T`. A function type
is interned in the `TypeStore` with its signature; closure values are Move and
count as view-holding, so every closure-related borrow is a loan. MIR creates a
closure with `Rvalue::Closure`, whose loans on the captured places (shared or
exclusive) are held by the closure value until its last use; a call through a
value is `Callee::Value(place)`, an exclusive access of the callee, and a
function value passed to a function-typed parameter is borrowed exclusively.
The closure body is an ordinary MIR body whose capture locals are by-reference
locals. Codegen represents a closure as `{ code, environment }`: the
environment is a hoisted `[N x ptr]` in the creating frame holding each captured
place's address (and its drop flags' address for a Move value), and the body
receives it as a leading `ptr` parameter and loads its capture locals from it.
Function-typed results, fields, and elements are rejected, so a closure cannot
escape the frame that holds its environment.

Clone (§10.7) has two forms. A declared custom `clone` is an ordinary method,
validated at its declaration (shared receiver, no parameters, own type as the
only result) and selected before any structural clone. Every other clone is
`Callee::Clone`, one MIR call that borrows its argument and writes an owned
value into its destination. Codegen (`codegen/clone.rs`) expands it inline per
type: a Copy value is copied, a struct clones field by field, a fixed array or
`Array<T>` clones element by element into fresh storage, and a map clones its
values into a copy made by the runtime's `zore_map_clone_shape`. A custom clone
of a non-Copy part is called with the part's address and scratch drop flags,
then the pending-panic flag is checked; on a panic the parts already cloned are
dropped and the new storage freed before control reaches the call's ordinary
unwind edge. Region analysis gives the destination the loans of a source that
holds views.

A view stored through a mutable slice is added to every owner that the slice
exclusively borrows from, following reborrows back to the original storage
(Q22b). `mut []T` may be held in `Array<T>`, map, and `mut []T` elements, but
not in a shared slice element or copied out by a map lookup (Q20b).

Temporary
restrictions, each diagnosed: a shared parameter cannot hold a `mut []T` inside
a struct, fixed array, or collection; a closure can store
into a capture only views of other captures; a value whose custom `drop`
reads a view must be declared after the storage it views; and a closure cannot
consume a captured Move value.

Return contracts also carry outputs (Q22): for each `mut` parameter or
capture, which inputs' storage or views it may receive, read from its holdings
at `Return` (such by-reference locals stay live until then). A call applies a
function's outputs to its `mut` arguments, and a closure's capture outputs are
applied where it is created. A call through a function value assumes its
results and `mut` arguments may borrow from every argument and from the
callee's own holdings.

A value whose destruction runs a custom `drop` that can read a view (Q21) is
an "observing" local in region analysis. Its destruction is a use: liveness
marks it live at its `EndScope`, at `Return`, and before a whole-value
assignment replaces it. `EndScope` checks each local's storage death against
only the observing locals of the same scope that drop after it, newest first.
A whole-local move clears the moved local's holdings.

An owning closure (Q02i) is decided after its enclosing body is checked:
literals in escaping positions, or bound to locals that escape, become owning,
and a literal whose body moves a capture is call-once. MIR's
`Rvalue::Closure { owning: true }` copies or moves each capture into the
environment instead of borrowing it, and a direct call of a call-once local is
followed by a `drop` of it. Code generation gives every closure a destructor
slot; an owning closure's heap environment begins with the capture pointers
its body loads, so one body serves both kinds.

A collection loop lowers to a counting loop. MIR binds two by-reference
temporaries with `Rvalue::Ref`: one to the collection, read by the loop header
each iteration so its shared loan lasts the whole loop, and one per iteration
to the current element (`Rvalue::MapValueRef` for a map value; keys are copied
with `Rvalue::MapKeyAt`). These binding loans are marked so that a value copied
out of an item keeps only the views the element holds. Drop insertion never
treats binding a reference as replacing a value. `Array<T>` is `{ ptr, len,
cap }`; `push` grows the storage through the runtime, doubling from 4.

The pass order in §25 is conceptual. The frontend lowers checked HIR to MIR,
runs ownership and error-use analysis, then returns diagnostics or a package.
Native builds lower the accepted package again and insert drops before code
generation. Async lowering remains future work. Ownership validation and
destruction placement are separate responsibilities.

`check` must work without a backend, linker, runtime, or LLVM installation.
The LLVM backend decision, supported toolchain, host target, and runtime ABI
are recorded in decision record 0001. M32 is backend hardening, not the first
backend implementation.

No scheduler has been chosen. Further internal choices are constrained by the
observable language guarantees. Record
substantial decisions with context, alternatives, consequences, and validation
under `docs/decisions/` when they are made.

Source files are UTF-8 and retain their original OS path. Source IDs include a
manager identity, so spans cannot accidentally resolve against another source
manager. Spans use half-open `u32` byte ranges and require UTF-8 boundaries;
files above 4 GiB are rejected. Lines split at LF, with CRLF terminators hidden
when rendering; a lone CR remains on its line. Locations are one-based Unicode
scalar columns. Diagnostic snippets expand tabs and escape control characters.
The renderer shows the first line of a multi-line span and reports its extent.
Terminal-cell width for wide Unicode glyphs is not yet measured; source offsets
and reported scalar columns remain exact. This display choice may improve later
without changing language semantics. No LLVM or runtime dependency is involved.

## MIR and native code generation

MIR (§33) is a control-flow graph per function: locals (HIR locals keep their
indexes, temporaries follow), basic blocks of `place = rvalue` statements, and
one terminator per block (`Goto`, `Branch`, `Call`, `Return`, `Unreachable`).
Operands distinguish `Copy`, `Move`, references, and constants. Lowering fixes evaluation
order explicitly: operands and arguments left to right, assignment values
retained before stores, struct fields in written order then assembled in
declaration order, compound assignment reading its target before the
right-hand side, and `&&`/`||` as branches. Calls are terminators, so results
land in temporaries or discarded destinations.

Code generation (`compiler/src/codegen/llvm.rs`) gives every MIR local a stack slot and
relies on LLVM's optimizer to promote them. §6.6 runtime checks are emitted
there: overflow intrinsics for `+ - *` and negation, zero and `MIN / -1`
checks for `/` and `%`, unsigned range checks for shift counts, and range
checks for numeric conversions; each failure calls `zore_panic` with the
operation and source location. MIR assert terminators carry cleanup paths so a
panic runs pending drops before the runtime reports it. Runtime strings are
`{ ptr, len }` descriptors: concatenation and `string(rune)` call the runtime,
which allocates the text in a counted buffer, and slices share their source's
storage after a bounds and character-boundary check. A copy that is kept adds an
owner (`zore_string_retain`) and ending its life removes one
(`zore_string_release`); the last owner frees the buffer. Text counts as a
value that needs cleanup even though it is Copy: drop flags, scope-end drops,
and the panic cleanup path cover it, and a value of a Copy type that holds text
(a struct or fixed array with a `string` or `error` inside) is retained field by
field when it is copied. Concatenation extends the buffer in place when the left text ends where
the buffer's text ends, which is safe because no string reads past its own end. Float
printing is reported as unsupported by the backend.
The generated IR contains no target triple, so clang supplies the host's; it
requires LLVM 15 or newer for opaque pointers.

The driver embeds the Rust runtime sources and compiles the LLVM IR to a native
object with clang. It compiles the runtime once into a Rust library and keeps
that library in a per-user cache folder (`$XDG_CACHE_HOME/zore`, `~/.cache/zore`,
`~/Library/Caches/zore` on macOS, or `ZORE_CACHE_DIR`), named by a hash of the
runtime sources and the `rustc -vV` output; a folder that cannot be used leads to
compiling the runtime for that build alone. Each program then needs only rustc
1.98+ to compile a small entry shim against that library and link the object.
Rustc manages its standard-library and system-library dependencies. Runtime
sources do not need to be installed alongside `zore`. Native builds require both
tools for the same host; `ZORE_CC` and `ZORE_RUSTC` select their executables.
Cache entries are never removed by the compiler; deleting the folder is always
safe. Rust startup
provides SIGPIPE handling; output is locked and explicitly flushed before
returning, so write failures become Zore panics. The unsafe Rust boundary is
limited to the internal ABI and does not introduce source-level unsafe syntax.
