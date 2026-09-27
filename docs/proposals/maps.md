# Q02 proposal: map construction, lookup, assignment, and removal

Status: ACCEPTED and incorporated into spec §13.3, with coordinated assignment,
evaluation, partial-move, grammar-status, and zero-value updates. The specification
is authoritative; this document preserves the accepted proposal. Pending cases
are in `tests/conformance/maps.md`. Compiler implementation is not authorized
by acceptance of this proposal.

## Recommended surface

```ore
var scores = map[string]int{"Ada": 10, "Lin": 20}
let found, score = scores["Ada"]
scores["Ada"] = 30
let removed, oldScore = scores.remove("Ada")
let empty = map[string]int{}
```

Both lookup and removal always produce two results, `(bool, V)`: presence first,
value second. A missing key produces false and V's zero value. A present key
produces true even when its stored value equals zero. Explicit `_` discards are
allowed. There is no context-dependent one-result lookup form.

Presence first is deliberate: methods returning an error-typed V still obey
§7.2's trailing-error rule. For `map[string]error`, removal returns `(bool,
error)` and follows ordinary error-use/propagation rules. Subscript lookup is
not a call, so §15.2 does not permit appending `?` to it; bind/use/discard its
error result explicitly. This proposal adds no exception to result forwarding
or error-result ordering.

## Key and value types

MVP keys are bool, integer types (including aliases), rune, and string. Key
matching uses their existing equality semantics; string keys compare content,
not buffer identity. Equal keys must have equal hashes. Hash algorithm, storage
layout, and collision strategy are implementation details.

Reject float keys for the MVP to avoid NaN/equality corner cases. Also reject
error, Task, channel, array, slice, map, struct, and closure keys in this bundle;
being Copy or supporting equality alone does not imply map-key eligibility.
General user-defined hashing/equality remains excluded. Key arguments must match
K under ordinary contextual-literal and exact typed-value compatibility rules.

Values may be Copy or Move, subject to existing recursive provenance and type
validity rules. Maps remain Move; storing borrowed views does not extend their
backing lifetime. No map equality, ordering, or hashing is added.

## Literals and evaluation

`map[K]V{key: value, ...}` explicitly states both types. Empty maps and trailing
commas are supported. Follow existing semicolon insertion: multiline final
entries need a comma before the newline. Parenthesize literals where a condition's
body brace would otherwise be ambiguous, as for struct/array construction.

Evaluate each key, then its value, and complete that entry before the next
entry, left to right. Each expression yields one value of its declared type;
multiple-result splicing, spreads, and type-elided literals are rejected.
Keys are copied; Move values are transferred, with no implicit clone.

Duplicate literal keys are rejected when equal constant keys can be established
statically; otherwise a duplicate encountered at runtime panics. For runtime
entries, evaluate the key and value before testing/inserting the complete entry.
On duplicate failure, destroy the uninserted owned value and previously
constructed entries exactly once. Do not silently overwrite an earlier literal
entry. This differs deliberately from assignment to an existing map.

## Lookup and borrowed access

Evaluate `m[key]` as map expression then key, once each. Lookup shared-borrows
the map, does not remove its entry, and returns `(found, copiedValue)` only when
V can be copied through shared access. If V is Move, reject lookup: no implicit
clone, ownership transfer, or reference-shaped result is invented.

A Copy type containing mutable views is also rejected when copying would grant
an exclusive capability through this shared access (§11.7, §12.3). Copy shared
views retain their external provenance; their backing must remain live after
lookup. Ordinary independent Copy values such as numbers and strings are valid.

Map subscripts are not general addressable element places. Reject borrowing an
entry by passing `m[key]` to a parameter expecting V, field updates such as
`m[key].Field = value`, and chained access treating the result pair as V. Bind
the two results first for Copy values; remove Move values to obtain ownership.

Borrowed in-place access for Move values is a follow-up Q05 API decision,
coordinated with closure/callback rules in Q02. No raw references or source
lifetimes are added here. This is a limitation of this bundle, not a claim that
all map operations needed by the MVP are resolved.

## Assignment and replacement

`m[key] = value` inserts or replaces one entry. It is a special map-assignment
target, not a borrowable pointer to an entry. The map must be a mutable place;
let-owned maps and maps reached through shared borrows cannot be modified.

Follow §5.6: evaluate the map/key target, then RHS, once each, retaining the
key and map identity rather than a pointer that rehashing could invalidate.
Validate ordinary aliasing/exclusivity constraints across these phases. Only
then modify the map. Copy values are copied; Move values transfer to the map.

For replacement, destroy the previous owned value before storing the new one.
Remove the old entry from the map's initialized-entry set before its destruction
begins; if that destruction panics, unwind without dropping it again. Clean up
the new RHS temporary and the remaining map entries under ordinary rules. A
containing custom destructor still sees a valid map whose replaced key is absent,
never uninitialized map storage. The key/value result is not rolled back.

This proposal does not add `m[key] += value`: compound map assignment is rejected
because map lookup is a two-result expression and missing-entry behavior would
need another rule. Multiple assignment follows §5.6's non-overlap requirement;
do not assume two key expressions denote distinct or independently mutable slots
in the same map. Use separate assignments when independence cannot be proven.

## Ownership-transferring removal

The compiler-provided `m.remove(key)` uses a mut receiver, borrows the key for
lookup, and returns `(bool, V)`. It is available for both Copy and Move V.
On success, detach the entry and return its value without destroying that value;
the caller now owns it. On absence, return false and V's harmless zero state.
No source-level generic method declaration syntax is introduced.

```ore
// Resource is a Move type with harmless zero-state cleanup (§41.4).
func take(resources mut map[string]Resource) {
    let found, resource = resources.remove("primary")
    if found {
        use(resource)  // ordinary shared borrow; resource remains locally owned
    }
    // cleanup: acquired resource once, or harmless empty-state cleanup
}
```

The map stays valid and contains no moved-out hole. Removal is not the forbidden
partial move from a computed index (§31.2); it is an explicit builtin operation
that updates the collection's initialization state. A removed value retains any
external borrow provenance. No view into map-owned storage may survive mutation.

## Ownership, errors, async, and cleanup

Missing lookup/removal is represented by false, not a panic or an ordinary
error. Literal duplicate failure panics with ordinary cleanup. No operation
implicitly awaits or spawns work. Explicit await or `?` in key/value expressions
follows left-to-right evaluation and keeps retained values/borrows valid through
suspension and early exit. Future iteration and callback APIs must respect the
same map mutation exclusion.

Destroying a map destroys every remaining initialized entry exactly once;
ordering between distinct entries is unspecified. Resource zeros from a miss
honor §41.4; initialization/destruction is not skipped because presence is false.
Partially constructed literals and failed replacements need per-entry cleanup
state. Completed moves are never undone on error or panic.

## Compiler impact and pending conformance plan

Add map-literal AST entries, type-restricted two-result map lookup, a distinct
map-assignment target, and compiler-provided remove resolution. Preserve spans
for map/key/value expressions. Reject invalid key types and illicit shared
copies, track external view provenance, and lower mutable operations without
retaining raw bucket addresses across arbitrary RHS evaluation. Maintain entry
ownership states for duplicate failure, replacement panic, removal, and drop.

On acceptance add paired cases for:

- Empty/populated literals, trailing commas, wrong key/value types, rejected key
  categories, duplicate constant/runtime keys, and exact evaluation order.
- Hit versus miss, present zero versus absent key, mandatory two-result shape,
  shared Copy lookup, and rejected Move/mutable-view lookup.
- Error-valued maps with trailing error results and ordinary error-use checks.
- Insertion/replacement through mutable maps, rejected immutable mutation,
  compound assignment, address-taking/entry borrowing, and overlapping targets.
- Move removal, missing removal's harmless resource zero, provenance retained on
  removal, and exactly-once destruction across success, `?`, panic, and await.
- Runtime duplicate failure cleans the just-evaluated value; replacement panic
  never redrops its old value or leaves invalid map state.

These are proposed future cases, not executable or passing tests. Iteration,
length/capacity APIs, and borrowed in-place entry access remain Q02/Q05 work.
