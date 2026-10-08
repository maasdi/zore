# Closure conformance cases

Authority: spec §16, with §5.7–5.8, §7.3–7.5, §7.7, §11.6, §12, §15.3, §17.2,
and §31. Rows marked "pending" have no executable counterpart yet: they depend
on features that do not exist (`await` with closures) and do not count as passing
tests. Every other row has an executable counterpart in
`tests/parser/parser.rs`, `tests/typecheck/check.rs`,
`tests/ownership/ownership.rs`, or `tests/codegen/native.rs`, and
`examples/closures` runs natively.

## Syntax and function types

| Scenario | Expected result |
| --- | --- |
| `let f = func() { println("hi") }` then `f()` | Accepted; prints `hi` |
| `func(a int, b int) int { return a + b }` | Accepted; type `func(int, int) int` |
| Closure with `mut`/`own` parameter modes | Accepted; modes are part of the type |
| Literal called where written, `func(x int) int { return x }(2)` | Accepted |
| Parameter of type `func(int) int` on a declared function | Accepted |
| Same parameter/result types written twice | Identical types |
| Different parameter mode or result list | Different types; passing one for the other is rejected |
| Named literal `func f() {}` in an expression, or named function-type parameters | Rejected |
| Parameter duplicates an outer-body declaration | Rejected, as for functions |
| `return` inside a closure | Exits the closure only |
| Result-returning literal that can fall through | Rejected |
| `break`/`continue` aimed at a loop outside the closure | Rejected |
| `?` in a closure with no trailing `error` result | Rejected |
| `?` in a closure with a trailing `error` result | Propagates out of the closure only |
| `await` in a closure body | Rejected |
| `f == g`, `println(f)`, or a function-type map key | Rejected |
| Wrong argument count or type in a call through a value | Rejected |
| Error result of a call through a value ignored | Rejected, as for any call |
| A declared function's name used as a value | Rejected as unsupported |
| A literal in a constant initializer | Rejected |

## Captures

| Scenario | Expected result |
| --- | --- |
| Body only reads outer `name` | Shared borrow; outer `name` is not writable while the closure is live |
| Outer local written after the last use of the closure | Accepted; loan has ended |
| Outer local written between creation and a later call | Rejected, naming the capture and the closure |
| Body assigns outer `var count` | Exclusive borrow; any other use of `count` while the closure is live is rejected |
| Body assigns an outer `let` binding or a shared parameter | Rejected (not a mutable place) |
| Body assigns an outer `mut` parameter | Accepted; the caller's place changes |
| Body passes outer Move value to a borrowed parameter | Shared borrow; outer value stays usable after the last closure use |
| Body passes outer value to a `mut` parameter | Exclusive borrow; rejected for a `let` binding |
| Body consumes outer Move value (`own`, return, `drop`) | Accepted; the closure is call-once (§16.6) |
| Outer value moved or dropped while a closure capturing it is live | Rejected |
| Capture of a value already moved | Rejected |
| Same name declared inside the body | Shadows the outer local; nothing captured |
| Two closures that both write the same local, both live | Rejected |
| Two closures that both only read the same local | Accepted |
| Nested closure writes a local two levels out | Every capture in the chain is exclusive; rejected for a `let` binding |
| Captured view whose backing is written while the closure is live | Rejected |
| Body stores a view of another captured local into a captured local | Accepted; the outer local borrows it from the closure's creation (Q22) |
| Body stores a view of its own parameter or local into a captured local | Rejected (the first as unsupported) |
| Body stores a plain value into a captured `var` | Accepted |

## Calls and function-typed parameters

| Scenario | Expected result |
| --- | --- |
| Closure passed to a function-typed parameter, callee calls it repeatedly | Accepted |
| Same closure passed to two parameters of one call | Rejected |
| Closure argument after an earlier argument it writes | Rejected |
| Closure called while another live closure captures it | Rejected |
| Closure called after the capturing closure's last use | Accepted |
| Closure passed while a later argument needs its exclusive capture | Rejected |
| Callee moves or stores its shared function-typed parameter | Rejected |
| Function-typed parameter declared `mut` | Rejected |
| Function-typed parameter declared `own`, then stored or returned | Accepted |

## Borrowing and owning closures

| Scenario | Expected result |
| --- | --- |
| Closure bound with `let g = f` | `f` moved; later use of `f` rejected |
| Literal returned, or bound to a local that is returned (directly or through `let g = f`) | Accepted; owning |
| Literal stored in a struct field, fixed array, `Array<T>`, or map value, or passed to `own` | Accepted; owning |
| Owning closure assigns a captured `var` | Changes its own copy; the outer local is treated as moved |
| Owning closure only reads a captured Copy local | Outer local stays usable and independent |
| Owning closure captures a Move value | The value moves into the closure |
| Owning closure captures a view of a local and is returned | Rejected; the view's backing does not outlive the call |
| Owning closure stores a view of one capture's storage into another | Rejected |
| Function type as a result, struct field, fixed-array, `Array<T>`, or map value type | Accepted |
| Function type as a slice element, or slicing storage that holds function values | Rejected |
| Shared parameter whose struct or collection type holds a function value | Rejected; declare it `mut` or `own` |
| Collection loop over function values | Rejected |
| Borrowing closure assigned to an outer `var` that outlives a captured local | Rejected |
| Closure used in `go` | See "Spawned closures" below |
| Closure live across `await` | Pending: `await` with closures is not implemented |

