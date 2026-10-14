# Borrowed map entries

Issue #116 uses the conditional entry bindings locked in specification §13.4.
The borrowed-entry part of Q05 is resolved; other API/layout questions remain open.

| Design | Benefit | Cost |
| --- | --- | --- |
| Conditional shared/mutable binding | Ordinary statements, explicit missing branch, no callback or presence/result plumbing | Compiler must track scoped aliases and cleanup through ordinary control flow |
| Scoped callback | Reuses function parameter borrowing | Callback ceremony and a separate return/error boundary |
| Direct retained entry handle or singleton slice | Familiar lookup followed by later use | New handle representation or awkward singleton slice, with longer-lived alias tracking |
| Remove, edit, reinsert | Already supported; caller owns the entry | Entry temporarily absent and manual reinsertion/error cleanup |

Choose conditional binding. Borrow by default and spell exclusive access with
`mut`, matching parameter contracts. `let`/`var` continue to declare ordinary
locals; `if name = ...` declares a conditional alias. `.entry(key)` makes its
provenance visible and keeps copied `map[key]` lookup unchanged.

```ore
if document = documents.entry("report") {
    println(document.data.len())
}
if mut document = documents.entry("report") {
    document.data.push(33)
} else {
    return error("document not found")
}
```

The binding exists only in the successful block. Missing keys skip that block
and optionally run `else`; lookup has no synthetic results or errors. Receiver
and key are evaluated once, left to right, with storage borrowed before key
evaluation. The missing branch runs after releasing that loan.

The entire map stays borrowed through successful-block cleanup; unequal keys do
not establish disjointness. Copy fields may be read normally; Move entries and
owned fields cannot be moved out. Entry-derived views cannot escape their scope.
Shared views already stored in entries retain their external backing provenance.
Mutable entries containing borrowed views remain excluded pending writeback work.

`return`, `?`, `break`, and `continue` retain their ordinary meaning and release
the scoped borrow after local cleanup. Changes remain committed on normal/error
exits. Mutable panic cleanup detaches the selected entry and destroys initialized
parts using its drop flags, including when replacement destruction panics.
Shared access leaves destruction to the map. Await uses ordinary borrowing and
stable storage; spawned borrowed entries remain rejected. No source references,
lifetimes, public entry handles or per-access heap allocation are introduced.

The [example](../../examples/map-entries) and
[conformance inventory](../../tests/conformance/map-entries.md) provide usage
and paired acceptance/rejection coverage.
