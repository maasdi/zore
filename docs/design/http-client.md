# HTTP client extension plan

Issue [#228](https://github.com/maasdi/zore/issues/228) tracks additive extensions
to the bounded HTTP/1.1 client. This document orders the work and records design
constraints. It is not a locked public API or evidence of implemented features.
Each implementation must first lock its own signatures, examples, ownership,
errors, async behavior and conformance cases under spec §53.

## Consumer priorities and delivery order

These are representative workloads, not claims of demand from named users.
Choose small, independently testable increments rather than one client rewrite.

| Priority | Consumer workload | Follow-up | Dependency |
| --- | --- | --- | --- |
| 1 | Download an artifact into a file or hash with bounded memory | [#286](https://github.com/maasdi/zore/issues/286): streamed responses | Existing composable I/O and client framing |
| 2 | Upload a file without making a second complete copy | [#287](https://github.com/maasdi/zore/issues/287): streamed requests | Response ownership decision |
| 3 | Repeated calls to one service with bounded connection cost | [#288](https://github.com/maasdi/zore/issues/288): HTTP/1.1 pooling | Body completion, retirement and close rules |
| 4 | Follow artifact redirects, tolerate selected failures, use explicit proxies or decode content | [#289](https://github.com/maasdi/zore/issues/289): policy slices | Separate consumer-selected contracts and PRs |
| Separate project | Multiplex concurrent calls over HTTP/2 | [#290](https://github.com/maasdi/zore/issues/290) | Protocol/resource model and verified TLS ALPN |

The existing `Get`, `Client.Get`, `Do` and `Exchange` retain their owned buffered
results, limits and one-exchange connection behavior. HTTPS is already available
through the verified external TLS transport. Every extension must preserve
certificate/hostname verification and explicit provider errors.

## Streamed downloads first

The response/body must be a Move resource owning the connection, parser state,
bounded context and cancellation guard. Returning headers must not cancel that
context or drop the transport. Reads borrow the body mutably; the body can
outlive the initiating call without borrowing a caller-local connection.
Decide whether status/headers belong to that resource or to a separate response
record before locking the API; do not publish placeholder declarations.

The first slice sends existing buffered requests and closes each connection.
An explicit consuming Close and deterministic drop close immediately, discarding
unread buffered data without network draining. Drop cannot report errors or
perform an unbounded wait. Specify Close error precedence and repeated/zero-value
behavior in the contract. A completed body releases its connection promptly.

The total timeout starts before dialing and runs until body completion or close;
the earlier caller deadline wins. Read loops retain bounded cancellation polling.
Custom providers retain their existing deadline/count obligations. Plain calls
use worker compensation and async calls use existing suspension rules.

Retain cumulative metadata and body limits even though memory is bounded.
Fixed-length, chunked and EOF-delimited framing must be incremental. Validate
chunk CRLF, extensions and trailers before declaring completion. Define when
trailers become observable and distinguish absent trailers from an incomplete
body. HEAD, informational responses, 204, 205 and 304 keep their framing rules.
Bytes already returned remain valid when a later error occurs. Specify
bytes-plus-error handling, sticky terminal errors and zero-length reads; never
expose framing bytes or convert a truncated fixed-length body to clean EOF.

The download workflow is: open the response, copy its body into an owned file or
digest with `io.Copy`, then close. On error a partial destination may exist;
applications choose temporary-file publication or deletion. There is no implicit
rollback of an arbitrary writer.

## Streamed uploads

Start with an explicitly owned Reader and a known nonnegative content length.
The separate contract must decide ownership on validation/dial failure, exact
length accounting, premature EOF, invalid/no-progress counts and source errors.
Do not probe for excess bytes with an unbounded extra read. Bound declared and
sent bytes; preserve cancellation through upload and response consumption.

The first upload slice excludes automatic replay, unknown-length chunked uploads,
duplex exchange and Expect: 100-continue. Define early server rejection and
partially sent requests explicitly. Errors can leave server-side effects even
when no response was obtained. A consumed Reader is not a replayable request.

## Bounded connection reuse

Pooling requires an owned lifecycle with finite global/per-origin active and
idle limits, bounded waiter bookkeeping, cancellable acquisition, idle expiry
and explicit shutdown. Decide fairness, timer ownership and shutdown with live
leases before implementation. Drop closes idle resources and cannot silently
detach an unbounded cleanup task.

Pool keys include normalized scheme, host, effective port and transport/TLS
trust and identity configuration. A connection is leased, reusable-idle or
closed, with exactly one owner. Only a fully consumed, valid, unambiguously
framed response with validated trailers and no unread bytes can return it.
Early close, framing errors, cancellation, EOF-delimited bodies, upgrades and
Connection: close retire the connection. Early body drop never drains it for
reuse. Reset per-operation deadlines on each lease. A stale idle socket may
fail; reusing it does not authorize replay.

## Separate opt-in policies

Redirects need a finite hop budget, loop detection, relative Location resolution
followed by strict HTTP admission, status-specific method/body rewriting and one
total deadline. Strip Authorization, Cookie and Proxy-Authorization across
origins; decide HTTPS downgrade policy explicitly. Authenticate every new TLS
destination. URI resolution work in PR #284 is a dependency until merged.

Retries need bounded attempts/backoff, explicit idempotency policy and replayable
body factories. Partial writes and broken reused connections do not prove that
the server did not process a request. Report partial failures and retire failed
connections; never automatically retry a consumed streaming source.

Proxies start with explicit configuration. Lock absolute-form and CONNECT
behavior, proxy credential scope, target validation, separate proxy/target TLS
verification and tunnel cleanup. Environment discovery is a separate decision.
No proxy mode may introduce an insecure TLS fallback.

Content decoding is opt-in. Define supported encodings, raw representation
access, header/length transformations, encoded and decoded limits, expansion
bounds, checksum/truncation/concatenation policy and partial-output errors.
Reuse existing compression packages; their availability alone does not define
HTTP negotiation or decoding policy. Each policy gets its own implementation PR.

HTTP/2 remains independent: decide ALPN/fallback, cleartext policy, framing and
HPACK limits, stream/connection concurrency and flow control, cancellation,
GOAWAY/drain, server push, fairness and finite memory/task/socket bounds.
Specify stream ownership and error scope before sharing a connection.

## Acceptance evidence for each increment

Use scripted connections for deterministic malformed framing, bytes-plus-error,
no-progress, limit boundaries and cancellation timing. Add real TCP and verified
HTTPS fixtures; avoid public-internet dependencies. Streaming tests must show
delivery before the peer finishes and memory independent of total body size.
Pair supported APIs with frontend ownership/privacy rejection cases.

Assert resource cleanup on completion, early Close, error, cancellation and panic,
including socket/task counts and one-worker plain/async progress. Pool tests
also cover capacity saturation, waiter cancellation, idle expiry, stale sockets,
TLS isolation and shutdown with active leases. Policy tests cover credential
leaks, replay failures and decoding expansion boundaries. HTTP/2 needs independent
protocol interoperability and flow-control/deadlock tests. Run the supported
Linux/macOS/Windows matrix before reporting a slice complete.
