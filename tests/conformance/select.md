# `select` conformance cases

Authority: spec §19.14, §3.17, §5.9–5.10, §19.8–19.12. Cases marked pending have
no executable test yet.

| Scenario | Expected result |
| --- | --- |
| `select { case let v, ok = ch.receive() { ... } }` | Waits for a value; `v` and `ok` are bound in the arm only |
| `case ch.receive() { }` and `case let _, _ = ch.receive() { }` | Valid; the results are discarded |
| `case out.send(x) { }` | Valid; `x` is moved in only if the case is chosen |
| `default { }` | Runs at once when no case can proceed; never waits |
| Two or more cases can proceed | One is chosen; repeated runs do not always pick the first |
| Receive from a closed or zero-value channel | Can proceed: `(zero, false)` |
| Send on a closed channel chosen | Panics; the value is dropped first |
| Unchosen send values with a custom `drop` | Dropped when the `select` ends, in reverse source order |
| Chosen receive of a Move value | Dropped when the arm's scope ends |
| Operands evaluated before choosing | Each channel and send value once, in source order |
| Blocked `select` | Other tasks keep running; woken by a send, receive, or close |
| `select` whose cases can never proceed and nothing else running | Deadlock report |
| `break` and `continue` in an arm | Target the enclosing `for`; outside a loop, rejected |
| `return` in an arm, and `select` as the last statement of a function with results | Valid when every arm returns |
| `select {}` | Rejected: at least one case |
| Two `default` arms | Rejected |
| `case 5 { }`, `case ch.close() { }`, `case f() { }` | Rejected: a case is a channel send or receive |
| `case let v = ch.send(x) { }` | Rejected: send has no results |
| `case let a, b, c = ch.receive() { }` | Rejected: receive has two results |
| Same channel in several cases | Valid |
| `case` or `default` as variable names elsewhere | Valid: not keywords |
| `select` as an identifier | Rejected: keyword |
| `async` function using `select` | Valid |