## Call-once closures

| Scenario | Expected result |
| --- | --- |
| `let f = func() { consume(job) }` then `f()` | Accepted; `job` moves into `f`, and the call consumes `f` |
| Call-once closure called twice, or used after its call | Rejected as a use of a moved value |
| Call-once closure passed, rebound, returned, stored, or captured | Rejected |
| Call-once literal bound with `var`, or not bound at all | Rejected |
| Body consumes the same capture twice | Rejected as a use of a moved value |
| Call-once closure created and called inside a loop over an outer value | Rejected on the second iteration's move |
| Call-once closure never called | Its captured values are destroyed at its scope end |
| Owning closure destroyed or replaced | Its captured Move values are destroyed in reverse capture order |

## Cleanup

| Scenario | Expected result |
| --- | --- |
| Borrowing closure goes out of scope | Nothing dropped; captured locals drop at their own scope exits |
| Captured Move value used through the closure | Dropped once, by its owner |
| Captured `var` replaced through an exclusive capture | Old value dropped at the replacement; new value dropped by the owner |
| `own` parameter of a closure | Dropped when the closure body ends |
| Closure body panics | Runs cleanup for body locals, then unwinds through the caller and its frames |
| Closure body `?`-returns an error | Closure returns the error; body cleanup runs; caller sees the error result |

## Declared functions as values (§16.2)

| Scenario | Expected result |
| --- | --- |
| `let f = add` then `f(1, 2)` | Valid; type `func(int, int) int` |
| Function value passed to a `func(...)` parameter | Valid; identity is by signature |
| Package-qualified function, written in Zore or native, converted then called | Valid |
| Function value stored in a struct field, `Array<T>`, or map value | Valid |
| Converted function value is Move | Binding it to another name moves it |
| Function value compared, printed, or used as a map key | Rejected |
| Name of an `async func` as a value | Rejected |
| A method as a value | Rejected |
| `println`, `len`, `push` as values | Rejected |
| Parameter type or mode mismatch on assignment | Rejected |
| Callee and arguments of a call through the value | Evaluated once, callee first |

## Spawned closures (§16.4, §16.6, §18.3, §18.4)

Executable counterparts are in `tests/typecheck/check.rs`
(`go_takes_closures_and_function_values`) and `tests/codegen/native.rs`
(`spawned_closures_own_their_captures_and_destroy_them_once`,
`a_panic_in_a_spawned_closure_destroys_its_environment_once`,
`a_panic_in_an_argument_destroys_the_evaluated_callable_and_spawns_nothing`,
`go_runs_function_values_call_once_closures_and_owning_arguments`,
`spawned_closures_run_as_plain_tasks_even_inside_async_functions`, and
`a_detached_spawned_closure_runs_without_a_handle`).

| Scenario | Expected result |
| --- | --- |
| `go func() { ... }()` capturing Copy values | Valid; values copied |
| Captured Move value | Valid; unusable in the spawner afterward |
| Captured borrowed parameter that is a Move value | Rejected |
| Captured view, `mut` parameter, or exclusive capture | Rejected |
| Spawned closure capturing a borrowing closure | Rejected |
| Closure assigns, compound-updates, or passes a captured Copy local to `mut` | Rejected, at the change |
| Closure copies a captured Copy local into its own `var` and changes that | Valid |
| `go job()` then `job()` or a second `go job()` | Rejected: use of a moved value |
| `go finish()` of a call-once closure | Valid; consumes it |
| `go runWorker(handler)` with an owning function-typed `own` argument | Valid; the closure moves into the task |
| Function-typed argument for a shared parameter, or a closure that holds a view | Rejected |
| Callee or argument expression | Evaluated exactly once, callee first |
| Environment destroyed on normal return, on an `error` result, and on panic | Exactly once each |
| Closure never spawned, then dropped or out of scope | Environment destroyed once |
| Argument expression panics after the callee was evaluated | Callable destroyed once; no task created |
| Detached task with an owning closure | Environment and results destroyed when it finishes |
| Result that is a slice, a view, or a function value | Rejected |
| Callee that is a field, element, map value, or a call result | Rejected |
| Closure inside an `async func` spawned with `go` | Valid; runs as a plain task, body not async |
| `await` inside a spawned closure | Rejected |
| Existing `go declaredFunc(args)` and closure calls | Unchanged |
