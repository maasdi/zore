# Closure conformance cases

Authority: spec §16, with §5.7–5.8, §7.3–7.4, §7.7, §11.6, §12, §15.3, §17.2,
and §31. These rows are pending: no executable counterpart exists yet, and
`zore check` still reports closure syntax as unsupported. They do not count as
passing tests.

## Syntax and function types

| Scenario | Expected result |
| --- | --- |
| `let f = func() { println("hi") }` then `f()` | Accepted; prints `hi` |
| `func(a int, b int) int { return a + b }` | Accepted; type `func(int, int) int` |
| Closure with `mut`/`own` parameter modes | Accepted; modes are part of the type |
| Parameter of type `func(int) int` on a declared function | Accepted |
| Same parameter/result types written twice | Identical types |
| Different parameter mode or result list | Different types; passing one for the other is rejected |
| Parameter duplicates an outer-body declaration | Rejected, as for functions |
| `return` inside a closure | Exits the closure only |
| `?` in a closure with no trailing `error` result | Rejected |
| `await` in a closure body | Rejected |
| `f == g`, a zero-initialized `var f func()`, or a function-type map key | Rejected |
| Call-site `mut`/`own` markers | Rejected, as for all calls |

## Captures

| Scenario | Expected result |
| --- | --- |
| Body only reads outer `name` | Shared borrow; outer `name` is not writable while the closure is live |
| Outer local written after the last use of the closure | Accepted; loan has ended |
| Outer local written between creation and a later call | Rejected, naming the capture and the write |
| Body assigns outer `var count` | Exclusive borrow; any other use of `count` while the closure is live is rejected |
| Body assigns an outer `let` binding | Rejected (not a mutable place) |
| Body passes outer Move value to a borrowed parameter | Shared borrow; outer value stays usable after the last closure use |
| Body passes outer Move value to a `mut` parameter | Exclusive borrow |
| Body consumes outer Move value (`own`, return, `drop`) | Rejected as unsupported (call-once closures open) |
| Same name declared inside the body | Shadows the outer local; nothing captured |
| Two closures that both write the same local, both live | Rejected |
| Two closures that both only read the same local | Accepted |

## Non-escaping rule

| Scenario | Expected result |
| --- | --- |
| Closure passed to a function-typed parameter, callee calls it twice | Accepted |
| Closure bound with `let g = f` | `f` moved; later use of `f` rejected |
| Callee returns or stores its function-typed parameter | Rejected |
| Closure returned from the function that made it | Rejected |
| Closure stored in a struct field, array, `Array<T>`, map, or channel | Rejected |
| Closure used in `go`, or live across `await` | Rejected as unsupported |
| Closure outlives a captured local's block | Rejected |

## Cleanup

| Scenario | Expected result |
| --- | --- |
| Closure goes out of scope | Nothing dropped; captured locals drop at their own scope exits |
| Closure body panics | Runs normal cleanup for body locals, then unwinds through the caller |
| Closure body `?`-returns an error | Closure returns the error; body cleanup runs; caller sees the error result |
