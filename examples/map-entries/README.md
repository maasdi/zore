# Borrowed map entries

`Document` owns a byte array. Use `if document = documents.entry(key)` to read
an entry and `if mut document = documents.entry(key)` to edit it in place.
The name borrows the entry only inside the successful block. Missing keys skip
that block and optionally run `else`, without inserting or producing an error.

```sh
zore run examples/map-entries/main.ore
```

Ordinary `return`, `?`, and loop exits work inside the block. The document remains
owned by the map and closes exactly once when `main` returns. Mutable access
requires a mutable map and initially excludes entries containing borrowed views.
See [the design and contract](../../docs/proposals/borrowed-map-entries.md).
