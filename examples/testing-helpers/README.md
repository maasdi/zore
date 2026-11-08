# Testing helpers

A small stack with tests that use `std/testing`:

- `TestPushPop` stops at the first failed check with `?`.
- `TestPopOrder` records checks with a `t mut testing.T`, uses a temporary
  folder, and registers a cleanup.
- `TestGrows` uses a helper that passes its `testing.Location` on, so a failure
  names the test's line.
- `TestPops` runs named cases with `t.Run`.
- `BenchmarkPushPop` measures a `Push` and a `Pop` in a `for b.Loop()` loop,
  so the stack stays small however many iterations run.

```sh
zore test examples/testing-helpers
zore test --bench examples/testing-helpers
```

See the [testing guide](../../docs/testing.md).
