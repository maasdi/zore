# Verified HTTPS client

From this directory, run `zore install --offline`, then `zore build --locked main.ore`.
Run `./main https://example.com/` (`main.exe` on Windows) to trust the system
roots, or `./main roots.pem https://localhost:8443/` to trust only the given PEM
roots, which override the system roots. The local dependency is the reviewed TLS example
package; no system TLS library or network dependency installation is required.

`https.NewDefaultClient(config)` and `https.NewClient(roots, config)` from the TLS
package's `tls/https` package return an `http.Client` whose `https.Transport` implements the standard HTTP
client's owned transport interface. It verifies the URL hostname, certificate
chain, and validity time, and maps resumable TLS poll timeouts to
`http.ClientPollTimeout`. Use `https.NewTransport` with
`http.NewClientWithTransport` to build the client yourself.
The HTTP client buffers bounded binary responses and closes each connection.
This example accepts HTTPS URLs only. URLs may use a DNS name or an IPv4 or
bracketed IPv6 address; an IP host must appear as an IP-address entry in the
server certificate.

`zore test --locked .` starts real TLS loopback peers. Tests cover fragmented
binary responses, IP-address URLs, and rejection of untrusted, expired, future,
and trusted wrong-host certificates. `testdata/wrong-host.pem` is a public test certificate
for `wrong.example`, valid from 2020 through 2100, using the TLS fixture's
publicly committed test private key. It is never a production credential.
