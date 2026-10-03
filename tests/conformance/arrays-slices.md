# Arrays, indexing, and slicing conformance cases

Authority: spec §12.6, with §3.7, §5.6, §7.5–7.6, §11.6–11.7, §12.3–12.5,
§17.6, and §31.2. Fixed-array typed literals, indexing/bounds-shape checks
(including static rejection of a provably-out-of-range constant index),
and mutable-place and conservative-aliasing rules for indexed places have
executable parser and type-checking coverage in `tests/parser/parser.rs` and
`tests/typecheck/check.rs`; rejection of moving an element out through an
index is covered in `tests/ownership/ownership.rs`. LLVM codegen for fixed
arrays is implemented and covered by `tests/codegen/native.rs`: array
construction, index read/write, runtime bounds-check panics (including the
unsigned-index wraparound case), and element cleanup through a custom `drop`
method at scope exit and on index replacement.

Borrowed slices are checked but not yet compiled natively. Slice types,
`base[low:high]` with omitted bounds, rejection of a third bound and of slice
literals, contextual shared/exclusive typing, mutable-source requirements,
no implicit array-to-slice conversion, static rejection of provably invalid
bounds, element mutability through shared versus mutable views, and the
region rows (disjoint mutable views, reborrow suspension, last-use release,
views outliving a local or temporary owner, and invalidating a live view's
owner) are covered in `tests/parser/parser.rs`, `tests/typecheck/check.rs`, and
`tests/ownership/ownership.rs`. Runtime slicing and slice bounds panics,
dynamic `Array<T>`, the constant-index move-extraction carve-out of §31.2, and
rows involving `await`, tasks, or channels remain pending.

## Typed literals

| Scenario | Expected result |
| --- | --- |
| `[int; 3]{10, 20, 30}` | Valid fixed array with exactly three int elements |
| `[int; 0]{}` | Valid zero-length fixed array |
| `[int; 3]{10, 20}` or `[int; 2]{10, 20, 30}` | Reject incorrect element count; no zero filling |
| Fixed size is negative, fractional, runtime-only, or cyclic constant | Reject under §5.3 |
| `Array<int>{10, 20, 30}` and `Array<int>{}` | Valid owned arrays with three and zero elements |
| `[uint8; 1]{255}` versus `[uint8; 1]{256}` | Accept representable contextual literal; reject out-of-range literal |
| Element is an already typed incompatible value | Reject; no implicit numeric conversion |
| Trailing comma in a single-line or multiline literal | Allowed |
| Newline after final element without a trailing comma | Reject after semicolon insertion; closing brace does not repair the separator |
| Literal omits its element type or uses repeat/spread syntax | Reject; not part of §12.6 |
| `[]int{1, 2}` | Reject slice literal; obtain views by slicing existing storage |
| One element expression produces multiple results | Reject; no result splicing |
| Literal in a condition whose braces conflict with the body | Require parentheses as for struct construction |
| Chained literal/index expression such as `Array<int>{1, 2}[0]` | Copy read yields 1; temporary owner lives through the access |

## Index types and bounds

| Scenario | Expected result |
| --- | --- |
| First and last valid indices of fixed/dynamic arrays or slices | Access corresponding element |
| Index equal to length, including zero on an empty array | Reject if index/length statically known; otherwise runtime panic |
| Negative signed index | Same bounds failure policy |
| Maximum uint64 index into a short collection | Bounds failure before narrowing; never wraps into a valid index |
| Float, rune, or bool index | Reject; integer indices only |
| Different signed/unsigned integer widths used as valid indices | Accept based on mathematical index value |
| Base and index expressions have observable effects | Evaluate base then index exactly once |
| Index expression propagates `?` | Do not access element; clean up owned temporaries |
| Runtime bounds failure in debug or release | Panic with ordinary cleanup in both modes |

## Places and element ownership

