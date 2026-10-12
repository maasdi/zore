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
