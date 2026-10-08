# Examples

Each directory holds one runnable program, `main.ore`, for a feature the
bootstrap compiler supports today, along with the exact output it prints,
`expected-output.txt`. Run one from the repository root:

```sh
zore run examples/maps/main.ore
```

`cargo test` checks every example with `zore check` and builds and runs each
one natively, comparing its output with `expected-output.txt`. So a directory
here is supported working code, not an aspiration. Features still in
progress have no example until they work.

| Example | Shows | Spec |
| --- | --- | --- |
| [`hello`](hello) | The smallest program and `println` | §3.19, §37.1 |
| [`semantic-target`](semantic-target) | The §42 semantic checkpoint: a struct passed by shared borrow | §42 |
| [`variables-and-constants`](variables-and-constants) | `let`, `var`, `const`, exact untyped constants, numeric types and conversions | §5, §6 |
| [`functions`](functions) | Parameters, multiple results, result forwarding, recursion | §7 |
| [`structs-and-methods`](structs-and-methods) | Struct literals, fields, and methods with shared, `mut`, and `own` receivers | §7.8, §8 |
| [`control-flow`](control-flow) | `if`/`else if`/`else`, counting and condition `for` loops, `break`, `continue` | §5.9–5.10 |
| [`borrowing`](borrowing) | Borrow-by-default parameters and `mut` borrows of mutable places | §11 |
| [`ownership-and-drop`](ownership-and-drop) | Move types with custom `drop`, `own` transfer, explicit `drop`, partial moves, cleanup order | §10, §14, §31.2 |
| [`errors`](errors) | `error` values, `nil`, comparison, and `?` propagation | §15 |
| [`fixed-arrays`](fixed-arrays) | `[T; N]` literals, indexing, element replacement and cleanup | §12.6 |
| [`slices`](slices) | `[]T` and `mut []T` views, slicing, returned views, views in structs | §11.7, §12 |
| [`dynamic-arrays`](dynamic-arrays) | Owned `Array<T>`: literals, indexing, slicing, `mut` passing, element cleanup | §10.5, §12.6 |
| [`packages`](packages) | A project with several packages: folders as packages, `import`, exported names, and cleanup of an imported type | §3.20 |
| [`standard-packages`](standard-packages) | `zore/strings` and `zore/strconv`: case, search, split, join, and number conversion | §37.2 |
| [`strings`](strings) | Byte length, indexing, and slicing, loops over characters, `+`, and `string(rune)` | §6.8 |
| [`collections`](collections) | `len`, `push`, and `pop`, and `for … in` loops over slices, `Array<T>`, and maps | §5.10, §12.7 |
| [`maps`](maps) | `map[K]V` literals, two-result lookup, assignment, ownership-transferring `remove` | §13.3 |
| [`clone`](clone) | `clone` of structs, `Array<T>`, and maps, and a custom `clone` for a resource | §10.7 |
| [`closures`](closures) | Closure literals, function types and parameters, shared and exclusive captures, `?` in a closure, a returned counter, and a call-once closure | §16 |
| [`function-values`](function-values) | Declared functions as values, and `go` on closure literals and function-typed locals that own what they use | §16.2, §18.3, §18.4 |
| [`tasks`](tasks) | `go`, `Task<...>` handles, `.wait()`, `async func` and `await`, owned inputs and results, errors from tasks, and a collection of tasks | §17, §18 |
| [`channels`](channels) | `channel<T>` with and without a buffer, `send`, `receive`, and `close`, a pipeline of tasks, a worker over a buffered channel, a request that carries its reply channel, and a closed channel's zero value | §19 |
| [`select`](select) | `select` over two producers, a `default` arm, a send that finds room or not, and a receive that finds nothing | §19.14 |
| [`io`](io) | `zore/time` and `zore/net`: tasks that sleep, a TCP server that accepts clients, and a reply read back over a loopback connection, with the answers ordered by their delays | §37.3 |
| [`bytes`](bytes) | `Array<byte>` made from a string and turned back with a UTF-8 check, a sum over a byte view, and raw bytes sent over a loopback connection and read back reversed | §37.2, §37.3 |
| [`timeouts`](timeouts) | A `select` that gives up after `time.After`, a worker stopped by a `cancel.WithTimeout` token, and a `Listener` whose `Accept` times out | §37.3, §37.4 |
| [`mutex`](mutex) | `Mutex<T>`: forty tasks updating one shared counter, a guarded struct with a custom `drop`, results from `withLock`, and `isPoisoned` | §20.2 |
