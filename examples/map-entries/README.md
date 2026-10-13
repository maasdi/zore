# Borrowed map entries

`Document` owns a byte array. Ordinary subscript lookup cannot copy it, so use
`withEntry` to read it and `withEntryMut` to edit it without removing it.

```sh
zore run examples/map-entries/main.ore
```

Each operation returns presence first, followed by any callback results. A
missing key skips the callback. The document remains owned by the map and closes
exactly once when `main` returns.

Callbacks are synchronous and cannot let borrowed entry views escape. The
mutable form requires a mutable map and initially excludes entries containing
borrowed views. See [the design and contract](../../docs/proposals/borrowed-map-entries.md).
