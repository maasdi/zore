# Ownership and destruction coverage inventory

Audit baseline: `main` at `1a98b74f3f0f413db21f9be0e6ef9835abede13e`.
Scope: specification §§10, 11, and 14, with §§39.2 and 46.4–46.5 as
coverage context. **Covered** means the linked cases assert the stated behavior,
not that the implementation is sound for every program. **Partial** means only
part of the requirement has executable evidence. **Unsupported** means the
compiler rejects a specified form. **Unverified** means no targeted executable
assertion was found. **Blocked by an unresolved specification decision** is
reserved for an explicitly undecided rule. The scenario lists in
`tests/conformance/` are not executable evidence.

The linked frontend tests use `accepts` to assert successful checking and
`rejects` to assert a diagnostic substring and no checked package. Native
`prints` asserts successful exit, exact stdout, and empty stderr; `panics`
asserts failure, a diagnostic substring, and exact stdout before failure.
These helpers are defined in [ownership.rs](../../tests/ownership/ownership.rs#L34)
and [native.rs](../../tests/codegen/native.rs#L9). N/A means the column does
not apply to that particular requirement, with the reason given in the cell.

| Spec | Requirement | Status | Implementation path / symbol | Executable acceptance | Executable rejection, panic, or cleanup | Specific remaining gap |
| --- | --- | --- | --- | --- | --- | --- |
| §10.1 | Assignment copies Copy values and transfers Move values. | covered | [`Package::is_copy`](../../compiler/src/hir/mod.rs#L32), [`ownership::checker::transfer`](../../compiler/src/ownership/checker.rs#L107) | [ordinary calls borrow and `own` consumes](../../tests/ownership/ownership.rs#L56); [Copy struct remains usable](../../tests/typecheck/check.rs#L704) | [use after `own` and double drop rejected](../../tests/ownership/ownership.rs#L56) | No gap in the identified whole-value cases; this is not exhaustive across all types. |
| §§10.2–10.3 | Primitives and string are Copy; resource-owning structs are Move; channel and mutex handles are Copy. | partial | [`Package::is_copy`, `holds_shared`](../../compiler/src/hir/mod.rs#L32) | [drop-bearing struct and Copy struct classification](../../tests/typecheck/check.rs#L704); [channel handles](../../tests/typecheck/check.rs#L3327); [mutex handles](../../tests/typecheck/check.rs#L3559); [shared string copies](../../tests/codegen/native.rs#L2735) | [use after moving a resource](../../tests/ownership/ownership.rs#L56) | No single executable matrix checks every listed primitive and resource kind, including file/socket ownership transfer. |
| §10.4 | Fixed arrays inherit Copy/Move classification from their elements. | partial | [`Package::is_copy`](../../compiler/src/hir/mod.rs#L32), [`ownership::checker`](../../compiler/src/ownership/checker.rs#L63) | [Copy array indexing and use](../../tests/typecheck/check.rs#L1488); [Move element cleanup](../../tests/codegen/native.rs#L385) | [moving an indexed Move element rejected](../../tests/ownership/ownership.rs#L193) | Add a direct Copy-after-assignment and Move-after-assignment pair for fixed arrays. |
| §10.5 | `Array<T>` owns storage and moves as a whole. | covered | [`Package::is_copy`](../../compiler/src/hir/mod.rs#L32), [`dropck::insert`](../../compiler/src/dropck/insertion.rs#L7) | [whole-array moves](../../tests/ownership/ownership.rs#L736); [reverse-order element cleanup](../../tests/codegen/native.rs#L531) | [element extraction rejected](../../tests/ownership/ownership.rs#L753); [construction and replacement panic cleanup](../../tests/codegen/native.rs#L600) | No gap in these identified cases; recursive owned elements are noted below. |
| §10.6 | Maps own their storage and move as a whole. | covered | [`Package::is_copy`](../../compiler/src/hir/mod.rs#L32), [`dropck::insert`](../../compiler/src/dropck/insertion.rs#L7) | [whole-map move and remove](../../tests/ownership/ownership.rs#L875); [value cleanup](../../tests/codegen/native.rs#L695) | [moved source rejected](../../tests/ownership/ownership.rs#L875); [literal failure cleanup](../../tests/codegen/native.rs#L724) | No gap in the identified whole-map and cleanup cases. |
| §10.7 | `clone` borrows its input; eligible structural/built-in and selected custom clones create an independent owner. | partial | [`Checker::clone_call`, `custom_clone`, `clone_blocker`](../../compiler/src/hir/lower.rs#L2977), [`clone_call` code generation](../../compiler/src/codegen/clone.rs#L6) | [input remains usable](../../tests/ownership/ownership.rs#L1215); [independent collection copies](../../tests/codegen/native.rs#L1920); [custom precedence](../../tests/codegen/native.rs#L1972) | [ineligible parts](../../tests/typecheck/check.rs#L2454); [invalid signature](../../tests/typecheck/check.rs#L2511); [partial clone panic cleanup](../../tests/codegen/native.rs#L2096) | Add a focused assertion that a clone containing shared views preserves provenance through a later call, beyond the direct local case. |
| §§11.1–11.2 | Plain parameters borrow; `mut` permits mutable borrowing; `own` transfers ownership. | covered | [`hir::lower::Checker`](../../compiler/src/hir/lower.rs#L147), [`ownership::checker`](../../compiler/src/ownership/checker.rs#L63) | [plain versus `own` calls](../../tests/ownership/ownership.rs#L56); [`mut` caller update](../../tests/codegen/native.rs#L1052) | [moving from plain parameter rejected](../../tests/ownership/ownership.rs#L56); [immutable caller rejected](../../tests/typecheck/check.rs#L622) | No gap in the identified parameter modes. |
| §11.3 | An active mutable loan excludes overlapping shared/mutable access and moves. | covered | [`region::check_access`](../../compiler/src/ownership/region.rs#L1360), [`checker::places_overlap`](../../compiler/src/ownership/checker.rs#L291) | [sequential and shared borrows](../../tests/ownership/ownership.rs#L271) | [overlapping mutable views](../../tests/ownership/ownership.rs#L240); [owner move/replace/drop while viewed](../../tests/ownership/ownership.rs#L545) | No gap in the identified array/slice cases; all alias shapes are not enumerated. |
| §§11.4–11.5 | Regions and last-use are inferred without source lifetime parameters; the analysis strategy may vary. | partial | [`region::Liveness::compute`](../../compiler/src/ownership/region.rs#L1594), [`region::check`](../../compiler/src/ownership/region.rs) | [owner usable after view's last use](../../tests/ownership/ownership.rs#L300) | [view use after conflicting write rejected](../../tests/ownership/ownership.rs#L300) | No focused parser rejection of source lifetime syntax; §11.5's preferred algorithm is design guidance, not a separate observable behavior. |
| §11.6 | A `mut` argument or receiver must originate from a mutable place. | covered | [`Checker::mutable_place`, `writable_place`](../../compiler/src/hir/lower.rs#L2825) | [mutable local, field, receiver](../../tests/typecheck/check.rs#L622); [mutable slice element](../../tests/typecheck/check.rs#L1676) | [immutable place and shared parameter](../../tests/typecheck/check.rs#L622); [shared slice element](../../tests/typecheck/check.rs#L1676) | No gap in the identified place categories. |
| §11.7 | Returned views retain inferred input provenance across branches, recursion, and aggregate nesting. | partial | [`region::return_origins`](../../compiler/src/ownership/region.rs#L1512), [`region::output_origins`](../../compiler/src/ownership/region.rs#L1543) | [parameter-backed and nested struct returns](../../tests/ownership/ownership.rs#L373); [two-input precision](../../tests/ownership/ownership.rs#L413); [recursive contracts](../../tests/ownership/ownership.rs#L438) | [local, temporary, `own`-backed returns rejected](../../tests/ownership/ownership.rs#L373); [ambiguous branch constrains both inputs](../../tests/ownership/ownership.rs#L413) | Add direct result-provenance cases for nested map/array results and a self-referential owner-plus-view return. |
| §11.7 | Stored views and reborrows retain backing loans through copies, moves, collections, captures, and a destructor's last use. | partial | [`region::store_views`, `inherited_loans`, `check_drop_order`](../../compiler/src/ownership/region.rs#L274) | [mutable descriptor reborrow ends at last use](../../tests/ownership/ownership.rs#L347); [views in arrays](../../tests/ownership/ownership.rs#L821); [views in maps](../../tests/ownership/ownership.rs#L898); [observing destructor](../../tests/ownership/ownership.rs#L1365) | [backing mutation while stored view lives](../../tests/ownership/ownership.rs#L898); [invalid drop order](../../tests/ownership/ownership.rs#L1442) | No direct test of a destructor-observed stored view across an async suspension. |
| §14.1 | Owned resources are destroyed automatically at lifetime end. | covered | [`dropck::insert_body`](../../compiler/src/dropck/insertion.rs#L13), [`Package::needs_drop`](../../compiler/src/hir/mod.rs#L76) | [scope and return cleanup](../../tests/codegen/native.rs#L207) | [panic unwind cleanup](../../tests/codegen/native.rs#L248); [error propagation cleanup](../../tests/codegen/native.rs#L118) | No gap in these normal/error/panic exits; this does not prove all control-flow paths. |
| §14.2 | Cleanup may follow last safe use while preserving deterministic destruction. | partial | [`dropck::insert_body`](../../compiler/src/dropck/insertion.rs#L13), [`region::Liveness::compute`](../../compiler/src/ownership/region.rs#L1594) | [loan ends at last use](../../tests/ownership/ownership.rs#L300) | [scope, branch, and loop cleanup](../../tests/codegen/native.rs#L230) | No executable assertion distinguishes early owned-value destruction from scope-end destruction; early drop timing is an optimization direction, not a required schedule. |
| §14.3 | Valid `mut drop()` runs once before automatic recursive field cleanup; its receiver stays complete. | partial | [`Resolver::check_drop_signature`](../../compiler/src/resolve/resolver.rs#L471), [`dropck::insert_body`](../../compiler/src/dropck/insertion.rs#L13) | [valid signature and Move classification](../../tests/typecheck/check.rs#L704); [custom body then fields](../../tests/codegen/native.rs#L290) | [invalid signature and direct call](../../tests/typecheck/check.rs#L704); [partial move through custom-drop ancestor](../../tests/ownership/ownership.rs#L134) | No focused test attempts to move a field from inside its own `drop` body; the linked partial-move rejection is outside the destructor. |
| §14.4 | `drop(value)` consumes the value and prevents a second destruction. | covered | [`Checker::drop_call`](../../compiler/src/hir/lower.rs#L2962), [`dropck::insert_body`](../../compiler/src/dropck/insertion.rs#L13) | [explicit destruction prints once](../../tests/codegen/native.rs#L207) | [second drop rejected](../../tests/ownership/ownership.rs#L56) | No gap in the identified simple value case. |
| §14.5 | Fields and array elements clean up in reverse applicable order. | covered | [`dropck::insert_body`](../../compiler/src/dropck/insertion.rs#L13), [`drop_contents`](../../compiler/src/codegen/llvm.rs#L1469) | [struct body/field order](../../tests/codegen/native.rs#L290); [fixed array](../../tests/codegen/native.rs#L385); [dynamic array](../../tests/codegen/native.rs#L531) | [panic during unwind aborts](../../tests/codegen/native.rs#L266); [partially moved field skipped](../../tests/codegen/native.rs#L337) | No gap in the identified aggregate orders. |
| §14.6 | Ownership transfer leaves destruction to the new owner; partial moves skip the removed part. | covered | [`MovedSet::record_move`](../../compiler/src/ownership/move_state.rs#L34), [`dropck::insert_body`](../../compiler/src/dropck/insertion.rs#L13) | [transfer and scope cleanup](../../tests/codegen/native.rs#L207); [reinitialization drops both owners once](../../tests/codegen/native.rs#L322) | [uninitialized partial field skipped](../../tests/codegen/native.rs#L337); [use-after-move rejected](../../tests/ownership/ownership.rs#L56) | No gap in the identified transfers. |
| §14.7 | A value being destroyed cannot be resurrected through `drop`. | unverified | [`Resolver::check_drop_signature`](../../compiler/src/resolve/resolver.rs#L471), [`ownership::checker`](../../compiler/src/ownership/checker.rs#L63) | N/A: prohibition has no valid resurrection case. | [moving out of custom-drop receiver is rejected](../../tests/ownership/ownership.rs#L134), but this is not a direct resurrection test. | Add a direct attempted escape from `drop` and assert its diagnostic. |
| §14.8 | `defer` is undecided and is not required for resource safety. | blocked by an unresolved specification decision | N/A: no locked `defer` implementation requirement. | N/A: no accepted syntax to exercise. | N/A: no locked form to reject here. | Decide `defer` separately if proposed; automatic cleanup already has tests above. |

## Review-baseline note: recursive owned types

[`Resolver::reject_self_containing_structs`](../../compiler/src/resolve/resolver.rs#L594)
rejects `type Node struct { Children Array<Node> }` because drops are emitted
inline and need out-of-line drop functions for this finite-size, recursively
owned type. The current diagnostic is asserted in
[`unsupported_dynamic_array_forms_are_rejected`](../../tests/typecheck/check.rs#L1889).
This is distinct from direct by-value recursion, which has no finite size and
is rejected by [`name_errors_are_reported`](../../tests/typecheck/check.rs#L205)
and [`self_containing_array_structs_are_rejected`](../../tests/typecheck/check.rs#L1545).
This inventory neither relaxes the rejection nor proposes new syntax.

Issue #51 follow-up: the implementation now accepts finite recursive ownership
through `Array<T>` and maps, using cached out-of-line destruction and clone
helpers. The paragraph above records the audit baseline, not the current
restriction. Current invariants and limits are in
[the architecture's recursive-type audit](../architecture.md#recursive-owned-types);
the `recursive_*` tests in the type-check, ownership, and native suites cover
the new support. The original validation counts below remain historical.

## Follow-up coverage added by issue #76

The five candidates below were added after the baseline. Each test passed on
`main` at `f164889` without a compiler change, so none exposed a defect. The
baseline rows above are kept as written; this section records what changed.

| Candidate | Test | Asserts |
| --- | --- | --- |
| 1. Fixed-array Copy/Move assignment | [`fixed_arrays_copy_or_move_on_assignment_by_their_element_type`](../../tests/ownership/ownership.rs#L1889) | A `[int; 2]` source stays usable after `let copy = source`; a `[Resource; 1]` source is rejected with ``use of moved value `source[_].id` `` at `return source[0].id`, and the new owner is accepted. |
| 2. Owner returned with its own view | [`an_owner_cannot_be_returned_alongside_its_own_view`](../../tests/ownership/ownership.rs#L1926) | `return values, view` is rejected with ``cannot return a view of local `values` `` at `values[:]` and ``cannot move `values` while it is borrowed`` at the return; returning the owner with a copied element is accepted. |
| 3. Resurrection from `drop` | [`a_custom_drop_cannot_export_the_value_being_destroyed`](../../tests/ownership/ownership.rs#L1957) | `let escaped = r` and `r.sink.send(r)` inside a custom `drop` are rejected with ``cannot move borrowed value `r` `` at the escaping expression; a `drop` that only reads is accepted. |
| 4. Destructor-observed view across suspension | [`a_drop_that_reads_a_view_keeps_the_backing_owner_live_across_suspension`](../../tests/codegen/native.rs#L7133) and [`a_drop_that_reads_a_view_still_runs_when_a_task_panics_after_suspension`](../../tests/codegen/native.rs#L7170) | A `Watch` whose `drop` reads a view of a fixed array and of an `Array<int>` keeps both owners live through a channel suspension. Output order shows the view readable after resume (`before`, `11`, `8`) and each `drop` reading its backing exactly once at cleanup (`7`, `5`, then the result `6`). On a panic after resume, the `drop` still reads its backing before the task panic is reported (`5`, `70`, exit status 2). |
| 5. Unapproved `copy(value)` | [`an_unapproved_copy_call_is_an_unknown_name`](../../tests/typecheck/check.rs#L4001) | ``cannot find `copy` in this scope`` at `copy`; no new builtin or syntax; `clone(values)` stays accepted. |

Retained gaps after this work:

- The §14.7 rejection is tested for two escape routes (a local binding and a
  channel send on a field). Other routes, such as storing into a collection field,
  rely on the same borrowed-receiver rule and have no separate test.
- The async case covers a channel suspension. Suspension through timers, mutexes,
  and I/O is not separately asserted for destructor-observed views.
- Row 5 asserts the diagnostic only; it does not decide whether `copy` should ever
  exist.

## Validation

Run on 2026-10-08 from base commit
`1a98b74f3f0f413db21f9be0e6ef9835abede13e`, with this document as the
only working-tree change. Native tests used the repository's pinned Rust
toolchain and clang with LLVM 15 or newer. Every command exited successfully:

| Command | Result |
| --- | --- |
| `cargo test --locked --test ownership` | 57 passed, 0 failed |
| `cargo test --locked --test check` | 102 passed, 0 failed |
| `cargo test --locked --test native` | 209 passed, 0 failed |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo build --locked` | Passed |
| `cargo test --locked --all-targets` | Passed, including 57 ownership, 102 type-check, 209 native, and 66 runtime tests |

## Focused follow-up candidates

1. In `tests/ownership/ownership.rs`, add a fixed-array Copy/Move assignment pair; expect the Copy source usable and the Move source rejected after transfer.
2. In `tests/ownership/ownership.rs`, return an owner alongside its own view; expect rejection of the self-referential result under §11.7.
3. In `tests/ownership/ownership.rs`, attempt to export a resource from its custom `drop`; expect a diagnostic that prevents resurrection.
4. In `tests/codegen/native.rs`, exercise a destructor-observed view stored across an async suspension; expect the backing owner to remain live through resume and cleanup.
5. In `tests/typecheck/check.rs`, call the unapproved `copy(value)` form; expect a clear unknown-builtin or unknown-function diagnostic.
