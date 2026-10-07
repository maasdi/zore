# Mutex conformance cases

Authority: spec §20.2, §10.3, §3.18, §41.4. Cases marked pending have no
executable test yet.

| Scenario | Expected result |
| --- | --- |
| `let counter = mutex(0)` | Type `Mutex<int>`; the value is held by the mutex |
| `counter.withLock(func(value mut int) { value += 1 })` | Valid; the call has no results |
| `let n = counter.withLock(func(value mut int) int { return value })` | Valid; `n` is an `int` |
| A closure whose parameter is not `mut T` | Reject: wrong function type |
| A closure that returns a slice or a function value | Reject: results cannot hold views |
| `mutex([]int)` of a slice, or `Mutex<[]int>`, `Mutex<func()>` | Reject: the value cannot hold views |
| 100 tasks each add 1000 times to one `Mutex<int>` | The total is exactly 100000 |
| Handles passed to tasks with `go` | Copy; every task sees the same value |
| Move value (a struct with `drop`) guarded by a mutex | Dropped once, when the last handle goes |
| Closure that suspends (channel, sleep) while holding the lock | Valid; other tasks wait for the lock without blocking workers |
| Waiters for the lock | Served in order of arrival |
| Calling `withLock` again from inside `f` | Waits forever; reported as a deadlock when nothing else can run |
| Task panics inside `f` | Lock released, mutex poisoned; the panic is raised again at the task's wait |
| `withLock` on a poisoned mutex | Runtime panic naming the poison; `isPoisoned()` is `true` |
| `isPoisoned()` on a healthy mutex | `false` |
| `withLock` on a zero-value mutex | Runtime panic; `isPoisoned()` is `false` |
| `m == m`, `m == nil`, `println(m)`, `clone(m)` | Reject |
| Shadowing `mutex` or `Mutex` | Reject: predeclared names |
| `Mutex` without a type argument | Reject |
