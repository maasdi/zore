# Implementation roadmap

M0–M4 are complete, and M9–M10 (resolution and type checking) are implemented
for an initial subset: `zore check <file.ore>` runs lex → parse → resolve →
type-check and accepts the §42 semantic target and the hello example. The
subset is synchronous, single-file, and all-Copy (bool, integers, rune, string,
structs of those); everything else is reported as unsupported. No MIR, ownership
analysis, or code generator exists yet, so nothing can be built or run. Linux/macOS CI is configured but has not run remotely.
The user has ended broad specification preparation: resolve further language
questions only when they block the active implementation milestone. No currently
recorded language question blocked M0–M4; Q13 and Q14 record the conservative
lexer and parser choices made where the spec is silent. Q15 is resolved by
§6.7 (Go's untyped-constant model), which the checker does not implement yet. Numbers refer to spec §43; the sequence
is guidance, not a language contract.

| Milestone | Deliverable and acceptance criteria |
| --- | --- |
| M0 — complete | CLI/driver: help, version, argument validation, honest unsupported-command errors; subprocess tests for exit status and output. Prioritize `check <file.ore>`. |
| M1 — complete | Source manager, file IDs, byte spans, diagnostic rendering; test empty input, UTF-8 boundaries, line endings, EOF, and multiple files. |
| M2 — complete | Tokens and lexer for agreed lexical rules; test spans, valid tokens, invalid input, EOF, and progress after errors. Q01 lexical choices are resolved; use the locked rules. |
| M3–M4 — complete | AST and parser together for package/functions/structs/bindings/calls; test shape, spans, recovery, and rejection. Resolve relevant grammar questions first. |
| M5–M8 | Hello program, variables, functions, structs. Establish the minimal native backend and builtin output support needed to run examples. Use resolution/type work below as prerequisites where needed. |
| M9–M12 — M9–M10 initial subset done | Name resolution, types, HIR, MIR/CFG; semantic IDs, typed calls/fields, explicit control flow, frontend-only checking. |
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

## Next implementation session

The semantic checkpoint now passes `check`; its remaining requirement is native
execution printing `Maas`. Next, start the minimal native path (M5–M8 with
M11–M12): first record the backend decision (LLVM version, binding strategy,
host targets, runtime ABI, and setup/test instructions) under
`docs/decisions/` as `docs/architecture.md` requires, then lower HIR to MIR/CFG
with explicit Copy operations and control flow, and emit a native executable
with a minimal runtime `println`. Keep `check` independent of the backend.

The checker now lags the spec in one known way: §6.7 (Q15) locks Go's
untyped-constant model, but the checker still limits untyped integers to 128
bits (below the required 256), has no untyped float kind, and reports untyped
`/`, `%`, bitwise operators, and floats as unsupported. It never accepts an
invalid program because of this, but implementing §6.7 (arbitrary-precision
constants, untyped float kind, representability rounding) is required before
floats are accepted. It needs a big-number decision: hand-written or a crate
such as `num-bigint`, justified per AGENTS.md.

Alternatively, widen the checker (methods, `error`/`?`, §6.7 constants and
floats), keeping every widening paired with rejection tests. Ownership
analysis (M13–M17) must land before any Move type or `mut` parameter is
accepted: the checker's all-Copy restriction is what currently makes ownership
checks vacuous, and `typeck` enforces it.

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
successful semantic check and eventual native execution printing `Maas`, with
spans and diagnostics retained throughout. A parser-only pass is insufficient.
The semantic check now passes (`tests/check.rs`, `tests/cli.rs`); native
execution is pending.
Because its `User` contains only a Copy string, add a separate Move-resource test
when available to prove that ordinary calls borrow rather than consume values.

## Completion policy

Mark milestones complete only with working behavior and relevant automated tests.
Track temporary restrictions and their spec basis. Do not count pending fixtures
as passing tests. Full MVP includes async, tasks, channels, runtime, native output,
and diagnostics; completing the synchronous subset does not complete the MVP.
