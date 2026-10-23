# Pinned Mbed TLS source closure

Mbed TLS 3.6.7 LTS, upstream release archive:
https://github.com/Mbed-TLS/mbedtls/releases/download/mbedtls-3.6.7/mbedtls-3.6.7.tar.bz2

Archive SHA-256: `a7e8bcbec0e6f761b4af24f25677626b35f762f68eef79c08677a363212d11f6`.
The include/ and library/ C/header files and upstream notices are byte-for-byte
copies. zore_config.h is the reviewed local configuration. SHA256SUMS records
the source closure and configuration; README.zore.md and SHA256SUMS are excluded
from that manifest. The compiler embeds every listed file plus these metadata
files, allowing native builds from the distributed executable without a checkout
or network. Its closure digest hashes sorted path/byte pairs, each preceded by
a little-endian u64 length. Changes require a reviewed recipe fingerprint update.

The fixed configuration supports TLS 1.2 ECDHE RSA/ECDSA with AES-GCM,
SHA-256/384/512, explicit PEM roots/identities, and host entropy. No system TLS
library is used. Windows links the OS bcrypt entropy API. Upstream LICENSE,
SECURITY.md and SUPPORT.md retain license and maintenance information.

AES uses immutable precomputed tables, avoiding process-global mutable table
initialization. Hardware AES feature detection and acceleration are disabled in
this recipe. Software AES still uses secret-dependent table lookups; this
profile does not claim protection from cache-timing observation by hostile local
co-tenants or a whole-provider constant-time guarantee.
