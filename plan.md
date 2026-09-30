# Ownership and drop plan (M13–M19)

Status: slices A and B merged; slice C CI-validated on its feature branch,
pending review and merge. This plan covers the first Move type and deterministic
cleanup. `docs/roadmap.md` stays the source of milestone status.

## Why this order

`drop` requires a `mut` receiver (§14.3), so the first Move type cannot come
first. `mut` parameters and receivers need caller-mutability and exclusivity
checks (§11.3, §11.6). Ownership analysis and cleanup must both land before any
Move type is accepted by `zore check` or `zore build`.

## Workflow

- Each slice is its own branch and its own pull request. Do not push slices to
  `main` directly.
- Before each push run `cargo fmt --all -- --check`,
  `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo build --locked`, and `cargo test --locked --all-targets`; where the
  toolchain is unavailable locally, CI on the pull request is the check.
- Each slice adds paired acceptance and rejection tests, updates the roadmap
  status honestly, and records unresolved spec gaps in
  `docs/spec-questions.md` rather than inventing syntax.
- Pending conformance cases live in `tests/conformance/ownership.md` and
  `tests/conformance/destruction.md`; they count as passing only once
  executable tests cover them.

## Slice A — `mut` parameters and receivers (§11.2, §11.3, §11.6)

- Mutable-place classification: a `var` local, a field of a mutable place, or a
  place reached through a `mut` parameter or receiver. Reject `let` bindings and
  shared-borrowed places passed to `mut` positions, naming the declaring
  binding.
- Allow assignment through `mut` parameters and receivers. Keep rejecting
  assignment to shared parameters.
- Exclusivity for one call: reject overlapping arguments where one is `mut`
  (for example `f(x, x)`). Borrows live only for the call until slices or
  stored references exist.
- MIR gains a reference operand (§33.3); `mut` arguments are passed by pointer.
  Read `codegen/llvm.rs` and `mir/lower.rs` in full before designing it.
- Remove the `mut` parameter and receiver rejections in the resolver.
- Tests: `tests/typecheck/check.rs` accept/reject pairs from the mutable-place
  table; native tests that mutate through a `mut` parameter and receiver.

## Slice B — `drop` declaration and Move classification (§8.3, §14.3, §14.4)

- Validate `drop`: `mut` receiver, no parameters, no result, defined on a
  struct declared in the package.
- A struct with `drop` is always Move; record this in HIR classification
  instead of deriving from fields alone.
- Reject direct `value.drop()` calls.
- Add the test-only `check_file_allowing_move_types` entry point so Move types
  can be tested while `zore check` and `zore build` keep rejecting them. The
  builtin `drop(value)` moves to slice C, since consuming a value needs move
  tracking.
- Tests from the drop receiver form and Copy/Move interaction tables in
  `tests/conformance/destruction.md`.

## Slice C — ownership analysis (§30, §31)

- A separate MIR dataflow pass; it answers whether a value is live and legally
  usable, not where destruction happens (§34.1).
- Per-place state: `Available`, `Moved`, `PartiallyMoved`. The data model must
  represent partial moves; the first version rejects them conservatively
  (§31.2).
- Diagnostics: use after move, including inside loops and where branches
  disagree; moving a borrowed value; whole use of a partially moved value. Each
  names the move site and the later use.
- Assignment to a moved place reinitialises it. `let b = a` and `own`
  arguments move Move types; ordinary calls borrow.
- Add the Move-resource test the roadmap requires, proving ordinary calls
  borrow instead of consume.
- Add the builtin `drop(value)`: it consumes its argument, and a second use or
  a second `drop` is a use-after-move error (§14.4).
- Gate: `zore check` and `zore build` keep rejecting Move types. The test-only
  entry point from slice B is not exposed through the CLI and is removed by
  slice D.

## Slice D — drop insertion and cleanup (§14, §15.3, §15.4, §34)

- Explicit MIR `Drop` statements, placed by a pass separate from ownership
  checking.
- Drop at scope end in reverse declaration order; on `return`; on `break` and
  `continue`; for `own` parameters not moved; for temporaries. Conditionally
  moved values use drop flags.
- A custom `drop` body runs first, then each field is dropped in reverse
  declaration order (§14.3, §14.5).
- Runtime checks (overflow, division, shifts, conversions) become MIR assert
  terminators with cleanup edges instead of panicking from generated code.
- Panic unwinding runs pending drops in every frame (§15.4). Proposed
  mechanism: a panic status flag checked after each call, branching to cleanup
  blocks. It is independent of LLVM exception tables and reusable for async
  state machines. A panic during a drop that runs while unwinding aborts the
  process.
- Remove the test-only gate; `zore check` and `zore build` accept Move types.
- Tests: exactly-once cleanup on normal, branch, early-return, loop-exit and
  panic paths, checked by native tests that print from `drop`.

## Open decisions

1. Panic unwinding: the status-flag mechanism above is the recommended default.
   The alternative is LLVM `invoke` with a landing pad, which needs a
   personality function and an unwinder in the runtime. Confirm before slice D.
2. The test-only Move-type gate is the chosen slice C behavior. CLI checking
   and builds retain the gate until slice D cleanup is implemented.

## Not in scope

Partial-move acceptance beyond the conservative rejection, borrowed slices and
return-borrow contracts (M20 onward), `error` and `?` cleanup paths (M19),
closures, tasks, and async cleanup. Each keeps its own roadmap milestone.
