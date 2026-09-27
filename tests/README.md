# Compiler testing

`tests/cli.rs` contains executable subprocess tests for the M0 driver: help,
version, usage errors, unsupported commands, native paths, and option delimiters.
`tests/source_diagnostics.rs` tests M1 UTF-8 loading, file IDs, byte spans,
line/column lookup, EOF, and multi-file diagnostic rendering. No language
conformance runner or compiler semantic tests exist yet. Add tests
alongside each stage; do not create ignored tests to imply that pending features
have coverage.

`conformance/` records spec-level cases awaiting executable coverage. In
particular, `conformance/identifiers.md` covers the locked ASCII identifier rules,
`conformance/comments.md` covers line and non-nesting block comments, and
`conformance/statement-boundaries.md` covers automatic semicolon insertion.
`conformance/strings.md` covers quoted and raw string literal forms.
`conformance/runes.md` covers single-scalar rune literals and escapes.
`conformance/integers.md` covers integer bases and prefix validation.
`conformance/floats.md` covers decimal fractions and scientific notation.
`conformance/keywords.md` covers MVP keywords and future-reserved words.
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
These documents do not count as passing tests.

Use Rust unit tests for small source/IR utilities and pass algorithms. Use Cargo
integration tests in top-level `tests/*.rs` for public compiler APIs and CLI
subprocess behavior. A directory of fixtures alone is not an executable test.

Introduce fixture suites as the corresponding stage is implemented:

| Suite | Check |
| --- | --- |
| Lexer | Token kind/text/span; invalid input, UTF-8, EOF, recovery progress |
| Parser | AST structure/spans; syntax rejection and recovery |
| Semantic | Resolution/types; diagnostics with primary and related locations |
| Ownership | Copy/Move, projected places, aliases, branches, explicit drop, use-after-move, double drop, task/await borrow validity |
| Native | Compile, execute, compare stdout/stderr/exit status; cleanup on each control-flow path |
| Async/runtime | Suspension/resume, live values, destruction, results/errors, detachment, buffering, close/drain, message transfer |

When a fixture runner is added, place inputs under `tests/fixtures/<suite>/` and
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
