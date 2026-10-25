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
the message-digest layer, HKDF, constant-time comparison and host entropy, plus
TLS 1.3 and 1.2 client and server connections. TLS 1.3 uses only ephemeral
ECDHE key exchange on P-256/P-384, AES-128/256-GCM, ECDSA or RSA-PSS
signatures, and middlebox compatibility mode; PSK, 0-RTT, session tickets and
resumption are not compiled in. TLS 1.2 is limited to ECDHE RSA/ECDSA key
exchange with AES-GCM on P-256/P-384, extended master secret, encrypt-then-MAC,
SNI, all fatal alerts and PEM roots and identities. RSASSA-PSS certificate
signatures are accepted because TLS 1.3 RSA authentication requires RSA-PSS.
TLS 1.3 needs the PSA crypto core (`MBEDTLS_PSA_CRYPTO_C` with a dynamic key
store) for its ECDHE keys, HKDF and Finished MACs, so the library sources now
include the `psa_*` core, `hkdf.c`, the `ssl_tls13_*` sources and the message
reader they use. PSA is not built with `MBEDTLS_USE_PSA_CRYPTO`, persistent key
storage or threading support: its key store and random generator are
process-global, so the TLS bridge initializes PSA once and serializes every
provider call process-wide. SHA-512 is the entropy
accumulator. No system crypto or TLS library is used. Windows links the OS
bcrypt entropy API. The compiled objects are cached per closure, compiler and
flags; vendored sources are compiled without warnings as errors.

`MBEDTLS_SSL_ALL_ALERT_MESSAGES` is kept deliberately: it only makes the provider
send fatal alerts such as `bad_record_mac` after a corrupted record, which TLS
conformance tests check, and enables no protocol feature.

AES uses immutable precomputed tables, avoiding process-global mutable table
initialization. Hardware AES feature detection and acceleration are disabled in
this recipe. Software AES still uses secret-dependent table lookups; this
profile does not claim protection from cache-timing observation by hostile local
co-tenants or a whole-provider constant-time guarantee.
