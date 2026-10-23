# Curated native adapters

Status: locked first integration contract for issue #138.

External Zore packages can register one versioned adapter through `[native]` in
`zore.toml`. The compiler owns a small reviewed registry, approved source
fingerprints, private C ABI and build recipe. A registered package supplies its
native C/header, Rust bridge, binding contract, ordinary Zore wrapper and tests.
Its name must match the registry's package name. This is a curated integration
route: adding an adapter requires compiler/runtime review and a new registry
entry. Applications cannot select arbitrary native source paths, linker flags,
shared libraries or build scripts through a manifest. The runtime contains only
generic registration and native resource leak accounting, with no Adler-32
implementation.

The first provider is `adler32-v1`, for a package named `adler32`. It demonstrates
a real statically linked C implementation behind a Rust resource bridge. Adler-32
is a non-cryptographic checksum, suitable for an integration proof, not a security
primitive. No new third-party dependency is needed for this implementation.

## Package contract

```toml
name = "adler32"
[native]
adapter = "adler32-v1"
```

The compiler supplies four private functions in this project's root package:

```ore
func nativeCreate() (int, error)
func nativeWrite(handle int, data []uint8) error
func nativeSum(handle int) (int, error)
func nativeClose(handle int)
```

These declarations are generated from the reviewed registry; ordinary source
functions still require bodies. A library defines its public `Hasher` resource,
constructor, methods and custom `drop` in Zore. Generated names participate in
ordinary duplicate checks and remain private to the package. Only the root
package receives them. Subpackages and consumers use the library's public API.
An application consumes this package with an ordinary local or pinned Git
dependency. No import aliases, source pointers, annotations or unsafe syntax
are introduced.

Wrapper maintainers are responsible for keeping tokens private and pairing
creation with a resource destructor. Registration validates bindings and recipes,
not the resource protocol of arbitrary wrapper code.

The example exposes `New() (Hasher, error)`, `Write(data []uint8) error`,
`Sum() (int, error)` and an owned `Hasher` with a private integer token. Its
custom `drop` makes it Move even though its field is an integer. The zero token
is harmless to close. The native implementation validates every token; it never
interprets a token as a pointer. Tokens increase monotonically and are never
reused. Invalid/stale handles fail operations; close is idempotent.

## Ownership and ABI

Generated LLVM calls the existing private runtime C ABI: Zore int is signed
64-bit, a byte slice is a borrowed pointer plus signed 64-bit length, and result
structs are written through out pointers. Rust owns Zore error-message storage
and returns it through the normal reference-counted string machinery. It never
passes a Rust allocation to C for deallocation.

The reviewed C provider uses explicit uint8_t/uint32_t/uint64_t and int32_t
scalars and opaque void pointers internally. It allocates its state with malloc
and frees it with free. Only the Rust bridge sees these pointers. Input bytes
are borrowed for the call, are never retained, and may contain NUL or non-UTF-8
bytes. A write is limited to 65,536 bytes and fails without changing state when
invalid. Empty input is valid. Allocation failure is translated into a Zore
error; Rust runtime allocation exhaustion retains the runtime's abort behavior.

C returns status codes, never language exceptions. The Rust bridge contains no
intentional panic path: poisoned locks become normal operation errors, while
cleanup recovers the stored state. An unexpected Rust/C bug, abort or process
termination has no cleanup guarantee. Zore's structured panic stays above the
native boundary and invokes the ordinary resource destructor before propagation;
no unwinding crosses C frames.

Each C state is owned by a Rust object behind a per-resource mutex and Arc lease.
The registry mutex is held only for token lookup, insertion or removal. Closing
removes the token; an operation already holding a lease can complete before the
state is freed exactly once. C access is serialized per resource and may move
between OS threads. There are no callbacks into Zore, borrowed native outputs,
or native references escaping into user values.

## Async and cleanup

All first-provider calls are synchronous, bounded CPU operations. They perform
no blocking I/O, wait, suspension or scheduler callback. A call is not interruptible;
ordinary cancellation/deadline checks surrounding it retain their existing
semantics. Async Zore functions can call the adapter, own a Hasher in a task frame,
borrow it under the normal checker, and keep it across awaits. Normal return,
`?`, explicit drop, task-result cleanup and structured panic run its custom drop.
Detached tasks retain their ordinary process-lifetime limits.

