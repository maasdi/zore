# Pinned Mbed TLS source closure

Mbed TLS 3.6.7 LTS, upstream release archive:
https://github.com/Mbed-TLS/mbedtls/releases/download/mbedtls-3.6.7/mbedtls-3.6.7.tar.bz2

Archive SHA-256: `a7e8bcbec0e6f761b4af24f25677626b35f762f68eef79c08677a363212d11f6`.
The include/ and library/ C/header files and LICENSE are byte-for-byte copies.
zore_config.h is the reviewed local configuration. The library/ sources are
exactly those that compile to code under that configuration; the headers are
every file reachable from them through an `#include` directive, regardless of
conditionals. SHA256SUMS records the source closure and configuration;
README.zore.md and SHA256SUMS are excluded from that manifest. The compiler
embeds every listed file plus SHA256SUMS, allowing native builds from the
distributed executable without a checkout or network. Its closure digest hashes
sorted path/byte pairs, each preceded by a little-endian u64 length. Changes
require a reviewed recipe fingerprint update. Upstream release notes, security
policy and support information are in the release archive.

The fixed configuration supports AES, CTR-DRBG, SHA-256/384/512, HMAC through
the message-digest layer, constant-time comparison and host entropy, plus TLS
1.2 client and server connections limited to ECDHE RSA/ECDSA key exchange with
AES-GCM on P-256/P-384, extended master secret, encrypt-then-MAC, SNI, all fatal
alerts and explicit PEM roots and identities. SHA-512 is the entropy
accumulator. No system crypto or TLS library is used. Windows links the OS
bcrypt entropy API. The compiled objects are cached per closure, compiler and
flags; vendored sources are compiled without warnings as errors.

AES uses immutable precomputed tables, avoiding process-global mutable table
initialization. Hardware AES feature detection and acceleration are disabled in
this recipe. Software AES still uses secret-dependent table lookups; this
profile does not claim protection from cache-timing observation by hostile local
co-tenants or a whole-provider constant-time guarantee.
