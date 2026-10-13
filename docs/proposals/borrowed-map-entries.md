# Borrowed map entries

Issue #116 is implemented against locked specification §13.4. The borrowed-entry
part of Q05 is resolved; other API/layout questions remain open.

| Design | Benefit | Cost |
| --- | --- | --- |
| Scoped shared/mutable callback | Uses existing function parameters and errors; keeps an entry borrow inside one operation; returns useful independent values | Callback syntax; borrowing results cannot escape; mutable view-containing entries are initially excluded |
| Direct singleton slice lookup `(bool, []V)` / `(bool, mut []V)` | Reuses an existing view type and supports several operations on a retained borrow | A one-element slice is an awkward entry handle; requires entry-specific lifetime/alias propagation across arbitrary later control flow and mutations |
| Remove, edit, reinsert | Already supported; caller owns the entry | Entry is temporarily absent; callers must handle error/panic cleanup and reinsertion manually |

Choose callbacks for this increment. Keep ordinary lookup `(bool, V)` and
ownership-transferring removal unchanged. No raw references, source lifetimes,
new entry-handle type, or source-level generic method syntax is introduced.

```ore
let found, length = documents.withEntry("report", func(document Document) int {
    return document.data.len()
})
let updated = documents.withEntryMut("report", func(document mut Document) {
    document.data.push(33)
})
```

The callback takes one shared or `mut` parameter, and must be synchronous.
Methods return presence first, then the callback's results. A missing key skips
the callback and returns zeros, including `nil` for an error result. A callback
without results produces only presence. Receiver, key, and callback are evaluated
once, left to right; the receiver's storage is kept borrowed while arguments
are evaluated, preventing invalidation before lookup.

Callbacks may return independent Copy or Move values, including trailing errors
used with `?`. Results cannot contain views or functions. Shared access preserves
the external provenance of ordinary shared views already in entries. Mutable
access initially rejects view-containing entries; proving safe writeback into
such entries is a later extension. Generic callbacks obey the existing generic
constraints and concrete view restrictions. Existing call-once closures can
only be called directly, so cannot be passed here.

The entire map is borrowed for the operation, with no disjointness inferred from
different keys. Captures cannot bypass exclusivity or retain entry views.
Ordinary ownership checking rejects moving a borrowed entry or spawning tasks
that borrow it. Callback parameters cannot store their views into captures.
Mutation of a value through an owned interface uses its ordinary borrowed view.

Normal and error returns keep the entry and completed mutations; no rollback is
promised. Mutable panic cleanup detaches the selected entry and destroys its
initialized parts with the callback's drop flags before caller unwind. This
also handles a destructor panicking during replacement. Shared access leaves
cleanup to the map. Double panic and custom-destructor ancestor abort rules
remain ordinary runtime behavior. The normal hit path needs one existing map
lookup, without removing/reinserting entries or allocating a new entry handle.

The [example](../../examples/map-entries) and
[conformance inventory](../../tests/conformance/map-entries.md) provide executable
usage and paired rejection coverage.
