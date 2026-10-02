# Compiler testing

`tests/driver/cli.rs` contains executable subprocess tests for the M0 driver: help,
version, usage errors, unsupported commands, native paths, and option delimiters.
`tests/diagnostics/source_diagnostics.rs` tests M1 UTF-8 loading, file IDs, byte spans,
line/column lookup, EOF, and multi-file diagnostic rendering. `tests/lexer/lexer.rs`
tests M2 token kinds, spans, literal validation and decoding, semicolon
insertion, diagnostics, and recovery progress; its cases come from the lexical
rows of the conformance documents below. `tests/parser/parser.rs` tests M3–M4 AST
shape (via an S-expression rendering), spans, syntax rejection, unsupported
later-milestone syntax, recovery, and termination on generated input.
`tests/typecheck/check.rs` tests resolution, type checking,
§6.7 constant evaluation, float typing and conversions, HIR shape, the
entry-point and `println` contracts, and that unsupported features are rejected
rather than accepted. Error-value tests cover `nil`, construction, equality,
explicit discard, ignored-result diagnostics, and path-sensitive checks for named
error bindings and parameters. Synchronous `?` tests cover typing, early return,
zero-filled results, evaluation order, and cleanup; awaited propagation remains
pending. Unit tests in `compiler/src/types/bignum.rs`
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

`conformance/` records spec-level cases awaiting executable coverage. In
particular, `conformance/identifiers.md` covers the locked ASCII identifier rules,
`conformance/comments.md` covers line and non-nesting block comments, and
`conformance/statement-boundaries.md` covers automatic semicolon insertion.
`conformance/strings.md` covers quoted and raw string literal forms.
`conformance/runes.md` covers single-scalar rune literals and escapes.
`conformance/integers.md` covers integer bases and prefix validation.
`conformance/floats.md` covers decimal fractions and scientific notation.
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
interaction, resource zero states, and panic unwinding.
`conformance/concurrency.md` covers async calls, task typing and retrieval,
task panics, process exit, all-exit spawn lifetime proof, and channel message
escape/close/zero-value/cleanup behavior.
`conformance/arrays-slices.md` covers typed array literals, bounds, indexing,
contextual mutable slices, and partial-construction cleanup (§12.6).
`conformance/maps.md` covers map literals, key restrictions, presence-first lookup
and removal, mutation, and entry cleanup (§13.3).
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
have counterparts in both `tests/typecheck/check.rs` and `tests/codegen/native.rs`.
Fixed-array literal/arity typing, indexing, mutable-place and
conservative-aliasing rules, and index-move rejection in `arrays-slices` have
parser counterparts in `tests/parser/parser.rs` and checker counterparts in
`tests/typecheck/check.rs`; `tests/codegen/native.rs` covers the honest
unsupported-backend diagnostic. Slicing, dynamic `Array<T>`, and mutable-slice
rows in `arrays-slices` remain pending, as do the remaining ownership,
runtime, and native rows elsewhere.

Use Rust unit tests for small source/IR utilities and pass algorithms. Use Cargo
integration tests in subsystem folders under `tests/` for public compiler APIs
and CLI subprocess behavior. Each target is registered in `compiler/Cargo.toml`
with its original name (`cli`, `source_diagnostics`, `lexer`, `parser`, `check`,
and `native`), so commands such as `cargo test --test lexer` still work.
Resolve examples from `env!("CARGO_MANIFEST_DIR")` plus `../examples/`, not the
process working directory. A directory of fixtures alone is not an executable test.

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
