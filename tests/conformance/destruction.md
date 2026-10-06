# Drop, clone, and panic-unwinding conformance cases

Authority: spec §8.3, §10.7, §14.1–14.5, §15.4. Drop signatures and Move
classification have executable type-checking coverage in
`tests/typecheck/check.rs`; explicit-drop consumption and double-drop
rejection are covered in `tests/ownership/ownership.rs`. Clone
availability, the built-in clones, and cleanup after a panicking clone have
coverage in `tests/typecheck/check.rs`, `tests/ownership/ownership.rs`, and
`tests/codegen/native.rs`. Automatic cleanup and panic unwinding remain pending
runtime coverage. Field-level
partial-move cleanup — dropping only still-available fields, skipping a
moved-out field that was never reinitialized, and rejecting a move that would
leave a custom-`drop`-bearing value incomplete — has coverage in
`tests/ownership/ownership.rs` and `tests/codegen/native.rs`.

## Drop receiver form (§14.3)

| Scenario | Expected result |
| --- | --- |
| `func (c mut Connection) drop() { ... }` | Valid: `mut` receiver |
| `func (c Connection) drop() { ... }` (plain/shared receiver) | Reject: `drop` must use `mut` |
| `func (c own Connection) drop() { ... }` | Reject: `drop` must use `mut`, not `own` |
| `func (c mut Connection) drop() int { return 0 }` | Reject: `drop` returns no result |
| `func (c mut Connection) drop(reason string) { ... }` | Reject: `drop` takes no parameters |
| `drop()` body calling `c.socket.close()` where `close()` has a `mut` or shared receiver | Valid |
| `drop()` body calling `c.socket.close()` where `close()` has an `own` receiver | Reject: cannot move a field out of a `mut` receiver |
| `drop()` body assigning `c.state = "closed"` | Valid: `mut` receiver permits field mutation |
| Custom `drop()` body runs, then the compiler drops each field automatically afterward | Expected: both occur, in that order |
| `connection.drop()` called directly as an ordinary method call | Reject: the user-defined `drop` method cannot be called directly |
| `drop(connection)` (builtin) | Valid: consumes `connection`; may not be used again |
| `drop(connection)` called twice on the same binding | Reject: double drop |

## Copy/Move interaction (§8.3)

| Scenario | Expected result |
| --- | --- |
| `type Handle struct { id int }` with a custom `drop` method, `id` is Copy | `Handle` is Move, despite all-Copy fields |
| Same `Handle`, assignment `let b = a` | Move, not Copy: `a` is unusable after |
| A struct with all-Copy fields and no custom `drop` | Copy, per the unmodified §8.3 rule |

## Clone availability (§10.7)

