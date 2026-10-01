# Error-result use and discard conformance cases

Authority: spec §6.6, §7.2, §15.1–15.2, §15.6, and §5.5. Executable checker and
native cases cover construction, contextual `nil`, comparison, explicit discard,
ignored expression results, and named error use across branches, loops, and
reassignment. `?` and async cases remain pending; the tables below are not themselves tests.

Assume `save()` returns `error`, `load()` returns `(Value, error)`, and each
fixture defines all required names.

| Input / scenario | Expected result |
| --- | --- |
| `save()` as an expression statement | Reject silently ignored error result |
| `load()` as an expression statement | Reject ignored error in multiple results |
| `_ = save()`, `let _ = save()`, `var _ = save()` | Allow explicit discard; evaluate once |
| `let value, _ = load()` | Permit explicit error discard; retain ordinary ownership of value |
| `let _, _ = load()` | Explicitly discard both results with ordinary cleanup |
| `let err = save()` with no subsequent use of `err` | Reject never-used error binding |
| `let _err = save()` with no use of `_err` | Reject; underscore prefix is not discard |
| `let value, err = load()` with value used but err never used | Reject never-used error binding |
| `let err = save()` followed by `_ = err` | Explicit discard permitted |
| Error passed to a valid handling function | Counts as a use; enforce ordinary typing/ownership |
| Error returned or propagated under a valid contract | Permitted; preserve required cleanup |
| Ignored error result from an awaited operation | Reject same as synchronous result |
| `_ = await operation()` where operation yields error | Permit explicit discard; preserve suspension and evaluation |
| `task.wait()` yielding an ignored error result | Reject silently ignored retrieved result |
| `_ = task.wait()` yielding one error | Permit explicit discard of retrieved result |
| Discarded Task handle | Detach per §18.6; do not infer implicit wait/error retrieval |

## Representation and construction (§15.1)

| Scenario | Expected result |
| --- | --- |
| `error("not found")` | Valid; constructs a non-nil `error` with that message |
| `var e error = nil` | Valid; zero value |
| `type MyError struct { ... }` used where `error` is expected | Reject: no user-defined error types in the MVP |
| A struct with an `Error() string` method passed where `error` is expected | Reject: no structural interface satisfaction for `error` |
| Type-assert/downcast an `error` value to a concrete type | Reject: no downcasting mechanism exists |

## Comparison (§6.6, §15.1, §41.4)

| Scenario | Expected result |
| --- | --- |
| `error("x") == error("x")` (two separately constructed values) | Equal: content equality on message |
| `error("x") == error("y")` | Not equal |
| `err == nil` where `err` is nil | Equal |
| `err == nil` where `err` is non-nil | Not equal |
| `task == nil` | Allowed (task-nil comparison only) |
| `task1 == task2` (two non-nil `Task<...>` values) | Reject: `Task<...>` supports only nil comparison |
| `ch == nil`, `ch1 == ch2` | Reject: `channel<T>` is not comparable (§19.12) |

## Result-shape and `?` typing (§7.2, §15.2)

| Scenario | Expected result |
| --- | --- |
| `func f() (string, error)` | Valid: error result is last |
| `func f() (error, string)` | Reject: error result is not last |
| `func f() (error, error)` | Reject: more than one error result |
| `let file = open(path)?` inside a function returning `(T, error)` | Valid; propagates on non-nil error, yields `T` otherwise |
| `open(path)?` inside a function with no trailing `error` result | Reject: nowhere to propagate to |
| `return parse(content)?` forwarding the whole result | Valid whole-result forwarding (§7.8) |
| `await fetch()?` inside a function returning `(T, error)` | Valid; awaits then propagates per §7.6 grouping |
| Early return via `?` in a function returning `(Config, error)` | On propagation, `Config` result takes its zero value (§41.4), `error` result carries the propagated error |
| `await task?` where `task` is `Task<T, error>`, inside an async function returning `(U, error)` | Valid; groups as `(await task)?` (§7.6, §18.9) |
| `task.wait()?` where `task` is `Task<T, error>`, inside a synchronous function returning `(U, error)` | Valid; a method call whose last result is `error` |
| `x?` where `x` is not a call, awaited call, or awaited task | Reject: `?` requires one of those forms |
| `x?` where `x`'s last result is not `error`-typed | Reject: not error-propagating |

## Flow-sensitive rules (§15.6)

| Scenario | Expected result |
| --- | --- |
| `var err = attempt1(); err = attempt2(); _ = err` | Reject: `attempt1()`'s error value was never used before being overwritten |
| `var err = attempt1(); if err != nil { return err }; err = attempt2(); return err` | Valid: prior value read and handled on every path before reassignment |
| `var err = attempt1(); if cond { _ = err }` then `err = attempt2()` reached without the discard on some path | Reject: not proven used on every path |
| `let value, err = load(); if err != nil { return zero, err }` with no further use of `err` after the `if` | Valid: the condition `err != nil` is itself a use of `err`'s current value, on both branches |
| `type Result struct { err error }`; construct with a non-nil error field, never read again | Valid: composite storage exits the tracked-binding analysis |
| `var results Array<error>`; push an error, never read it back | Valid: composite storage exits the tracked-binding analysis |

Check diagnostics against resolved types and source spans. Do not infer error
status from a function name, or exempt calls known to return success. Verify
side effects and required cleanup occur once even for explicitly discarded
errors. Wrapping, sentinel error declarations, and structured error payloads
remain open under Q05 and are not covered here.
