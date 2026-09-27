# Bootstrap architecture

Status: M0–M2 are implemented. `src/main.rs` and `src/cli.rs` handle CLI
arguments and exit status. `src/lib.rs` exposes the source, diagnostic, token,
and lexer APIs; `src/source.rs` stores UTF-8 text under stable file IDs and
validated byte spans, and `src/diagnostic.rs` renders primary and related
locations. `src/token.rs` defines token kinds and `src/lexer.rs` turns one
source file into tokens plus lexical diagnostics. `tests/cli.rs`,
`tests/source_diagnostics.rs`, and `tests/lexer.rs` exercise these contracts.
Parsing and later compiler stages remain planned. Rust is the bootstrap implementation language; use one crate until
stable boundaries justify extraction.

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
integration tests and future stages. Introduce `ast` and `parser` together with
M3/M4. Do not stub future modules.

The lexer (M2) always consumes the whole file and ends with one `Eof` token.
It inserts semicolons itself (§3.7), marking each as explicit, newline, or EOF,
with inserted ones given an empty span at the start of the line terminator (the
first one inside a multiline block comment) or at EOF. Malformed input yields a
diagnostic plus a recovery token: `MalformedLiteral` (which still ends a
statement) for bad literals, `Unknown` for stray characters and `++`/`--`, and
a single `Ident` for a name containing non-ASCII letters. A file is lexically
valid only when no diagnostics are reported; later stages must not treat
recovery tokens as accepted source. String and rune literals are validated and
decoded in the lexer, because the one-scalar rune rule requires decoding.
Numeric tokens carry only their kind and base: values are decoded later from
the span, since exact untyped-constant values and range checks belong to typing.
The parser must split `>>` when closing nested type arguments such as
`Array<Task<int>>`; the lexer always takes the longest operator.

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
