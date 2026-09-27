# Bootstrap architecture

Status: M0 is implemented in `src/main.rs` (process dispatch and exit status)
and `src/cli.rs` (native-OS argument parsing and help). `tests/cli.rs` exercises
the binary's process contract. All compiler pipeline boundaries below remain
planned. Rust is the bootstrap implementation language; use one crate until
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

Introduce `source`, `span`, and `diagnostic` modules with M1; `token` and `lexer`
with M2; `ast` and `parser` together with M3/M4. Do not stub every future module.
Add a library target when the first reusable compiler APIs need integration tests.

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
