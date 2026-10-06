# Return-borrow, caller-mutability, partial-move, and slice-aliasing conformance cases

Authority: spec §5.6, §11.3, §11.6–11.7, §12.5, §14.6, §30.1, §31.2.
`tests/typecheck/check.rs` covers mutable places, call-local exclusivity, and
slice-element mutability through shared and mutable views.
`tests/ownership/ownership.rs` covers whole-place moves, branch and loop move
state, field-level partial moves, reinitialization restoring whole-value
usability, the custom-`drop`-ancestor restriction, and stored slice borrows:
slice aliasing at the originating place, mutable-descriptor reborrows,
return-borrow contracts (including two-input precision, recursion, and zero
slices from `?`), and recursive provenance through structs.
`tests/codegen/native.rs` covers the runtime cleanup of partial moves.
`mut []T` held in struct fields and fixed arrays has coverage in the check,
ownership, and native suites (Q20).
Destructor-observed views have coverage in the ownership and native suites
(Q21).
Pending: rows needing `await` or tasks; and mutable views inside `Array<T>`,
map, or slice elements and views stored through parameters or captures
(rejected for now as unsupported).

## Mutable place requirements for callers (§11.6)

| Scenario | Expected result |
| --- | --- |
| `var user = loadUser(); rename(user, "Alice")` where `rename(user mut User, ...)` | Valid: `user` is a `var` binding, a mutable place |
| `let user = loadUser(); rename(user, "Alice")` | Reject: `user` is a `let` binding, not a mutable place |
| `var container = Container{...}; rename(container.inner, "Alice")` | Valid: field of a mutable place is mutable |
| `let container = Container{...}; rename(container.inner, "Alice")` | Reject: field of an immutable place is not mutable |
| Function receives `container User` (shared borrow) and calls `rename(container.inner, ...)` | Reject: reached only through a shared borrow |
| Function receives `container mut Container` and calls `rename(container.inner, ...)` | Valid: reached through a `mut`-borrowed parameter |
| `var user = loadUser(); user.rename("Alice")` where `rename` has a `mut` receiver | Valid: receiver place is mutable |
| `let user = loadUser(); user.rename("Alice")` where `rename` has a `mut` receiver | Reject: receiver place is not mutable |
| Passing a shared `[]T` element to a function expecting `mut` access to it | Reject: element reached through a shared slice is not mutable |
| Passing a `mut []T` element's place onward as a `mut` argument | Valid: element of a mutable slice is mutable |

## Return-borrow contracts (§11.7)

| Scenario | Expected result |
| --- | --- |
| `func firstHalf(s mut []int) mut []int { return s }` | Valid: returned value backed by borrowed parameter `s` |
| A function returning `[]T` built from a function-local owned `Array<T>` | Reject: local storage does not survive return |
| `var data = loadData(); let half = firstHalf(data[:]); use(data)` while `half` is live | Reject: caller's continued use of `data` conflicts with the live returned borrow, per §11.3 |
| `var data = loadData(); let half = firstHalf(data[:]); use(half)` with no further use of `data` while `half` is live | Valid |
| A function with two `[]T` parameters returning `[]T` provably derived from only one of them | Valid: region bound to the one it actually derives from |
| A function returning a nonzero borrowed view whose external backing provenance cannot be proven | Reject rather than guess |
| Returning a built-in zero slice with no backing loan | Valid without a borrowed parameter |
| Returned slice held across an `await` where validity cannot be proven | Reject, per §17.6 |

## Partial moves and reinitialization (§31.2)

The acceptance cases below assume no containing value on the moved projection
path defines custom `drop`; exceptions are tested separately below. Field
reinitialization also requires a mutable containing place.

| Scenario | Expected result |
| --- | --- |
| `let inner = outer.field` (Move-typed field), then read a different still-available field of `outer` | Valid |
| `let inner = outer.field`, then read `outer.field` again | Reject: use after move |
| `let inner = outer.field`, then pass `outer` whole to a function by borrow or by move | Reject: whole value unusable while partially moved |
| `let inner = outer.field`, then `outer.field = newValue`, then use `outer` as a whole | Valid: reinitializing the only moved field restores whole-value availability |
| `let inner = outer.field`, then `outer.field = newValue` | No drop runs for the old field value (already moved out) |
| `outer` fully reinitialized then goes out of scope | Ordinary full cleanup runs, per §14.5 |
| `let inner = outer.field` and `outer` goes out of scope without reinitialization | Cleanup skips the moved-out field; still-available fields are cleaned up normally |
| Moving out the same field twice (`let a = outer.field; let b = outer.field`) | Reject: use after move on the second move |
| Moving out `outer.inner.field` (nested field path) | Valid, tracked the same way as a single-level field |
| Attempting to move out `array[i]` where `i` is a runtime-computed index | Reject: partial-move tracking does not extend to computed indices |
| Attempting to move out a map value by key | Reject: partial-move tracking does not extend to map/collection elements |

## Slice aliasing (§12.5)

