# Q02 proposal: arrays, indexing, and slicing

Status: ACCEPTED and incorporated into spec §12.6, with coordinated updates to
§5.3, §7.5–7.6, §11.6–11.7, and §41.1. The specification is authoritative;
this document preserves the accepted proposal, not a competing contract.
Pending conformance cases are in `tests/conformance/arrays-slices.md`.
Acceptance does not authorize compiler implementation.

## Recommended syntax

```ore
var fixed = [int; 3]{10, 20, 30}
var dynamic = Array<int>{10, 20, 30}
let empty = Array<int>{}
let item = dynamic[1]
dynamic[1] = 25

let shared = dynamic[0:2]           // shared []int
var editable mut []int = fixed[:]  // exclusive mutable view
```

Array literals always state their type. Fixed-array literals must supply exactly
N elements; N follows the existing nonnegative constant-size rule. Dynamic-array
literals have as many elements as written, including zero. Reject omitted fixed
array elements, inferred element types, repeat/spread forms, and slice literals
in this bundle. Borrowed slices must view existing storage, except for zero views
already produced by built-in operations under §41.4.

Elements use comma separators, permit a trailing comma, and evaluate left to
right. Multiline literals require a trailing comma before a physical newline
following their final element, consistently with semicolon insertion. Every
element must produce one value compatible with the declared element type;
multiple-result calls cannot be spliced into an initializer. Copy elements are
copied and Move elements transferred under ordinary rules.

As with struct literals, parenthesize a collection literal used directly in an
if/for condition where its braces would otherwise conflict with the body.

## Indexing

`base[index]` is a postfix place expression for fixed arrays, dynamic arrays,
and shared/mutable slices. Evaluate base, then index, once each. Accept integer
indices, including contextually representable untyped integer constants; reject
float, rune, and bool indices. Check the mathematical integer value before any
machine-index narrowing: negative or index >= length is invalid.

Reject invalid bounds when both index and length are statically known;
otherwise panic on an invalid runtime bound, in every build mode. Large unsigned
indices must fail the bounds check rather than wrap into valid indices.

Copy element reads produce copied values. Move elements may be borrowed by an
ordinary parameter without being extracted. Moving out is permitted only for
constant-indexed fixed-array places allowed by §31.2; moving out of a dynamic
array, slice, or runtime-indexed fixed array remains rejected. Extraction APIs
are a separate Q05 decision.

Runtime indexing through mutable arrays and mutable slices produces a writable
place. This explicitly extends §11.6's current constant-index wording for
mutability, without extending partial-move tracking. Runtime-index aliasing
remains conservative: two differently written indices are not proof that two
mutable borrows are disjoint. Shared slices never grant element mutation.

## Slicing

`base[low:high]` denotes a half-open range, excluding high. Permit omitted low
(default zero), omitted high (default current length), and `base[:]`. Bounds
use the same integer rules as indexing. Require 0 <= low <= high <= length;
equal bounds, including length:length, are valid. Reject statically known
invalid ranges; otherwise panic at runtime. Do not add step/stride or a third
capacity bound.

Evaluate base, low if present, then high if present, once each. Slicing borrows
existing backing storage; it neither clones nor transfers the elements.
Owned-array bases must be places whose lifetime covers the resulting borrow;
a view cannot escape a temporary owner. Slicing an existing view preserves its
original provenance. The whole originating place remains the alias-checking
unit (§12.5), even for disjoint runtime ranges or zero-length subslices.

Without a mutable-slice expected type, slicing yields shared `[]T`, even from a
var binding. An explicit `mut []T` binding/result annotation or a parameter of
that type requests an exclusive view. The source must supply mutable access
under §11.6; a let-owned array or shared slice cannot do so. A mutable view's
subslice is an exclusive reborrow when mutable access is requested; overlapping
access through the source remains suspended until that reborrow ends.

```ore
func inspect(items []int) { /* reads only */ }
func edit(items mut []int) { /* may mutate */ }

func demo() {
    var data = Array<int>{1, 2, 3}
    inspect(data[:])               // shared borrow during call
    edit(data[:])                  // contextual exclusive borrow during call
    var part mut []int = data[1:]  // exclusive view, named mutable binding
    edit(part)
    // data can be used again after part's final use
}
```

There is no implicit whole-array-to-slice conversion: use `data[:]`. An already
bound shared slice is not upgraded by a later mutable use. A let-bound mutable
slice descriptor still obeys existing §11.6 caller-place requirements; use var
for a named view that will be passed onward as mut.

This approach avoids a new call-site borrowing marker. Its tradeoff is that
slice mutability can depend on the surrounding type context; explicit binding
annotations make stored mutable views clear. The alternative is new explicit
mutable-slice construction syntax, which would require another grammar choice.

## Ownership, errors, and async

Literal construction retains exactly one owner per Move element. On `?` or
panic during a later initializer, clean up already initialized elements and
owned temporaries exactly once; do not clean up nonexistent elements. No
source value is restored after ownership was already transferred.

Index assignment uses §5.6's target/RHS/store ordering and cleanup rules. Bounds
failures panic rather than return error values. Access through a view retains
its backing loan, including through composites and across suspension; §11.7,
§12.3, §17.6, and task/channel escape restrictions continue to apply. Resizing,
replacing, moving, or dropping a backing owner cannot invalidate a live view.

## Compiler impact and pending conformance plan

Add distinct AST forms for fixed/dynamic array literals, indexing, and slicing;
keep source spans for the base, bounds, and elements. Resolve collection types,
contextual slice mutability, element count/type checks, and place mutability
before ownership checking. Lower bounds checks without narrowing first; retain
partial-initialization state for cleanup. Do not treat runtime-index writable
places as permission for partial moves or proven disjointness.

After acceptance, add pending cases covering:

- Exact fixed-array counts, empty arrays, typed elements, trailing commas, and
  rejection of omitted/repeated/spread elements and multiple-result splicing.
- Valid first/last indices; negative, equal-to-length, huge unsigned, and
  noninteger indices; constant diagnostics versus runtime panic.
- Runtime-index mutation versus rejected runtime-index Move extraction.
- All omitted-bound forms; empty/full slices; reversed and out-of-bounds ranges.
- Shared default, contextual mutable creation, rejected immutable sources,
  unchanged shared bindings, and suspended source access during reborrow.
- Borrowing a Move element without extracting it; no implicit array-to-slice
  conversion; temporary-owner and composite escape rejection.
- Left-to-right evaluation with observable side effects; exact-once cleanup
  after partial construction, `?`, panic, and suspension.

These are a plan for future conformance cases, not executable or passing tests.

## Remaining Q02/Q05 dependencies

This bundle does not define maps, string indexing/slicing/length, iteration,
append/remove/capacity APIs, closures/function types, or complete go grammar.
Map access must separately settle how Move values are borrowed or removed;
string slicing must preserve the locked UTF-8 invariant. Keep those decisions
open rather than inheriting another language's behavior.
