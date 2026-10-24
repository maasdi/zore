# SQLite persistence

`sqlite/` is a separate native-backed Zore package. `app/` consumes it through an
ordinary dependency, binds an insert parameter, commits a transaction, and reads
back the stored text. The app creates or updates `notes.db` in its working directory.

```sh
zore install --offline examples/sqlite-library/app
zore check --locked --offline examples/sqlite-library/app/main.ore
zore run --locked --offline examples/sqlite-library/app/main.ore
zore install --offline examples/sqlite-library/sqlite
zore test --locked --offline examples/sqlite-library/sqlite
```

See the [library guide](../../docs/libraries/sqlite.md) for the API, ownership,
blocking behavior, pinned upstream source hashes and supported host toolchains.
The package tests remove their generated database files after successful tests.
Driver conformance additionally consumes a tagged Git release from an offline cache.
