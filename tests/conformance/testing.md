# `std/testing`, context tests, and benchmarks

These cases cover spec §37.22, the `zore test` rules for test contexts and
benchmarks in §45.1, and the type-parameter formatting rule of §37.2. Native
cases are in `tests/codegen/testing.rs`; runner cases are in
`tests/driver/testing.rs`; checking cases are in `tests/packages/packages.rs`;
`examples/testing-helpers` is a stack with tests and a benchmark.

## Formatting a type parameter's value

| Scenario | Expected result |
| --- | --- |
| `%v` and `%4v` of `int`, `int8`, the largest `uint64`, strings, `bool`, `rune` and a named integer in a `comparable` generic function | The same text as for the concrete type, padded to the width |
| `formattable`, `comparable` and `ordered` type parameters, including one passed on to a `formattable` function | Accepted |
| A type parameter constrained by `any` or `copyable` given to `fmt` | Rejected in the generic function, naming `formattable` |
| A struct without `String()` given to a `formattable` parameter, or an `any` parameter passed on to one | Rejected at the call as not satisfying `formattable` |
| `Sprint`, `Sprintln`, and `Errorf` of floats, errors, and a `String()` type through a generic function | The concrete type's text; `Errorf` builds the expected error |
| A generic function calling another with its type parameter | Formatted in the inner copy |
| `%d`, `%s`, or a precision on a type parameter's value | Rejected in the generic body: only `%v` and a width apply |

## Locations

| Scenario | Expected result |
| --- | --- |
| A check called directly, and a method call such as `t.Fail` | The message starts with `file:line:column` of the call, the column at the start of the call or its receiver |
| A helper taking `at testing.Location` and passing it to a check | The location of the helper's caller |
| `File`, `Line`, `Column`, `String` of a received location | The path, numbers, and `file:line:column` |
| A `go` call of a function taking a location | The location of the called function's name |
| A call through a function value | The location is passed explicitly |

## Checks

| Scenario | Expected result |
| --- | --- |
| `Equal` of equal and unequal ints, strings, and a named type | `nil`, or `got 4, want 5` |
| `NotEqual` of different and equal values | `nil`, or `got same, want any other value` |
| `EqualSlices` of equal, different-length, differing, and empty slices | `nil`, `got length 2, want 3`, or `at index 1: got b, want x` |
| `Near` inside, outside, and at the tolerance, with negative numbers | `nil`, or `got 1.0, want 1.5 within 0.25` |
| `True` true and false | `nil`, or the message |
| `EqualFunc` of two structs with a `String()` method | `nil` when the function says they are the same, otherwise both printed |
| `NoError` of `nil` and an error | `nil`, or `unexpected error: disk full` |
| `ErrorIs` of a wrapped target, `nil`, and another error | `nil`, `got no error, want "missing"`, or `got error "other", want "missing"` |
| `ErrorContains` of a matching error, `nil`, and a non-matching error | `nil`, or a message naming both texts |
| `Equal` of floats, mixed types, or an unread result | Rejected by the checker |

## The test context

| Scenario | Expected result |
| --- | --- |
| `Check(nil)`, `Check(err)`, and `Fail` | Only failures are recorded, in order; `Failed` and `Failures` report them |
| `Log` | Written at once, before the failures `Finish` writes |
| `Finish` with failures and a returned error | Cleanups first, newest first; then each failure and the error; returns 1 |
| `Finish(nil)` with no failures | Cleanups run; returns 0 |
| A `T` dropped without `Finish` | Its cleanups run, newest first |
| A panic while a `T` is alive | The panic is reported and the cleanups still run, newest first, with no leak |
| `TempDir` | A new folder named after the test exists, and is removed with its files after the cleanups that use it |
| Using a `T` declared with `let`, using it after `Finish`, or reading its fields | Rejected by the checker |

## Named cases

| Scenario | Expected result |
| --- | --- |
| Three cases, one failing | Each case's context is named `<test>/<case>`; its cleanup runs when the case ends; `Run` returns whether it passed; the failure is added with `<case>: ` before it |

## Benchmarks

| Scenario | Expected result |
| --- | --- |
| A 20 ms goal | More than one iteration; `Iterations` matches the loop count; `Elapsed` is at least the goal; `Loop` stays `false` afterwards |
| An iteration slower than the goal | Exactly one iteration |
| `Finish` before `Loop` ended, and with an error | `benchmark <name> did not run its Loop to the end`, or the error; returns 1 |
| `NewB` with a zero or negative goal | Panics with `testing.NewB: goal must be positive` |

## The runner

| Scenario | Expected result |
| --- | --- |
| `TestName(t mut testing.T)` that records two failures and logs | Both failures listed after the log; the test fails; later tests run |
| `TestName(t mut testing.T) error` stopping at `?` | Its cleanup runs; the error is listed; the test fails |
| A context test that panics | Status 2 and the panic message; both cleanups run, newest first |
| A context test with `TempDir` | The folder exists during the test and is gone after the run |
| A plain `TestName()` next to context tests | Runs as before |
| `zore test` with benchmarks present | Benchmarks are not run |
| `zore test --bench` | A header naming the compiler version, host, `clang -O2` and the one-second goal; benchmarks only, in name order; each measured line, or its failure; `Benchmarks: N passed, M failed` |
| A shared or second `testing.T` parameter, a `testing.B` on a test, a parameterless benchmark, a benchmark returning `int`, or a local type named `T` | A signature error before building, with and without `--bench` |
| `--filter Add` | Only tests whose names contain `Add` run, and only such benchmarks with `--bench` |
| `--bench --bench-time 50ms --count 2` | The header names `50ms` and `2 runs each`; each benchmark runs twice and each run is counted |
| `--count` or `--bench-time` without `--bench`, a duration without a unit or of zero, a count of 0 or 101, an empty filter | Invalid argument, status 2 |
| `examples/testing-helpers` | 5 tests pass, including a helper that passes its location on and named cases; its benchmark passes with `--bench` |
