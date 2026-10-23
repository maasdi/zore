# TLS 1.2 package

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
1.2 ECDHE RSA/ECDSA with AES-GCM is enabled. Servers accept PEM chains and
unencrypted PEM private keys; key parsing verifies the matching certificate.
The fixtures' private keys are public test data, never deployment identities.

Configs and connections are Move values. Each session owns independent parsed
certificates and keys, so configs can be shared by borrowing and dropped after
session creation. Close attempts close-notify within the active context and
operation deadlines; drop releases immediately without sending. EOF means an
authenticated close-notify; transport EOF without it is a truncation error.

The native engine only processes bounded memory buffers. The Zore wrapper drives
TCP with ten-millisecond deadline slices, checking context cancellation and the
stored operation deadlines between waits. Ordinary external functions currently
block callers, including async callers, using the TCP runtime's worker
compensation. They do not introduce provider socket waits or claim pollable TLS
frames. Dial selects an owned buffered helper result against context cancellation, so
canceled callers return while native resolution/connect may still run.
Noninterruptible platform DNS can outlive the configured timeout; the helper
and any late socket are eventually released when that native operation finishes.
Entropy acquisition uses the provider's OS source under worker compensation. Configure contexts to bound whole sessions.

Read/Write accept call-scoped byte slices. A Write accepts at most 16 KiB per
call. It reports plaintext accepted, even if later ciphertext flushing times
out. On timeout pending ciphertext and its offset are retained; retry only the
unaccepted plaintext suffix. The next Read or Write flushes pending ciphertext
first. Other transport or provider errors invalidate the session. Cancellation of the stored
context ends future operations. Native calls use mutex-protected handle leases;
there are no Zore pointers or callbacks retained by C.

`server/` accepts one loopback connection on port 8443, with a 30-second accept
limit, five-second session context and a 4096-byte request buffer; it writes a
fixed HTTP response and closes. `client/` sends one bounded HTTP/1.1 request over
verified TLS. Run each from its own directory after `zore install --offline`.
These are protocol examples; HTTP client integration is supplied separately by
the HTTP client package through its owned transport interface.

Conformance uses local certificates, public test keys and loopback sockets;
there is no Internet server dependency. The integration test copies the compiler
executable outside the repository, installs/checks offline with missing native
tools, then compiles and executes provider-present tests with leak checks. Linux,
macOS and Windows CI run the same test; evidence belongs to the actual CI result.
