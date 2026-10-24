# TLS 1.2 package

**Trust roots must be supplied explicitly.** There are no system or platform
roots: a client trusts exactly the PEM certificates passed to
`NewClientConfig`, and an empty bundle is an error.

`tls/` is an external curated package, registered as `tls-mbedtls-v1`.
It statically links compiler-embedded Mbed TLS 3.6.7, without OpenSSL or network
access during builds. Install the local package lock before checking or testing:

```
zore install --offline examples/tls/tls
zore test --locked --offline examples/tls/tls
```

Clients require explicit PEM roots and a DNS name. They always verify the chain,
certificate validity against the host wall clock, and hostname; the DNS name is
also SNI. There is no verification bypass or platform root discovery. Only TLS
1.2 ECDHE RSA/ECDSA with AES-GCM is enabled, with the extended master secret
extension; fatal errors send their TLS alerts. Servers accept PEM chains and
unencrypted PEM private keys; key parsing verifies the matching certificate.
The fixtures' private keys are public test data, never deployment identities.
AES uses immutable precomputed tables; hardware acceleration and feature
detection are disabled. Software AES uses secret-dependent table lookups, so
this profile does not promise protection from hostile local cache-timing
observers or whole-provider constant-time execution.

Configs and connections are Move values. Each session owns independent parsed
certificates and keys, so configs can be shared by borrowing and dropped after
session creation. Configs check the certificate/key pair once; sessions copy the
parsed identity without repeating that check or holding the config lock.

The context passed to `DialContext`, `Client` or `Server` bounds only the dial
and handshake, like Go's `HandshakeContext`; canceling it afterwards does not
affect the connection. Later Read, Write and Close are bounded by
`SetDeadline`, `SetReadDeadline` and `SetWriteDeadline` (none by default). The
handshake replaces any deadlines already set on the TCP source, so bound it with
the context. Close attempts close-notify within the write deadline; drop
releases immediately without sending. EOF means an
authenticated close-notify; transport EOF without it is a truncation error.
Peer close-notify permanently ends application I/O: repeated reads return EOF
without provider or socket reads, and writes fail. Close sends one reciprocal
close-notify within the write deadline, then releases resources.

The native engine only processes bounded memory buffers. During the handshake
the Zore wrapper drives TCP with 100-millisecond deadline slices, checking
context cancellation between waits; afterwards each wait uses the connection's
own deadline, so an idle connection without one blocks without waking. Ordinary external functions currently
block callers, including async callers, using the TCP runtime's worker
compensation. They do not introduce provider socket waits or claim pollable TLS
frames. Dial selects an owned buffered helper result against context cancellation, so
canceled callers return while native resolution/connect may still run.
Noninterruptible platform DNS can outlive the configured timeout; the helper
and any late socket are eventually released when that native operation finishes.
Entropy acquisition uses the provider's OS source under worker compensation.

Read/Write accept call-scoped byte slices. A Write sends all of its data as
16 KiB records and reports the plaintext accepted, even if later ciphertext
flushing fails or times out. On timeout pending ciphertext and its offset are retained; retry only the
unaccepted plaintext suffix. The next Read or Write flushes pending ciphertext
first. Other transport or provider errors invalidate the session after a best-effort
flush of any queued alert; Close then only releases resources without another
provider operation. Native calls use mutex-protected handle leases;
there are no Zore pointers or callbacks retained by C.

`server/` accepts one loopback connection on port 8443, with a 30-second accept
limit, five-second handshake context, five-second I/O deadline and a 4096-byte
request buffer; it writes a fixed HTTP response and closes with close-notify. `client/` sends one bounded HTTP/1.1 request over
verified TLS, prints the first response read and closes with close-notify. Run each from its own directory after `zore install --offline`.
These are protocol examples; HTTP client integration is supplied separately by
the HTTP client package through its owned transport interface.

Conformance uses local certificates, public test keys and loopback sockets;
there is no Internet server dependency. The integration test copies the compiler
executable outside the repository, installs/checks offline with missing native
tools, then compiles and executes provider-present tests with leak checks. Linux,
macOS and Windows CI run the same test; evidence belongs to the actual CI result.
