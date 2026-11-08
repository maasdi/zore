# Application testing

Put tests in files ending in `_test.ore`, with the same package clause as the
implementation. Tests can use private package functions and types. Ordinary
builds and imported dependencies exclude these files.

```ore
package calculator

func TestSum() {
    if sum(2, 3) != 5 { panic("unexpected sum") }
}

func TestFixture() error {
    let value = loadFixture()?
    if value != 5 { return error("unexpected fixture value") }
    return nil
}
```

Test functions start with `Test` followed by an ASCII uppercase letter, take no
parameters or one `t mut testing.T`, and return nothing or `error`. They are
synchronous and have no type parameters. A matching top-level function with
another signature is an error. Methods, `Test`, and names such as `Testhelper`
are ordinary helpers. There is no special assertion syntax: use the checks in
[`std/testing`](#checks-and-the-test-context), returned errors, and panics. A
synchronous test can spawn async work and wait for its results.

```sh
zore test .
zore test path/to/package/implementation.ore
zore test path/to/package/specific_test.ore
zore test -- -package
zore test --filter Pop .
zore test --bench .
zore test --bench --bench-time 200ms --count 3 --filter Push .
```

`--filter` runs only the tests or benchmarks whose name contains the text.

A directory recursively selects packages; an ordinary source file selects all
tests in its package; a test file selects only its own tests. Every selected
package is checked with all its test files, including unselected helper/test
functions. Imported packages exclude their tests. External test packages,
remote dependency solving, and registries are outside this command's scope.
Local dependencies follow [the project dependency rules](local-dependencies.md);
imported library tests are excluded unless that library is selected directly.

The runner skips symlinks, hidden directories, `target`, and `node_modules`, and
rejects a symlink named directly as the target. It checks and builds all selected
packages before running any tests. Paths and then test names determine ordering.
One executable is built per package and reused for a fresh process per test.
The application's `main` is checked if present but is not called; a package
containing only tests can use `package main` without declaring `main`.

Each child inherits environment variables and standard streams, and runs in its
package directory. Output includes `RUN`, `PASS`, and `FAIL` lines followed by
`Tests: N passed, M failed`. A returned error is printed on stdout; panics retain
their source diagnostics on stderr. A failed child does not stop later tests.
Exit status is 0 for all passing tests or no tests, 1 for execution or compilation
failures, and 2 for invalid command arguments. No tests means no native toolchain
is needed, but discovered packages are still checked.

Normal return, error propagation, and panic use ordinary ownership and cleanup.
The generated wrapper also cleans up before reporting a returned error. Explicit
`os.Exit` bypasses cleanup and sets the child status, including 0. Detached tasks
retain their ordinary process-exit behavior; wait for the work your test checks.
External state such as files remains shared across test processes. There is no
implicit timeout; use a caller/CI timeout for potentially hanging suites.
Temporary executables are removed when the runner returns, including after
failures; forced termination may leave temporary artifacts.

Native tests require clang/LLVM 15 or newer and rustc 1.98 or newer targeting the
same host, like `zore build`. Set `ZORE_CC` and `ZORE_RUSTC` to choose compilers;
the ordinary runtime cache is reused. CI verifies Linux, macOS and Windows x64 MSVC; see
[platform support](platform-support.md) for host requirements.

The [multi-file example](../examples/application-tests) shows private helpers,
both signatures, cleanup, and opt-in demonstrations of returned-error and panic
failures followed by a passing test. The [testing-helpers
example](../examples/testing-helpers) shows checks, a test context, a temporary
folder, and a benchmark. The locked contracts are specification §45.1 and §37.22.

## Checks and the test context

`std/testing` checks return an `error`: `nil` when the check holds, otherwise a
message that starts with the file, line and column of the check, then says what
was found and what was wanted: `stack_test.ore:10:5: got 1, want 2`. You never
pass the location; the compiler fills it in.

```ore
package stack

import "std/testing"

func TestPushPop() error {
    var s = New()
    s.Push(1)
    s.Push(2)
    testing.Equal(s.Len(), 2)?
    let top, ok = s.Pop()
    testing.True(ok, "Pop on a full stack should succeed")?
    return testing.Equal(top, 2)
}
```

| Check | Fails with, after the location |
| --- | --- |
| `Equal(got, want)`, `NotEqual(got, avoid)` | `got 4, want 5`, `got 4, want any other value` |
| `EqualFunc(got, want, same)` | `got (1, 0), want (0, 1)` |
| `EqualSlices(got, want)` | `got length 2, want 3`, `at index 1: got 2, want 3` |
| `Near(got, want, tolerance)` | `got 0.3, want 0.5 within 0.001` |
| `True(ok, message)` | your message |
| `NoError(err)` | `unexpected error: disk full` |
| `ErrorIs(err, target)`, `ErrorContains(err, text)` | `got error "x", want "missing"`, `got no error, want one containing "x"` |

Values are written the way `fmt` writes `%v`, so the checks work for numbers,
strings, booleans, runes, and named types, including ones with a
`String() string` method. Compare floats with `Near`, errors with `ErrorIs` or
`ErrorContains`, and structs with `EqualFunc`, giving it a function that says
whether two values are the same; the struct needs a `String() string` method so
the failure can show it.

### Your own helpers

A helper that runs checks for a test can take a `testing.Location` as its last
parameter and pass it on. Callers leave it out, so failures point at the test's
line rather than the helper's:

```ore
func expectLen(s Stack, want int, at testing.Location) error {
    return testing.Equal(s.Len(), want, at)
}

func TestGrows() error {
    var s = New()
    s.Push(1)
    expectLen(s, 1)?
    return nil
}
```

`?` stops the test at the first failed check. To keep going and see every
failure, take a `t mut testing.T` and record checks with `t.Check`:

```ore
func TestPopOrder(t mut testing.T) error {
    var s = New()
    s.Push(1)
    s.Push(2)
    let first, _ = s.Pop()
    let second, _ = s.Pop()
    t.Check(testing.Equal(first, 2))
    t.Check(testing.Equal(second, 1))
    let dir = t.TempDir()?
    t.Check(os.WriteFile(dir + "/data.txt", Array<byte>{1}[:], 0o600))
    t.Cleanup(func() { println("finished") })
    return nil
}
```

A `T` gives you:

- `t.Check(err)` records a failed check and continues; `t.Fail(message)` records
  a message with its location. `t.Failed()` and `t.Failures()` show what was
  recorded.
- `t.Log(message)` prints a line, with its location, at once.
- `t.Run(name, func(c mut testing.T) { ... })` runs one named case with its own
  context. Its failures are reported with the name in front, and its cleanups
  run when the case ends. It returns whether the case passed.
- `t.TempDir()` makes a new, empty folder for this test and removes it, with
  everything inside, when the test ends.
- `t.Cleanup(f)` runs `f` when the test ends. Cleanups run newest first, after a
  normal return, a returned error, or a panic.

When the test ends, the runner prints each recorded failure, then any returned
error, and the test fails if there was either. Like this one, a test can return `error` and use
`?` too, and `testing.NewT("name")` makes a context of your own, which is
how to test a helper that takes one.

### Named cases

`t.Run` keeps table-driven tests readable: each failure says which case it came
from.

```ore
func TestPops(t mut testing.T) {
    for count in Array<int>{0, 1, 3} {
        let pushes = count
        _ = t.Run("after pushes", func(c mut testing.T) {
            var s = New()
            for var i = 0; i < pushes; i += 1 {
                s.Push(i)
            }
            let _, ok = s.Pop()
            c.Check(testing.True(ok == (pushes > 0), "Pop should succeed only on a full stack"))
        })
    }
}
```

A case runs in the same process as its test, so a panic in a case ends the
whole test.

## Benchmarks

A benchmark is named `BenchmarkName` and takes a `b mut testing.B`. The code to
measure goes in a `for b.Loop()` loop; anything before the loop is setup and is
not measured.

```ore
func BenchmarkPushPop(b mut testing.B) {
    var s = New()
    for b.Loop() {
        s.Push(1)
        let _, _ = s.Pop()
    }
}
```

`zore test --bench .` runs the benchmarks, and only the benchmarks, in a fresh
process each. Plain `zore test` never runs them, but both check every
signature. `--bench-time 500ms` (or `2s`) changes how long each one measures,
`--count 3` runs each one three times so you can see how much the numbers move,
and `--filter Push` picks benchmarks by name. The output starts with what the numbers depend on, then shows each
benchmark's iterations, time per iteration, and total time:

```text
Benchmarks: zore 0.1.0-alpha.3, linux x86_64, clang -O2, at least 1s per benchmark in its own process
RUN ./stack::BenchmarkPushPop
601806580 iterations, 2.03 ns/op, 1.223821341s total
PASS ./stack::BenchmarkPushPop
Benchmarks: 1 passed, 0 failed
```

`Loop` runs one iteration, then more and more, until at least the benchmark time
(one second by default) has been measured or a billion iterations have run. The time includes the `Loop` calls
themselves, so a nearly empty loop shows a few nanoseconds. Each number is one
run on one machine: there is no warm-up, statistics, memory counting, or
comparison with earlier runs, and other programs on the machine change the
result. Use `--count` and compare the runs before trusting a difference.

## Git dependency workflows

`cargo test --locked --test git_dependencies` uses temporary local Git repositories
and isolated caches; it requires Git and the ordinary native toolchain for its
build/run/test case. It covers lock regeneration after pin changes, offline validation, lock drift,
cache corruption, concurrent publication, credential redaction, unsupported layouts,
path confinement and shared nominal identity/initialization. The frontend-only
check case explicitly selects a nonexistent native compiler. No public server is
needed. The same suite runs on Linux, macOS and Windows CI.

## Curated native adapters

`cargo test --locked --test native_adapters` validates the C ABI, native recipe
lock drift, package-owned source integrity and confinement, missing-source rejection
after cache population, local and pinned Git diamond consumption, linker-independent checks,
runtime-cache selection and owned async/scoped-result cleanup on normal/error/panic
paths. It requires Git and the native C/Rust toolchain. `cargo test --locked
--test packages native_adapters` covers registration, visibility, duplicate/bad
metadata, root-only private bindings and ordinary move rules without a linker.
CI enables generic `zore_native_adapters` registration with the example
`native/registration.rs` harness when linting the package-owned bridge. Production
builds generate registration from validated package sources.
