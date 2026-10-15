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
parameters, and return nothing or `error`. They are synchronous and have no type
parameters. A matching top-level function with another signature is an error.
Methods, `Test`, and names such as `Testhelper` are ordinary helpers. There is no
test context or special assertion syntax: use ordinary checks, returned errors,
and panics. A synchronous test can spawn async work and wait for its results.

```sh
zore test .
zore test path/to/package/implementation.ore
zore test path/to/package/specific_test.ore
zore test -- -package
```

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
failures followed by a passing test. The locked contract is specification §45.1.

## Git dependency workflows

`cargo test --locked --test git_dependencies` uses temporary local Git repositories
and isolated caches; it requires Git and the ordinary native toolchain for its
build/run/test case. It covers lock regeneration after pin changes, offline validation, lock drift,
cache corruption, concurrent publication, credential redaction, unsupported layouts,
path confinement and shared nominal identity/initialization. The frontend-only
check case explicitly selects a nonexistent native compiler. No public server is
needed. The same suite runs on Linux, macOS and Windows CI.
