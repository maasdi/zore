# Native-backed Zore libraries

A native-backed library exposes ordinary Zore functions and resource types. An
application consumes it with the same local or pinned Git dependency declaration
as another library. Its reviewed adapter is registered with the compiler through
the library's own manifest:

```toml
name = "adler32"

[native]
adapter = "adler32-v1"
```

The first supported adapter is a small incremental Adler-32 checksum, implemented
in C with a Rust lifetime/error bridge. It is a non-cryptographic checksum.
Its Zore wrapper is a separate project in [the example](../examples/native-library).
You can maintain that project outside the application tree or consume a committed
copy through a Git pin. The library owns its reviewed native implementation in
`native/`; the compiler validates it against the registered recipe.

## Application usage

```ore
import "adler32"
import "std/strings"

func checksum(text string) (int, error) {
    var hasher = adler32.New()?
    let data = strings.Bytes(text)
    hasher.Write(data[:])?
    return hasher.Sum()
}
```

`Hasher` owns its state and cleans it up when its lifetime ends, including on
`?` and structured panic. It is Move, has a private resource token, and uses
ordinary borrowing. Pass ownership into a task with an `own` parameter; the task
can keep it across waits. A native write borrows its bytes for that call only.
Strings use ordinary `strings.Bytes` conversion at the wrapper/application layer;
the C provider accepts lengths and bytes, including NUL and non-UTF-8.

`New()` returns an allocation error when native state cannot be created.
`Write` accepts at most 65,536 bytes per call and leaves state unchanged on invalid
input; split larger inputs into calls. `Sum()` returns a nonnegative checksum in
an int. A zero or closed resource reports an operation error and is safe to drop.
These short CPU calls are synchronous and not interruptible. They do not perform
blocking I/O or make an async function block on an external wait.

```sh
zore install --offline examples/native-library/app
zore check --locked --offline examples/native-library/app/main.ore
zore run --locked --offline examples/native-library/app/main.ore
zore install --offline examples/native-library/adler32
zore test --locked --offline examples/native-library/adler32
```

The sample prints `300286872` for Wikipedia. Check needs no native compiler or
linker. Build/run/test need the [normal native toolchain](platform-support.md)
with clang's C11 headers. Linux GNU and macOS x86_64/aarch64, and Windows x86_64
MSVC, are the supported adapter hosts. No extra native SDK/library is needed.

## Maintainer contract

An adapter registration fixes the expected project name, generated private
binding signatures, ABI, source closure, build recipe and supported hosts.
`adler32-v1` supplies `nativeCreate`, `nativeWrite`, `nativeSum` and `nativeClose`
only inside the library's root package. They are private; downstream code uses
`New`, `Hasher.Write` and `Hasher.Sum`. Duplicate declarations are errors.
Ordinary source functions still require bodies. The wrapper is trusted adapter
glue: maintainers keep tokens private and close them from a custom drop.
Registration validates the binding contract; it does not prove an arbitrary
wrapper's resource protocol. The example keeps that protocol in ordinary Zore
code and private binding tests.

The entry lockfile records the adapter ID and SHA-256 recipe fingerprint, along
with the graph's ordinary manifest/source metadata. Even a local native graph
requires its lock. `zore lock` explicitly regenerates it when reviewed native
recipe changes make an old lock incompatible. Native sources must match the
reviewed registry fingerprint; regenerating a lock cannot approve modified
native code. Missing files and paths escaping the library root fail validation,
including for unused/transitive native packages. A pinned Git package still
obeys Git source/cache validation; libraries' own locks are ignored when
consumed.

C state is allocated/freed by C, Rust owns Zore error allocations, and a validated
registry token connects them. Operations hold a resource lease and serialize
access to the C state. Native unwinding cannot cross the ABI. Zore panic cleanup
runs outside the C call; native bugs, process abort and external termination have
the ordinary cleanup limits. The runtime leak checker includes native states.

Builds compile the selected reviewed C sources into temporary objects and link
statically from validated source snapshots. The driver generates bridge module
registration; the runtime provides generic resource leak accounting. Runtime
cache keys include the adapter recipe, generic runtime sources and toolchain; C
objects are never reused from an unchecked persistent artifact cache. No source
or native object is written into a user's library/dependency checkout. Locks
select source recipes; fixed compiler/toolchain versions are still needed for
reproducible machine code.

This first path supports curated adapters reviewed with the compiler/runtime.
Adding a registry entry is required for a new native backend. The registered
paths are `native/bindings.abi`, `native/adler32.c`, `native/adler32.h` and
`native/bridge.rs` for this provider. Arbitrary native sources, link flags, build
scripts, dynamic loading and a
general third-party ABI SDK are outside this contract. See the
[design record](design/native-adapters.md) for the ABI and extension requirements.

## SQLite and later TLS libraries

The [SQLite package](libraries/sqlite.md) registers SQLite 3.54.1 through
`sqlite-v1`. Its package-owned amalgamation and direct C ABI bridge supply owned
databases, statement connection leases, transient parameter copies, owned binary
rows, and callback-scoped and retryable low-level transactions with rollback on
abandonment. The fixed build flags participate in recipe fingerprints. Its
wrappers use helper waits in async callers and compensated synchronous calls
otherwise. Busy timeout bounds lock retries; query cancellation, interrupts and
hard deadlines are not exposed.

An audited TLS backend would similarly register its complete reviewed source and
link closure, expose owned session/configuration values, and define certificate
validation, socket ownership, secret-buffer handling and handshake/read/write
state machines. Cancellation and deadlines must retain input/resource leases
until native work finishes. These remain adapter-specific design and validation
work; the checksum proof establishes registration, ownership and static linking.

## Cryptographic byte utilities

The maintained external [`crypto` package](crypto.md) uses the curated
`crypto-v1` adapter and the compiler-embedded, pinned mbedTLS 3.6.7 source
closure. It exposes secure random bytes, SHA-256, HMAC-SHA-256 and constant-time
byte equality with ordinary borrowed inputs and owned arrays. The guide records
its bounded scope, offline installation, platform dependencies and provenance.
