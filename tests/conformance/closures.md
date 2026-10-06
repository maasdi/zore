# Closure conformance cases

Authority: spec §16, with §5.7–5.8, §7.3–7.5, §7.7, §11.6, §12, §15.3, §17.2,
and §31. Rows marked "pending" have no executable counterpart yet: they depend
on features that do not exist (tasks, `await`) and do not count as passing
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
| Body consumes outer Move value (`own`, return, `drop`) | Rejected as unsupported (call-once closures open) |
| Outer value moved or dropped while a closure capturing it is live | Rejected |
| Capture of a value already moved | Rejected |
| Same name declared inside the body | Shadows the outer local; nothing captured |
| Two closures that both write the same local, both live | Rejected |
| Two closures that both only read the same local | Accepted |
| Nested closure writes a local two levels out | Every capture in the chain is exclusive; rejected for a `let` binding |
| Captured view whose backing is written while the closure is live | Rejected |
| Body stores a closure or view into a captured local | Rejected as unsupported |
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
| Callee moves or stores its function-typed parameter | Rejected |
| Function-typed parameter declared `mut` or `own` | Rejected |

## Non-escaping rule

| Scenario | Expected result |
| --- | --- |
| Closure bound with `let g = f` | `f` moved; later use of `f` rejected |
| Function or function type with a function-typed result | Rejected |
| Function type or literal with a result holding a view | Rejected as unsupported |
| Function type as a struct field, array, `Array<T>`, slice, or map element | Rejected |
| Closure outlives a captured local's block through an outer `var` | Rejected |
| Closure used in `go`, or live across `await` | Pending: tasks and `await` are not implemented |

## Cleanup

| Scenario | Expected result |
| --- | --- |
| Closure goes out of scope | Nothing dropped; captured locals drop at their own scope exits |
| Captured Move value used through the closure | Dropped once, by its owner |
| Captured `var` replaced through an exclusive capture | Old value dropped at the replacement; new value dropped by the owner |
| `own` parameter of a closure | Dropped when the closure body ends |
| Closure body panics | Runs cleanup for body locals, then unwinds through the caller and its frames |
| Closure body `?`-returns an error | Closure returns the error; body cleanup runs; caller sees the error result |
