# Async closures and method values

An explicit `async func(...) { ... }` literal can capture locals and suspend
on waiting operations. `counter.Add` captures the receiver in an async method
value; awaiting it holds exclusive access until the method completes.

```sh
zore run examples/async-callables/main.ore
```

The example calls the same method value twice, reads the counter through a
borrowing async closure, and transfers an independent callable into a task.
Use `await` for sequential async calls and `go` for concurrent work. A plain
literal inside an async body keeps its plain behavior.

Captures obey the existing ownership rules. A spawned callable must own values
that can outlive its caller, and cannot mutate a copied capture to change the
caller's value. See [the design](../../docs/proposals/async-callables.md).
