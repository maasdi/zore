# 0004 — HTTP/2 client protocol and resource model

Status: accepted as the plan for issue #290. **HTTP/2 is not implemented.** This
record fixes the choices that later slices must follow. Each slice still locks
its own public signatures, examples and conformance cases in the specification
before code is written. All four slices are delivered: application protocol
negotiation in the `tls` package, the header compression package, a
connection that multiplexes buffered requests, and an HTTPS client that
chooses HTTP/2 or HTTP/1.1 per server (specification §37.50). Streamed HTTP/2
bodies are not delivered; the ordinary `http.Client` calls still speak
HTTP/1.1 only.

## Consumer

A program that makes many calls at once to one HTTPS service, for example an
API gateway or a batch job. With HTTP/1.1 each call in flight needs its own
connection and TLS handshake. With HTTP/2 the calls share one connection, so the
cost is one socket and one handshake instead of one per call. Programs that make
calls one after another gain little and keep using the HTTP/1.1 pool.

The server side, a general framing library and gRPC are out of scope.

## Negotiation

- HTTP/2 is used only over verified TLS, chosen by application protocol
  negotiation (ALPN) with the name `h2`. Certificate and name checks are those
  of the existing provider; there is no weaker path.
- The client offers `h2` then `http/1.1`. If the server picks `http/1.1` or
  answers without a choice, the same connection is used by the existing
  HTTP/1.1 code. A server that picks a name that was not offered fails the
  handshake.
- No cleartext HTTP/2: no `h2c` upgrade and no prior-knowledge mode. Plain
  `http` addresses stay on HTTP/1.1.
- HTTP/2 is opt-in through its own constructor. `Do`, `Pool`, `Follow`,
  `DoRetry` and the other existing calls keep speaking HTTP/1.1 and never offer
  `h2`, so no existing program changes behaviour.
- The provider's TLS 1.2 suites are ECDHE with AES-GCM only, which the HTTP/2
  rules allow, so both TLS 1.3 and TLS 1.2 connections may carry `h2`.

## Where the code lives

The protocol engine lives in `std/net/http` beside the HTTP/1.1 client, so it
shares that client's request validation, limits, timeouts and response type. It
works on any `http.ClientConn` and has no TLS dependency, so scripted
connections can drive every frame in tests. Header compression is its own
bundled package, `std/net/http/hpack` (specification §37.48), because bundled
packages are tested only through their public names and its limits need direct
tests. The `tls/https` package only does the negotiation and hands over the
connection, as it does today for HTTP/1.1.

## Limits

Every limit is finite and has a default; a peer cannot raise one.

| Item | Decision |
| --- | --- |
| Frame size | Advertise and accept 16,384 bytes; larger frames are a connection error. Our own frames never exceed 16,384 bytes even if the peer allows more. |
| Header block | At most `ClientConfig.MaxHeaderBytes` (32 KiB by default) per decoded message, counted as HTTP/2 counts a header list: name plus value plus 32 bytes per field. The same bound applies to the compressed bytes across `HEADERS` and `CONTINUATION` frames. A larger list is a connection error, because the decoder stops and its table can no longer follow the peer's. |
| Header compression | Decoder table at most 4,096 bytes, advertised to the peer. A table size update above that, an index outside the tables, an integer longer than five bytes or bad Huffman padding is a connection error. The encoder sends literal fields without adding table entries, so the peer's table setting costs no memory here. |
| Streams | At most 32 open streams per connection by default, and never more than the peer allows. Extra calls wait in a bounded FIFO queue (256), like pool waiters. |
| Receive memory | 256 KiB window per stream, so one connection buffers at most 32 × 256 KiB of unread body. |
| Control frames | At most 64 unanswered `PING`, `SETTINGS` and reset frames are queued; more is a connection error (`ENHANCE_YOUR_CALM`). Empty `DATA` frames that do not end a stream are counted the same way. |
| Tasks and sockets | One socket and one task per connection, whatever the number of streams. A connection value has a single owner and a TLS connection cannot be read and written from two tasks, so the task alternates short bounded reads and writes instead of using a reader and a writer. |
| Connections | One live connection per origin, plus connections that are draining after `GOAWAY`. At most 64 origins keep a connection; a request beyond that uses a connection that closes after it. |

