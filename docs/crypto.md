# Cryptographic byte utilities

The maintained `crypto` package provides secure random bytes, SHA-256,
HMAC-SHA-256, HMAC tag verification and constant-time byte comparison through the curated `crypto-v1`
adapter. It is an external library, distributed initially at
[`examples/crypto/crypto`](../examples/crypto/crypto), rather than an implicit
standard package. It requires the compiler version that registers this adapter.

To use the checked-in example offline:

```sh
zore install --offline examples/crypto/app
zore check --locked --offline examples/crypto/app/main.ore
zore run --locked --offline examples/crypto/app/main.ore
zore install --offline examples/crypto/crypto
zore test --locked --offline examples/crypto/crypto
```

An application adds its own dependency, then explicitly installs its lock:

```toml
[dependencies]
crypto = { path = "../libraries/crypto" }
```

Copy the complete `examples/crypto/crypto` project, including `native/`, to that
path. Alternatively use a full reviewed commit in a Git dependency with
`subdir = "examples/crypto/crypto"` when consuming this repository, or publish
the complete project at a separate repository root. Do not use a floating branch.
A library's own lock is ignored when consumed; the application's lock records
its dependency graph and the approved native recipe. See
[native libraries](native-libraries.md) for source validation and recipe updates.

```ore
import "crypto"
import "std/strings"

func verify(key []byte, message string, tag []byte) bool {
    let bytes = strings.Bytes(message)
    return crypto.VerifyHMACSHA256(key, bytes[:], tag)
}
```

`RandomBytes(size)` accepts 0 through 1,048,576 inclusive; zero does not consult
the entropy provider. Invalid sizes or provider failures return an error and an
empty owned array. Every nonempty request creates a private CTR-DRBG, seeds it
from the operating system through mbedTLS entropy, generates bounded chunks and
frees the state. An OS entropy failure has no insecure fallback.

`SHA256(data)` and `HMACSHA256(key, data)` return a fresh owned 32-byte array.
They accept empty and arbitrary binary input; HMAC accepts an empty key. Inputs
are borrowed for the call and never retained or changed. A digest or MAC
provider failure, which only a broken build or an allocation failure inside the
provider can produce, panics with a message naming the function. Allocation
exhaustion follows the runtime's ordinary fatal allocation policy.
`Equal(left, right)` returns false immediately for different lengths, true for
two empty inputs and otherwise delegates to mbedTLS's constant-time byte
comparison. Lengths are public; this does not make surrounding source code
constant time. Verify MACs with `VerifyHMACSHA256(key, data, tag)`, which
computes the expected tag and compares it through `Equal`; it returns false for
a tag of the wrong length. Never compare MACs with an early-exit comparison such
as `bytes.Equal`.

Calls are thread-safe, synchronous and non-cancellable. Random generation uses
a runtime helper so entropy waits receive the runtime's ordinary blocking
compensation; it is not a suspending async API. Hashing, MAC and comparison do
CPU work proportional to the input size without yielding. Bound untrusted input
and use task/application work limits for large buffers. There are no resource
handles to close. Ordinary array cleanup applies across return, `?`, panic and
async frame destruction. There is no locked memory, guaranteed secret erasure,
secret type or FIPS validation. Avoid logging secrets and do not assume freed
arrays or temporary copies are erased.

## Provider and build provenance

The provider is mbedTLS 3.6.7 LTS, distributed here under Apache-2.0; upstream
also offers GPL-2.0-or-later. The compiler embeds the reviewed source closure,
fixed configuration, license and source manifest from
[`compiler/native/mbedtls`](../compiler/native/mbedtls). Its
[provenance record](../compiler/native/mbedtls/README.zore.md) gives the upstream
release URL and archive SHA-256; `SHA256SUMS` records every source/configuration
file. No system OpenSSL, external crypto installation, provider download or
runtime dynamic loading is used. The compiler embeds the source bytes so a
copied compiler executable can build without its original checkout. Selected
adapters share one statically linked closure. Windows links the OS bcrypt
entropy API; Linux/macOS use the upstream platform entropy implementation.
AES uses immutable precomputed tables to avoid first-use initialization races.
Hardware AES feature detection and acceleration are disabled in this recipe.
Software AES still uses secret-dependent table lookups: this profile does not
claim cache-timing protection against hostile local co-tenants or a whole-provider
constant-time guarantee.

Supported native hosts are Linux GNU and macOS x86_64/aarch64 and Windows x64
MSVC, with the normal clang/rustc native toolchain. `check` needs neither a C
compiler nor linker. Source changes cannot be approved by lock regeneration;
a provider update requires compiler review, updated recipe fingerprints,
known-answer/failure/host tests and explicit application lock regeneration.
Package maintainers track upstream security advisories and issue reviewed
compiler/package updates together; a pin is not an automatic security update.

The regression suite exercises NIST SHA-256 and RFC 4231 HMAC vectors, empty,
binary and long-key inputs, size boundaries, comparison mismatch positions,
fresh-process concurrent first random use, concurrency and cleanup. Test-only wrappers inject failed entropy, a failed
second random chunk and failed digest/MAC calls. Those wrappers are absent from
production and use no environment-controlled insecure modes. Comparison tests
check delegation and results; timing measurements do not prove constant time.
The injected digest and MAC failure tests run at the C and bridge level; the
Zore panic path for a provider failure cannot be triggered from Zore code and is
not exercised.

This package supports tokens, content digests and MAC verification. Password
KDFs, AEAD, signatures, key encodings, certificate handling, streaming state and
complete authentication protocols need separate reviewed designs. Hashing a
password with SHA-256 is not a password storage scheme.
