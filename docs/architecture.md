# Bootstrap architecture

Status: M0–M12 cover the implemented synchronous, all-Copy subset.
`compiler/src/main.rs` delegates to `driver`; `driver/command.rs` and
`driver/session.rs` handle CLI arguments and exit status; `check`
runs the frontend pipeline in `compiler/src/driver/check.rs`. `compiler/src/source/mod.rs` stores UTF-8 text
under stable file IDs and validated byte spans, and `compiler/src/diagnostic/mod.rs` renders
primary and related locations. `compiler/src/lexer/token.rs` and `compiler/src/lexer/mod.rs` produce tokens;
`compiler/src/ast/mod.rs` and `compiler/src/parser/mod.rs` produce the syntax tree. `compiler/src/resolve/mod.rs` binds
names to IDs, `compiler/src/types/mod.rs` interns types, and `compiler/src/types/checker.rs` produces the
typed HIR in `compiler/src/hir/mod.rs`. `tests/driver/cli.rs`, `tests/diagnostics/source_diagnostics.rs`,
`tests/lexer/lexer.rs`, `tests/parser/parser.rs`, and `tests/typecheck/check.rs` exercise these
contracts. `compiler/src/mir/lower.rs` lowers HIR to the MIR in `compiler/src/mir/mod.rs`,
`compiler/src/codegen/llvm.rs` emits LLVM IR, and `compiler/src/driver/build.rs` compiles it with clang and
the Rust sources in `runtime/src/` using rustc (decision record 0001);
`tests/codegen/native.rs` builds and
runs programs. Ownership analysis and drop insertion remain planned. Rust is the bootstrap implementation language; use one crate until
stable boundaries justify extraction.

## Repository organization

The root Cargo manifest contains the `compiler/` and `runtime/` members.
Root-level checks build and test both; `cargo run` selects the compiler's sole
binary. The compiler has no dependency on the runtime crate, and `Cargo.lock` stays
at the repository root. The layout follows
[`compiler-structure.md`](../compiler-structure.md), with only implemented stages:

- `compiler/src/driver/`: commands, sessions, frontend and native orchestration.
- `source/`, `diagnostic/`, `lexer/`, `parser/`, `ast/`: source and syntax.
- `resolve/`, `types/`, `hir/`: resolution, types, and typed representation.
  Type checking, constant evaluation, and big-number arithmetic have separate
  files under `types/`.
- `mir/`: control-flow representation and HIR lowering.
- `codegen/`: LLVM emission.

Paths after the first bullet are relative to `compiler/src/`. Existing library
paths such as `zore::check`, `zore::token`, and `zore::typeck` remain available
through re-exports. Cohesive stage implementations use `mod.rs`; further file
splits can follow when their responsibilities need them.

Integration tests live in repository-level subsystem folders, explicitly
registered in `compiler/Cargo.toml` with their existing target names. Example
paths are anchored to the compiler manifest directory, so tests work from
either the workspace root or the compiler directory.

The authoritative specification stays at `spec/language-spec.md`. The Rust
runtime in `runtime/src/` separates I/O, panic reporting, and string comparison.
Its `main.rs` is a native entry shim compiled only when linking a Zore program;
the Cargo library target enables runtime unit tests without a generated entry.
Future ownership, drop, async, shared-context, and standard-library modules
remain uncreated.

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

M1 stores `Span` with its source manager in `source`; a separate `span` module
would have no independent work. The library target exposes these APIs to
integration tests and future stages. Do not stub future modules.

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
rejects structs that contain themselves by value. `typeck` then checks types
without re-resolving strings and builds HIR: every expression has a type,
constant expressions are folded, assignment targets are places (a local plus
field projections), and locals carry their declaration kind. Typed constant
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

The checker accepts a deliberately small subset: one file per package; `bool`,
integer and float types, `rune`, `string`, and structs of those; functions and
methods with default, `mut` or `own` parameters and receivers; `println`; numeric conversions. `error`/`nil`/`?`,
`async`/`await`, imports, package variables, rune
conversions, and function values are reported as unsupported. `println` of a
float type-checks, but its text format is still TBD (§37.1).
Because every accepted type is Copy, no move rule can be violated yet, and `mut`
arguments are checked only for mutable places and exclusivity within one call; `typeck` refuses to produce HIR containing a non-Copy type, so
this cannot silently change when new types are added before ownership analysis.

AST preserves written structure; HIR records resolved meaning; MIR describes
execution. Source identity and spans survive transformations. Use typed IDs for
semantic entities and place projections for fields/indexes. Ownership data-flow
must handle branches and eventually partial moves and async state. Track
recursive borrow provenance independently of Copy/Move classification, including
exclusive reborrow relationships and input-to-result contracts (§11.7, §12.3).
Projected moves must respect custom-destructor boundaries (§31.2). Task/channel
escape checks need independent backing lifetime proofs across error and unwind
paths (§18.4, §19.4). These are planned requirements, not implemented passes.

The pass order in §25 is conceptual. MIR construction, ownership analysis, async
lowering, and drop insertion may need multiple steps. Document concrete ordering
when these stages exist; preserve safety before and after transformation. Keep
ownership validation and destruction placement separate responsibilities.

`check` must work without a backend, linker, runtime, or LLVM installation.
Before adding LLVM, record its version, binding strategy, host target, runtime ABI,
and setup/test instructions in an architecture decision document. Initial native
output must integrate a backend early enough to support the native milestones;
M32 is backend hardening, not the first backend implementation.

No scheduler, allocator, string layout, or LLVM binding has been chosen. These
are internal choices, constrained by the observable language guarantees. Record
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
Operands distinguish `Copy`, `Move`, and constants; lowering emits only `Copy`
and constants because every accepted type is Copy. Lowering fixes evaluation
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
operation and source location. Since no accepted type has a destructor, a
panic needs no unwinding. When drop insertion lands (M18), these checks become
explicit MIR assert terminators with cleanup paths. Runtime string
concatenation and float printing are reported as unsupported by the backend.
The generated IR contains no target triple, so clang supplies the host's; it
requires LLVM 15 or newer for opaque pointers.

The driver embeds the Rust runtime sources, compiles the LLVM IR to a native
object with clang, then invokes rustc 1.98+ to compile the runtime entry shim
and link that object. Rustc manages its standard-library and system-library
dependencies. Runtime sources do not need to be installed alongside `zore`.
Native builds require both tools for the same host; `ZORE_CC` and `ZORE_RUSTC`
select their executables. There is no runtime artifact cache yet. Rust startup
provides SIGPIPE handling; output is locked and explicitly flushed before
returning, so write failures become Zore panics. The unsafe Rust boundary is
limited to the internal ABI and does not introduce source-level unsafe syntax.