| Scenario | Expected result |
| --- | --- |
| Two `mut []T` slices taken from the same `Array<T>`/`[T; N]`, with disjoint (but not compiler-proven-independent) index ranges | Reject: conflict checked at the originating place, not the runtime range |
| A shared `[]T` and an overlapping `mut []T` from the same originating place, held concurrently | Reject, per §11.3 |
| A `mut []T` borrow released (out of scope/last use) before a second `mut []T` borrow of the same originating place begins | Valid: sequential, not concurrent, borrows |
| Attempting a "split at midpoint into two independent mutable halves" pattern with no dedicated builtin | Reject: not expressible in the MVP language |

These cases test ownership/borrow-checker behavior. Array/slice syntax is now
locked in §12.6; see `arrays-slices.md`. Map operations follow §13.3 and
`maps.md`; closure forms and borrowed map-entry APIs remain Q02/Q05.

## Custom destructors and partial moves (§14.3, §31.2)

| Scenario | Expected result |
| --- | --- |
| Move a Move field out of a value defining custom `drop` | Reject; destructor requires a complete receiver |
| Move a nested field or constant-indexed array element through such a value | Reject; inspect every containing type on the projection path |
| Move a field out, promising immediate reinitialization | Reject; intervening failure cannot leave an incomplete destructor receiver |
| Move the entire value defining custom `drop` | Valid; one owner remains responsible for its destructor |
| Move a whole destructor-bearing field out of a destructor-free outer struct | Valid; outer cleanup skips that field; new owner later drops it once |
| Move through that field to extract one of its own Move fields | Reject; the inner custom destructor needs its intact receiver |
| Read a Copy field of a value with custom `drop` | Valid under ordinary borrow rules |
| Borrow or mutably borrow a field of a value with custom `drop` | Valid subject to mutability and exclusivity |
| RHS evaluation of a field replacement panics or propagates `?` | Old field remains initialized; enclosing destructor can run normally |
| Old-field cleanup during replacement panics after invalidating a field required by a containing custom destructor | Process abort; never invoke the enclosing destructor on incomplete storage |
| Successful field replacement in a value defining custom `drop` | Old field cleaned once before replacement is stored; containing value stays usable afterward |

## Recursive borrow provenance (§11.7)

| Scenario | Expected result |
| --- | --- |
| `View{Items: items}` returned from a function receiving `items []int` | Valid; result carries the input backing provenance |
| Same result passed through another function by copying the View | Provenance survives the second return |
| View containing a slice into a function-local owned array is returned | Reject; wrapping the view does not extend the backing lifetime |
| Same view nested in another struct, fixed array, owned array, map value, or closure capture escapes | Reject recursively, once the relevant grammar is implemented |
| Move an owned container containing borrowed views | Moves container ownership; external backing loans remain unchanged |
| Return a view into an `own` parameter's allocation | Reject; the callee-owned allocation does not survive as independent backing |
| Return an owner alongside a view into that owner | Reject; no self-referential return exception |
| Return an externally backed view already stored in an `own` input container | Valid only with preserved, proven external provenance; owning the container does not own the backing |
| Return may select a view backed by either of two inputs | Contract retains both possible origins; caller respects both |
| Recursive functions have unresolved borrowed-result origins | Reject rather than erase provenance or assume ownership |
| Destructor observes a contained view after its ordinary last read | Backing lifetime includes the destructor's use, at scope end, `return`, `?`, replacement, and panic cleanup |
| Value whose destructor reads a view is moved or explicitly dropped | Its borrows end there; the new owner or the drop is the last use |
| Value whose destructor reads a view is declared before the storage it views | Reject; locals drop in reverse declaration order (Q21) |
| Built-in zero shared/mutable slice returned by `?` | Empty result has no backing loan |
| Arbitrary zero-length subslice of local storage escapes | Reject; zero length alone does not erase provenance |

## Mutable descriptor copies and reborrows (§12.3)

| Scenario | Expected result |
| --- | --- |
| Copy a shared slice, then read through both copies while backing stays live | Valid; both retain the same shared backing loan |
| Inside `func edit(s mut []int)`, bind `var next = s` and mutate only through next | Valid exclusive reborrow |
| Access backing storage through s while next has a later use | Reject; source access is suspended during the reborrow |
| Use s after next's last use | Valid; the derived loan has ended |
| Copy a struct containing a mutable slice | Same reborrow restrictions apply recursively; the source's other fields stay usable |
| Obtain mutable access through a shared borrow of such a struct or descriptor | Reject; a shared parameter cannot hold a nested `mut []T` at all (Q20) |
| Structural clone of a shared-borrowed container would duplicate a mutable view | Reject; any clone of a value holding a `mut []T` is rejected (Q20) |
| Replace a struct holding a mutable view | Reborrows through its old views end; the old backing is usable again |
| Reborrow remains live across await | Source remains suspended; backing validity must satisfy §17.6 |
| Two derived mutable views remain independently usable | Reject, including through nested composites |

Map forms follow §13.3 and array/slice forms §12.6; closure forms and borrowed
map-entry APIs remain Q02/Q05. These requirements do not count as executable coverage.