## Flow control

- The stream window is refilled only as the caller reads the body, so a slow
  reader pushes back on the server for that stream alone.
- The connection window is refilled as soon as bytes are stored in a stream's
  buffer. Because every stream buffer is bounded by its own window, a stalled
  stream cannot use up the connection window and block its siblings.
- Uploads wait for both windows. The wait is bounded by the context and by the
  write deadline; a peer that never sends a window update ends that stream with
  a timeout and a `RST_STREAM`, not the connection.
- The connection task never blocks in one direction. Each turn it writes at
  most one frame's worth of queued bytes and reads with a deadline of 1 to 16
  milliseconds, so a peer that stops reading cannot stop the client from
  reading, and the reverse. Replies it owes (settings acknowledgements, ping
  answers, window updates, resets) are queued ahead of new body data, and body
  data is produced only when little output is queued.
- Body data from different streams is written round-robin, one frame at a time.
  Stream priorities are neither sent nor honoured.

## Ownership and lifetime

- The connection manager is a Copy handle over shared, locked state, like
  `Pool`. Callers on any task may start streams on it.
- A response body is a Move resource, like `ResponseStream`. It owns its stream
  slot, not the connection. Buffered calls read the whole body under the usual
  `MaxBodyBytes` limit and release the slot before returning.
- Dropping or closing a body early, or cancelling its context, sends
  `RST_STREAM` with `CANCEL`, returns its window to the connection and frees
  the slot. Sibling streams keep running.
- `Close` on the manager stops new calls with a closed error and sends `GOAWAY`.
  Bodies that are still open may be read to the end; the socket closes when the
  last one is finished or dropped. Handles are Copy values, so dropping them
  is not observed: a connection that is never closed stays until the peer
  closes it, or until the per-origin manager of the last slice retires it.

## Errors

- A stream error (`RST_STREAM`, a bad body length, a malformed response) fails
  that stream only.
- A connection error (bad frame, compression failure, flood limit, TLS or
  socket failure) fails every open stream with the same error, sends `GOAWAY`
  when the socket still works and closes it.
- `GOAWAY` from the server stops new streams on that connection. Streams above
  its last-stream number, and streams refused with `REFUSED_STREAM`, fail with a
  distinct "not processed" error: the server promises it did nothing, so these
  may be retried for any method. New calls open a fresh connection.
- Every other failure after the request was sent follows the existing retry
  rules: the request may have been processed, so only safe methods repeat.
- Server push is disabled in the first settings frame; a `PUSH_PROMISE` is a
  connection error.
- Responses are validated before they are returned: one `:status`, no other
  pseudo-header, no connection-specific fields, lowercase names, and a
  `Content-Length` that matches the data. Interim 1xx answers are skipped and
  trailers are checked and discarded.

## Delivery order

1. Application protocol negotiation in `tls` (delivered; specification §37.10).
2. Header compression decoder and encoder with its limits (delivered;
   specification §37.48).
3. Frames, settings, flow control and the connection task over a scripted
   connection, with buffered requests (delivered; specification §37.49).
4. Negotiation and HTTP/1.1 fallback in `tls/https` with one connection per
   origin and interoperability evidence (delivered; specification §37.50).
   Streamed bodies remain a follow-up.

## Evidence required before HTTP/2 is called supported

Interoperability with an independent HTTP/2 server; malformed frames and
compression limits; slow readers and writers; flow-control stalls; cancelling
one stream while siblings progress; `GOAWAY` and shutdown cleanup with socket
and task counts; certificate checks and negotiation including fallback; plain
and async callers on one worker; Linux, macOS and Windows.

## Alternatives not taken

- **Cleartext HTTP/2.** Rarely deployed, needs a second negotiation path and
  gives no verified peer.
- **Automatic upgrade of existing calls.** It would change connection counts,
  error text and ordering for programs that did not ask for it.
- **A dynamic table in the encoder.** Saves bytes on repeated headers but adds
  state that must stay in step with the peer; it can be added later without an
  API change.
- **One task per stream.** Simple, but the task count would follow the peer's
  stream limit instead of a fixed bound.