| Scenario | Expected result |
| --- | --- |
| `type Point struct { X int; Y int }`, no custom `drop`/`clone`, `clone(p)` | Valid: structural default clone |
| `clone(a)` followed by using both `a` and the clone | Valid: `clone` does not consume `a` |
| A struct with a custom `drop` and no custom `clone`, `clone(v)` | Reject: not clonable |
| A struct with a custom `drop` and a custom `func (c Type) clone() Type` | Valid: custom clone used, structural default not attempted |
| A struct with no custom `drop`, all fields Copy-or-clonable, plus a custom `clone` method | Valid: custom clone takes precedence over the structural default |
| A struct containing a field of a type with no eligible clone (drop-bearing, no custom clone), no custom `drop`/`clone` on the outer struct | Reject: outer struct is not structurally clonable |
| `clone(arr)` where `arr Array<Point>` (`Point` structurally clonable) | Valid: built-in element-wise clone |
| `clone(arr)` where `arr Array<Connection>` (`Connection` has `drop`, no `clone`) | Reject: element type is not clonable |
| `clone(m)` where `m map[string]Point` | Valid: built-in entry-wise clone |
| `value.clone()` called directly via method-call syntax | Valid: no double-invocation hazard, unlike `.drop()` |
| `[T; N]` array of a structurally clonable `T`, no custom `drop` on the array's element type | Valid: element-wise structural default |
| `copy(value)` (generic copy builtin) | Reject: not part of the MVP |
| `clone(1)`, `clone("s")`, `clone(err)`, a slice, or a closure | Reject: clone applies to structs, fixed arrays, `Array<T>`, and maps (Q19) |
| `clone()` or `clone(a, b)`, or `clone` used as a value | Reject |
| Custom `clone` with a `mut` or `own` receiver, parameters, or a result other than its own type | Reject at the declaration |
| `value.clone()` on a type without a custom `clone` | Reject, noting `clone(value)` (Q19) |
| `clone(v)` of a value holding views, then writing the viewed owner while the clone is live | Reject: the clone keeps the backing borrowed |
| A clone is independent: changing the source afterwards leaves the clone unchanged | Valid |
| Clone of a struct, `Array<T>`, fixed array, or map of resources with a custom `clone` | Each part cloned once; the clone and the source are each dropped once |
| A custom `clone` panics partway through a struct, fixed array, `Array<T>`, or map | Parts already cloned are dropped once, storage is freed, then the source unwinds as usual |

## Panic and unwinding (§15.4)

| Scenario | Expected result |
| --- | --- |
| `panic("message")` in the initial task with a live owned resource on the stack | Runtime unwinds, running the resource's `drop`, then the process terminates |
| Nested function calls each holding an owned resource, then a panic | Each frame's owned resources are dropped in reverse acquisition order (§14.5) during unwind |
| A `drop` invoked during unwinding itself calls `panic()` | Runtime aborts the whole process immediately; no nested unwind is attempted |
| Attempting to catch a panic (`try`/`catch`/`recover`-style construct) | Reject: no such construct exists in the MVP |
| A function that both returns `(T, error)` and may `panic()` | The two are independent: `error` results remain the ordinary, catchable failure channel; panic is unrelated and non-catchable |
| Panic inside a `go`-spawned task | Only that task unwinds; see `tests/conformance/concurrency.md` (§18.10) |

These cases test drop/clone/panic semantics within one task. Cross-task panic
containment, re-raising at retrieval, and reporting are covered in
`tests/conformance/concurrency.md`.

## Complete receivers and empty resources (§31.2, §41.4)

| Scenario | Expected result |
| --- | --- |
| Partially move out of a value with custom drop | Reject before lowering; destructor always requires an intact value |
| Move whole custom-drop value, then source scope exits | Source does not drop it; destination owns the complete receiver |
| Destructor receives a recursively zero-produced resource | Must complete harmlessly; compiler does not skip custom drop |
| Destructor-free aggregate partially moved before `?` or panic | Cleanup drops only remaining initialized fields |
| Field replacement invalidates a field needed by a containing destructor, then old-field destruction panics | Abort; no destructor observes an incomplete receiver |

Detailed provenance and replacement cases are in `ownership.md`; resource
acquisition/release accounting cases are in `zero-values.md`.

## Custom clone precedence (§10.7, Q12)

| Scenario | Expected result |
| --- | --- |
| Struct is structurally clonable and also declares valid custom clone | `clone(value)` invokes the custom method exactly once |
| Point example in §10.7 | Prints custom clone once, then 7 and 7; source remains available |
| Same eligible struct has no custom clone | Structural default applies |
| Struct declares clone with wrong receiver, arguments, or result type | Reject invalid custom declaration; no structural fallback |
| Selected custom clone fails ordinary borrow checking | Reject; no structural fallback |
| Custom clone panics while owning a temporary resource | Ordinary unwind cleanup; no structural retry |
| Custom clone called via `value.clone()` | Same user method and its observable behavior |

These cases refine dispatch expectations; resource acquisition correctness and
borrow provenance remain subject to §11.7 and §41.4.
