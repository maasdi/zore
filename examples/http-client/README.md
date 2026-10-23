# Buffered HTTP client

Build with `zore build examples/http-client/main.ore`, then run
`./main http://localhost:8080/users` (`main.exe` on Windows) against a peer
accepting GET and JSON POST. The program prints both response statuses.

Requests use one connection each and return bounded, fully buffered binary
responses. Non-2xx statuses are ordinary responses. Redirects are returned
unchanged. The default transport supports HTTP; HTTPS requires an owned verified
TLS `ClientTransport` configured with `http.NewClientWithTransport`.
The runnable [`../https-client/`](../https-client/) example supplies that
transport with explicit trusted roots and URL hostname verification.

Cancellation returns promptly while a native dial helper may still be resolving
DNS or trying addresses. Its eventual connection is dropped; platform resolver
completion and delayed cleanup have no fixed deadline.
