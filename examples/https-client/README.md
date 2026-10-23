# Verified HTTPS client

From this directory, run `zore install --offline`, then `zore build --locked main.ore`.
Run `./main roots.pem https://localhost:8443/` (`main.exe` on Windows) with
explicit trusted PEM roots. The local dependency is the reviewed TLS example
package; no system TLS library or network dependency installation is required.

`securehttp.Transport` implements the standard HTTP client's owned transport
interface. It verifies the URL hostname, certificate chain, and validity time,
and maps resumable TLS poll timeouts to `http.ClientPollTimeout`.
The HTTP client buffers bounded binary responses and closes each connection.
This example accepts HTTPS URLs only.

`zore test --locked .` starts real TLS loopback peers. Tests cover fragmented
binary responses and rejection of untrusted, expired, future, and trusted
wrong-host certificates. `testdata/wrong-host.pem` is a public test certificate
for `wrong.example`, valid from 2020 through 2100, using the TLS fixture's
publicly committed test private key. It is never a production credential.
