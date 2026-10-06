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
progress (packages and imports, tasks and async, channels) have no
example until they work.

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
| [`maps`](maps) | `map[K]V` literals, two-result lookup, assignment, ownership-transferring `remove` | §13.3 |
| [`clone`](clone) | `clone` of structs, `Array<T>`, and maps, and a custom `clone` for a resource | §10.7 |
| [`closures`](closures) | Closure literals, function types and parameters, shared and exclusive captures, `?` in a closure | §16 |
