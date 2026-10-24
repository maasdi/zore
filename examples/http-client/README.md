# Buffered HTTP client

Build with `zore build examples/http-client/main.ore`, then run
`./main http://localhost:8080/users` (`main.exe` on Windows) against a peer
accepting GET and JSON POST. The program prints both response statuses.

`http.Get(ctx, url)` sends a GET with default limits; `client.Get` and
`client.Do` reuse a configured client. Requests use one connection each and
return bounded, fully buffered binary responses. HTTP/1.0 and HTTP/1.1 replies
are accepted. Non-2xx statuses are ordinary responses. Redirects are returned
unchanged. The default transport supports HTTP; HTTPS requires an owned verified
TLS `ClientTransport`, such as `https.NewTransport` from the `tls/https`
package of the TLS example. The runnable [`../https-client/`](../https-client/)
example uses it with explicit trusted roots and URL hostname verification.

Cancellation returns promptly while a native dial helper may still be resolving
DNS or trying addresses. Its eventual connection is dropped; platform resolver
completion and delayed cleanup have no fixed deadline.