| Scenario | Expected result |
| --- | --- |
| Read Copy element through index | Copy element value |
| Pass Move element to a shared parameter | Borrow element without extracting it |
| Pass Move element to own parameter through dynamic-array/slice index | Reject Move extraction |
| Move from constant-indexed fixed array | Allowed only under §31.2, including custom-destructor restrictions |
| Move from runtime-indexed fixed array | Reject partial-move extraction |
| `var data = Array<int>{1, 2}; data[i] = 3` with valid runtime i | Valid writable place |
| Runtime-index mutation through mut-borrowed fixed/dynamic array | Valid with exclusive access |
| Mutation through let-owned array or shared slice | Reject |
| Mutation through mutable slice element | Valid under §11.6 |
| Two mutable borrows using distinct runtime index expressions | Reject unless non-overlap is proven; textual difference is insufficient |
| Index assignment evaluates base/index, then RHS | Each evaluated once, following §5.6 |
| Replace a Move element | Drop prior initialized value before storing replacement, subject to existing panic rules |

## Ranges and contextual slice mutability

| Scenario | Expected result |
| --- | --- |
| `data[1:3]` for length at least 3 | View contains elements at indices 1 and 2 |
| `data[:2]`, `data[1:]`, `data[:]` | Defaults omitted lower bound to zero and upper bound to length |
| `data[0:0]` or `data[length:length]` | Valid empty view; retains source provenance |
| Negative bound, reversed range, or upper bound beyond length | Static rejection if provable; otherwise runtime panic |
| `data[0:1:2]` or step/stride syntax | Reject; no third bound or stride |
| Base/lower/upper bound expressions have observable effects | Evaluate in that order once each, skipping omitted expressions |
| Upper bound propagates `?` after lower bound succeeds | No slice is produced; prior side effects persist and temporaries are cleaned up |
| `let shared = data[:]` with var-owned data | Produces shared slice; source mutability alone does not request exclusivity |
| `var part mut []int = data[1:]` from mutable data | Produces exclusive view with mutable descriptor binding |
| Function returning `mut []int` returns a slice of an eligible mut parameter | Expected result type requests exclusive view; return provenance checked |
| `edit(data[:])` with edit taking `mut []int` | Valid contextual exclusive view for the call; no named temporary required |
| `inspect(data[:])` with inspect taking `[]int` | Shared view for the call |
| Pass whole array directly where slice expected | Reject; use `data[:]` |
| Mutable context slices a let-owned array | Reject; immutable source cannot grant mutable access |
| Mutable context slices an already shared view | Reject; shared view cannot be upgraded |
| Shared slice binding is later passed as mut | Reject; later use does not change its original type |
| Let-bound mutable descriptor passed onward as a mut argument | Reject under existing caller-place rule; use var for that binding |
| Mutable subslice derived from a mutable view | Exclusive reborrow; overlapping source access suspended until final derived use |
| Two live mutable slices of disjoint ranges of one owner | Reject; whole originating place is the alias-checking unit |
| Slice of existing view forwarded in a composite | Retains original backing provenance |
| View would outlive a temporary or local owned array | Reject, including composite return/capture |
| Backing owner moved, replaced, dropped, or resized while view remains live | Reject invalidating operation |

## Partial construction, cleanup, and suspension

Use instrumented Move resources and explicit synchronization once a runnable
compiler exists; these scenarios do not add a source-level instrumentation API.

| Scenario | Expected result |
| --- | --- |
| Literal evaluates several side-effecting elements | Left-to-right, exactly once; no parallel evaluation |
| Third initializer propagates an error after two Move elements are initialized | Drop those two elements and applicable temporaries exactly once; no later initializer runs |
| Later initializer panics | Unwind initialized elements/temporaries; no drop of uninitialized slots |
| Earlier Move initializer consumed a source before a later failure | Ownership is not restored to source; cleanup belongs to partial construction |
| Initializer awaits before later elements | Preserve earlier owned elements in async state; later evaluation starts after await completes |
| Bound/index awaits while retaining a base borrow | Accept only if borrow remains valid through suspension under §17.6 |
| Returned slice crosses task/channel boundary | Existing independent-provenance restrictions still apply |

Map access is covered separately in `maps.md` (§13.3). String access, iteration,
and remaining collection library operations await their specification decisions.
