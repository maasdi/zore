# Application tests

The calculator implementation lives in `calculator.ore`. Its tests share the
same package and call the private `sum` function. The application's `main`
prints `5`; it is checked but never called by the test runner.

```sh
zore test examples/application-tests
zore test examples/application-tests/calculator_test.ore
```

The first command runs all four tests. The second runs `TestCleanup` and
`TestSum`, while checking the whole package. Each test has a fresh process and
runs with this folder as its working directory.

To demonstrate both failure paths and continued execution:

```sh
ZORE_TEST_DEMO_FAILURES=1 zore test examples/application-tests
```

`TestPanic` reports a panic, and `TestReturnedError` propagates an error through
`?`. Both print their cleanup message. `TestSum` still runs afterward. The
summary is `Tests: 2 passed, 2 failed`, and the runner exits with status 1.

See the [application testing guide](../../docs/testing.md) for the contract and
native toolchain requirements.
