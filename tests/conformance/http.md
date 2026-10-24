# HTTP servers and request URLs

The contracts are locked in specification §37.6. Executable tests are in
`tests/codegen/http.rs`, with compiler acceptance/rejection coverage in
`tests/packages/packages.rs` and `tests/ownership/ownership.rs`.

| Area | Executable coverage |
| --- | --- |
| URL | UTF-8, percent escapes, query duplicate/order retention, all-or-error decoding, forbidden path delimiters and dot segments, repeated/trailing slashes |
| Headers | Case folding, duplicate values, injection rejected without changing existing values |
| Dates | Unix epoch, leap day, 1601/9999 boundaries, out-of-range failure, separate wall clock |
| Ownership | Two mutable facades sharing a socket slice, borrowed I/O and function adapters; rejection of clone, channel/Mutex/task escape and owned I/O conversion; compatible array and opaque handler loans retained |
| Routing | Literal precedence, decoded path params, crossing ambiguity rejection, HEAD fallback, OPTIONS/Allow, 404/405 |
| Function callbacks | Declared functions, inline Copy-handle captures, independent immutable snapshots, concurrent invocations; unsafe/opaque captures rejected |
| Convenience/lifecycle | Sticky registration, Text/JSON framing, serialization failure before commitment, ListenAndServeContext startup failures, default/configured ListenAndServe signal bootstrap, active-request drain, and signal cleanup after startup failure |
| OS signals | Real SIGINT/SIGTERM on Unix and isolated Ctrl+C/Break on Windows; overlapping subscriptions, parent cancellation, idempotent stop, restored defaults; direct Windows termination-callback mapping and bounded-wait tests |
| Framing | CL and chunked echo, valid quoted extensions, repeated CL, TE+CL, unsupported TE, signed/overflowing lengths, missing/repeated Host, invalid fields, version rejection, header overflow, early EOF, trailers rejected |
| Body/response | Known and streamed limits, sticky body error before commitment, Expect once, empty Read, Read after final commitment, 204/205/304/HEAD, length overrun/underrun, generic handler error, unread-body close |
| Buffered body spans | Multi-buffer CL and chunked bodies, chunk boundaries across refill, retained pipelined input, partial body EOF |
| Lifecycle | Pipelined responses, silent initial deadline, body and idle timeouts, capacity rejection/release, handler panic cleanup, poisoned factory registry, idle shutdown and repeated Shutdown |
| REST model | JSON string escapes and surrogate pairs, invalid/non-JSON escapes, duplicate/unknown fields, empty names, trailing input |

Tests use local TCP peers with bounded read/write waits and native leak checking.
Fault injection also checks byte-count preservation across partial writes,
zero-progress failure, sticky commitment after a failed final header, and
suppression of automatic 100 after that failure. A controlled clock verifies
that the initial first byte does not restart the acceptance/header budget,
while a later request gets its new header budget. A controlled scheduler queues
a second factory acquisition before the first factory panics, then checks both
completion guards and a later 503. Shutdown expiry is tested with a handler
blocked on an application channel: expiry returns before it finishes, request
cancellation is observable, and a later Shutdown waits successfully.

Windows close/logoff/shutdown cleanup is best effort under OS deadlines. Direct
callback tests do not claim real delivery of these events. The Ctrl+C/Break HTTP
cases exercise both convenience entry points and drain active requests before exit.

## Bounded outbound client

The client contract is locked under §37.11. `Client` owns an optional transport;
responses own headers and binary bodies, with no connection retained. `Do`
uses HTTP by default; a verified owned `ClientTransport` enables HTTPS through
the same method. `Exchange` consumes an already authenticated connection.
A custom connection reports only safely resumable poll timeouts as
`ClientPollTimeout`; all other transport errors terminate the exchange.

| Evidence | Required behavior |
| --- | --- |
| `http_client_real_fragmented_binary_framing_and_hostile_peers` | Real fragmented loopback CL/chunked/EOF binary bodies, HTTP/1.0 and reason-less status lines, UTF-8 and ISO-8859-1 obs-text header values and reason phrases, `client.Get`, JSON POST composition, informational/HEAD/204/304/205 handling, returned redirects/status failures, injection/framing/chunk/trailer rejection, 7,500 tiny chunks within the metadata bound and oversized chunk lines, unexpected EOF in framed bodies, body/metadata bounds, cancellation/deadline socket close |
| `http_client_absolute_urls_and_validation_precede_transport` | Absolute URL authority/default ports/IPv6/escapes, underscore hosts, stripped fragments, unsupported forms, controlled outbound headers, CRLF and method rejection, `http.Get`, and the separate request body limit before I/O |
| `http_client_partial_io_owned_transport_and_error_cleanup` | Exact partial write offsets, bytes with EOF, normalized retry timeout, terminal/no-progress/invalid-count failures, exactly one owned connection and transport drop |
| `http_client_panic_drops_owned_connection_and_transport` | Panic unwinds owned transport/connection and cancellation guard without leaks |
| `http_client_async_calls_suspend_and_cancel_against_real_peer` | Direct async waiting calls and cooperative cancellation |
| `http_client_frontend_checks_without_native_linker` | GET and JSON POST application type checking without native linker/provider |
| `http_client_response_cannot_escape_body_borrow` | Owned response body views do not outlive their backing response |
| `offline_https_do_verifies_peer_and_preserves_fragmented_binary_bytes` | Distributed offline compiler checks the TLS transport without native linkers, then real HTTPS `Do` verifies fragmented binary responses and rejects wrong trust, expired/future validity, and a trusted wrong-host certificate |

HTTPS integration evidence is supplied by the verified external TLS transport
fixture; the standard HTTP package does not depend on a native TLS provider.
Linux/macOS/Windows native CI runs the portable real-peer tests. A local run on
one platform alone does not establish results on the other hosts.