The registered SQLite adapter uses helper jobs for its private operations and
poll-lowered library wrappers; its destructors remain compensated synchronous
cleanup. Connection and input loans remain live until helper completion. Ordinary
user plain helpers retain synchronous semantics. The [SQLite guide](../libraries/sqlite.md)
locks its resource, busy-timeout and transaction boundary. A later TLS adapter
must register its wait strategy and retain resource/input leases until native
completion, including after cancellation. Pollable I/O, deadline
propagation, callback/reentrancy policy and backend-specific shutdown remain
adapter design requirements, not capabilities established by this checksum proof.

## Build, distribution and integrity

The adapter's C source, header, Rust bridge and binding contract ship with the
declaring package. For `adler32-v1` these are `native/adler32.c`,
`native/adler32.h`, `native/bridge.rs` and `native/bindings.abi`. The registry
fixes these paths and the approved fingerprint of all four, the symbol mapping
and the versioned build recipe. Sources must stay inside the declaring project
root. Missing, escaping or modified sources fail validation across the whole
graph, including unused and transitive dependencies; there is no bundled
fallback. The binding contract is compiler input, not an ordinary bodyless Zore
source file. The entry lockfile records adapter ID and fingerprint on each
declaring project's record, including transitive/unused projects. Native graphs
require a matching lockfile; `install`/`lock` generate it. Library sources may
be local and mutable, or pinned with the Git cache/commit validation from #137.
The fingerprint prevents silently substituting a different native implementation
under an existing application lock. Regenerating a lock cannot approve arbitrary
native code: a source change also requires a reviewed registry recipe update.

`check` validates metadata and public Zore APIs without invoking a native
compiler, linker or native code. `build`, `run` and `test` compile the selected C
source snapshots with the same ZORE_CC/clang used by the native driver and link
each object statically into the program. No dynamic loading or system-library search is
performed. Multiple packages using one registry adapter share one build selection;
existing graph identity rules reject incompatible projects/versions.

The driver generates module registration for validated package-owned bridges.
It compiles snapshots of the same bytes that passed approval checks, with no
package registration/build script. The runtime library cache includes selected
adapter recipes and generic runtime sources in its key, along with rustc's
host/version identity. C objects are built in a unique temporary build directory
on each build and never reused as unchecked
persistent artifacts. Interrupted builds cannot publish partial native objects
for later reuse. The dependency cache remains immutable and user sources are
never modified. Locks pin source/recipe selection, not the compiler/toolchain or
bit-identical machine code; use fixed compiler, clang and Rust versions for that.

Supported adapter hosts are Linux GNU and macOS on x86_64/aarch64, and Windows
x86_64 MSVC. CI validates the native hosts it runs on; cross-compilation, other
architectures and other ABI environments receive explicit build diagnostics.
The existing ZORE_CC/ZORE_RUSTC overrides and Windows MSVC prerequisites apply.
No additional SDK or external library beyond the normal host toolchain is needed
by the checksum provider.

There is no transitive native library list in package metadata. Future curated
providers must enumerate and pin their full source/library closure in the registry
recipe and the lock fingerprint, and define platform-specific link ordering and
licensing requirements. An unrestricted third-party ABI/plugin SDK, arbitrary
C/Rust package compilation, shared libraries, native version solving, full SQLite
and TLS implementations are outside this first contract.

## Validation

Conformance covers metadata rejection, root-only/private registration, duplicate
names, unchanged rejection of user bodyless declarations, native recipe lock
mismatch, missing/modified/escaping package sources, linker-independent checks,
local and pinned Git consumption, direct and transitive visibility, binary
bytes/empty/oversized input, known checksum vectors, invalid tokens,
normal/error/panic cleanup, resource moves and retention across async waits.
Native leak checking counts live provider allocations through generic runtime
hooks. Linux/macOS/Windows CI builds and exercises the C ABI and statically
linked bridge. The package-owned bridge is linted with generic native
registration enabled.
