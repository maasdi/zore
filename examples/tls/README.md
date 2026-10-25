# TLS 1.3 and 1.2 package

**Trust roots default to the platform's, and explicit roots override them.**
`tls.DefaultClientConfig()` trusts the system roots, like Go. `tls.NewClientConfig(rootsPEM)`
trusts exactly the PEM certificates passed to it and rejects an empty bundle, so a
mistakenly empty file never means "trust the system". `tls.SystemRoots()` returns
the platform roots as PEM. If `SSL_CERT_FILE` is set to a nonempty path, only that file is read and
failure is an error; otherwise Linux and other Unix hosts try
`/etc/ssl/certs/ca-certificates.crt`, `/etc/pki/tls/certs/ca-bundle.crt`,
`/etc/ssl/ca-bundle.pem`, `/etc/pki/tls/cacert.pem`,
`/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem` and `/etc/ssl/cert.pem` in
order and use the first with a usable certificate. macOS reads `/etc/ssl/cert.pem`,
which omits keychain-added and administrator roots; using the Security framework
is a follow-up. Windows reads the `ROOT` system store through crypt32 and may lack
roots Windows installs on demand. On every platform the certificates are loaded
into Mbed TLS, which verifies the chain, not the OS verifier, so OS distrust and
trust settings are not applied. `SSL_CERT_DIR` is not supported. Certificates the
provider cannot use, such as Ed25519 roots, and duplicate certificates are
skipped. The result is limited to 4 MiB, like explicit PEM inputs. The snapshot is taken at
call time, and since every session parses the configured roots again, large
bundles cost parse time and memory per connection; sharing one parsed root chain
per configuration is a follow-up.

`tls/` is an external curated package, registered as `tls-mbedtls-v1`.
It statically links compiler-embedded Mbed TLS 3.6.7, without OpenSSL or network
access during builds. Install the local package lock before checking or testing:

```
zore install --offline examples/tls/tls
zore test --locked --offline examples/tls/tls
```

Clients need trust roots (the system roots by default, or explicit PEM roots)
and a server name: a DNS name or an
unbracketed IPv4 or IPv6 address. They always verify the chain, certificate
validity against the host wall clock, and server name; a DNS name is also SNI.
An IP address matches only IP-address subjectAltName entries of the same
address, never DNS entries or the common name, and is not sent as SNI.
DNS names may have one trailing root dot; TLS removes it for SNI and certificate
matching while the HTTP Host authority keeps its spelling. Repeated trailing dots
and malformed IP forms are rejected. Bracketed
forms, zones and ports are rejected, so pass the unbracketed host. There is no
verification bypass. TLS 1.3 is preferred and TLS 1.2 is accepted. TLS 1.3 uses
ephemeral ECDHE on P-256/P-384, AES-GCM, ECDSA or RSA-PSS signatures and
middlebox compatibility mode, without PSK, 0-RTT, session tickets or KeyUpdate;
a peer's NewSessionTicket is ignored and a peer KeyUpdate fails the connection.
TLS 1.2 is limited to ECDHE RSA/ECDSA with AES-GCM and the extended master
secret extension. Fatal errors send their TLS alerts. Provider calls are
serialized process-wide because the PSA crypto core that TLS 1.3 needs has
process-global state, so concurrent connections do not run TLS cryptography in
parallel. Servers accept PEM chains and
unencrypted PEM private keys; key parsing verifies the matching certificate.
The fixtures' private keys are public test data, never deployment identities.
`testdata/ip.pem`, `ip-dns.pem` and `ip-cn.pem` are public self-signed test
certificates for the IP-address tests, using that public key and valid from their
creation until about 2100: the first has IP entries for 127.0.0.1 and ::1, the
second only DNS entries spelled like those addresses, the third only a matching
common name. `duplicate-roots.pem` repeats `roots.pem` twice.
`unsupported-root.pem` is a public Ed25519 test certificate, whose
private key was discarded, and `mixed-roots.pem` joins it with `roots.pem`; they
test skipping of unusable platform certificates. `rsa.pem` and `rsa-key.pem` are a
public self-signed RSA test identity for `localhost`, valid from creation until
about 2100.
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
verified TLS, prints the first response read and closes with close-notify. Run each from its own directory after `zore install --offline`. The client
uses the system roots unless given a roots file, so pair it with the fixture
server as `main ../tls/testdata/roots.pem` after `zore build`.
These are protocol examples. For HTTPS with the standard HTTP client, import
`tls/https`: `https.NewDefaultClient(http.DefaultClientConfig())` returns an
`http.Client` that trusts the system roots and verifies each URL's hostname, and
`https.NewClient(rootsPEM, ...)` does the same with only the given roots.
`https.NewDefaultTransport()` and `https.NewTransport(rootsPEM)` supply the same
transports for `http.NewClientWithTransport`. It is ordinary Zore source layered on `tls`, so
the `tls` package itself does not import HTTP.

Conformance uses local certificates, public test keys and loopback sockets;
there is no Internet server dependency. The integration test copies the compiler
executable outside the repository, installs/checks offline with missing native
tools, then compiles and executes provider-present tests with leak checks. Linux,
macOS and Windows CI run the same test; evidence belongs to the actual CI result.
