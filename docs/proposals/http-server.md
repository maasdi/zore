# HTTP server and URL proposal (issue #105)

Status: Accepted. Normative contracts are locked in specification §37.6.

This document preserves the reviewed design from PR #107. The implementation
uses shared connection slices and owns each facade's protocol state by value;
the original mutable-state-slice sketch below is historical. Type-aware
indirect-call provenance makes the handler boundary checkable without dropping
lifetime restrictions. See §37.6 and `tests/conformance/http.md` for the current
contract and executable coverage. General JSON and OS signals remain separate;
the REST example uses a local user-model codec and stdin-driven shutdown.

## Recommendation

Add `std/net/http` for bounded HTTP/1.1 servers and `std/net/url` for request
targets and query strings. Use ordinary waiting methods, as in §37.3, rather
than requiring async handlers. One task owns each accepted connection and serves
its requests sequentially. A handler accesses the connection through borrowed
request/writer views in that same task; the connection never crosses into a
second task while those views are live.

Use a shared-receiver `Handler` interface with a `Handle` method, plus a borrowed
function helper. The `http` package and parameter types identify the protocol;
`Handle` names the operation without repeating HTTP in every implementation.
Store owned handler factories in a sealed router, sharing its registry through
a Copy mutex handle. A factory makes an independently owned handler for each
request. This avoids inventing immutable shared ownership, cloning interfaces,
async closures, or support for function values inside owned interfaces.

This first version deliberately cannot register arbitrary closure values in a
persistent router. That limitation follows §§18.4, 20.2, and 22.2, rather than
being an implementation detail. Accepting persistent `HandleFunc` registration
would require a separate language decision about independently owned callable
values and their concurrency. Async handlers can be a later distinct interface;
there is no automatic conversion between plain and async entries.

## API sketches

All declarations below describe proposed signatures, not bundled sources or
programs accepted today. Types with private fields have constructors. Their
zero values are inert and safe to destroy. Fallible operations on an empty
server, request, writer, router, or builder return a documented error; infallible
request/writer getters return empty metadata, false, or a Background context.
No empty-state method indexes an empty internal view.

```ore
type Handler interface {
    Handle(w mut ResponseWriter, r mut Request) error
}

type HandlerFactory interface {
    NewHandler() (Handler, error)
}

func CallHandler(f func(mut ResponseWriter, mut Request) error,
                 w mut ResponseWriter, r mut Request) error

func NewRouterBuilder() RouterBuilder
func (b mut RouterBuilder) Handle(pattern string, factory own HandlerFactory) error
func (b own RouterBuilder) Build() (Router, error)

func DefaultConfig() Config
func NewServer(router Router, config Config) (Server, error)
func (s Server) Serve(listener own net.Listener) error
func (s Server) Shutdown(ctx context.Context) error
```

`CallHandler` calls a plain function or borrowing closure immediately. It neither
stores nor spawns it. A concrete handler can call a declared function through
this helper; that concrete handler and its factory contain only owned data and
Copy handles. Arbitrary captured function registration is excluded. Methods
that wait are ordinary calls: they suspend a suspending caller and block a
plain caller with worker compensation, following §§17.7 and 37.3.

`RouterBuilder` is Move. `Build` consumes it, validates routes, and seals the
registry. `Router` is a struct containing a Copy `Mutex<Registry>` handle;
the private registry contains route data and owned `HandlerFactory` values,
never slices or function values. `HandlerFactory.NewHandler` has a shared
receiver and must create a fresh handler or one holding Copy shared handles.
An owned interface cannot be cloned merely because its concrete value is Copy.

Dispatch locks the registry only to match a route and construct its handler;
the returned owned handler contains no views and is used after unlocking.
Factories must be short and avoid I/O, reentrancy, or calls to this router.
Factory construction is serialized; handler execution is concurrent across
connections. Application data is shared through channels or `Mutex<T>`;
handlers never run under the registry or application mutex by default.
For example, `UsersFactory` and `UsersHandler` can each hold
`Mutex<UserStore>` and a Copy route-kind discriminator. Registration takes
`UsersFactory`, and `NewHandler` returns a fresh `UsersHandler` interface.

### Request and response

`Request` is a Move, request-scoped facade with private fields. Proposed methods:

| Method | Contract |
| --- | --- |
| `Method() string`, `Path() string`, `RawQuery() string` | Validated method token, escaped path, and query without `?` |
| `Target() string`, `Host() string`, `RemoteAddr() string` | Original target, validated Host value, peer address |
| `Headers() Header` | Independent owned header snapshot |
| `Query() (url.Values, error)` | Parse query explicitly; malformed query is an error |
| `PathParam(name string) (string, bool)` | Decoded route parameter; absent gives empty text and false |
| `Context() context.Context` | Cooperative request cancellation and deadline |
| `mut Read(buf mut []byte) (int, error)` | Size-limited decoded body stream; satisfies `io.Reader` |
| `ContentLength() (int, bool)` | Declared length when present; false for chunked or absent |

The body is the request's `io.Reader` behavior, not an owned `Body` interface
field that secretly owns or duplicates the socket. Read bytes with
`io.ReadAll(r)` or stream them; arbitrary body bytes need not be UTF-8.
Request and writer are borrowed exclusively for the handler call. The handler
cannot retain either borrow after returning or pass it to a task. The server
retains both facades through response finalization, then drops them before
reusing parser storage. The following concrete representation uses existing
slice provenance (§11.7), shared slice aliasing (§12.3), and shared-receiver
Conn operations (§37.3); it requires no new borrowed-reference syntax.

### Ownership representation

The connection task moves its Conn into a private one-element fixed array.
It also owns separate one-element arrays for BodyState and ResponseState.
BodyState owns the input buffer, framing counters, and parsed request metadata;
ResponseState owns response headers, output buffer, and byte counters. They are
separate owners, not two mutable slices into one aggregate. Each exchange
constructs these private fields:

| Facade field | Request | ResponseWriter |
| --- | --- | --- |
| Connection view | shared `[]net.Conn` of the task's connection slot | shared `[]net.Conn` of the same slot |
| Protocol state view | exclusive `mut []BodyState` | exclusive `mut []ResponseState` |
| Exchange control | Copy `Mutex<ExchangeControl>` handle | Copy handle to the same control value |

ExchangeControl contains only owned/Copy state, such as the final-response-started
flag, the automatic-100 flag, and a sticky protocol error. It contains no Conn,
views, functions, or user callbacks. Its locks only read/update flags; release
them before socket I/O. The two facades execute sequentially in one connection
task, so no cross-task check/write atomicity is needed for their wire operations.
The shared Conn views can both call Read, Write, and deadline setters because
those methods already have shared receivers. They do not duplicate or own the
socket. Facade methods enforce the body/response order described below.

Both facade types define a harmless, non-I/O custom drop and no custom clone.
This makes them Move and non-clonable (§§8.3, 10.7), preventing a second parser
or writer capability through structural cloning. Their slice fields continue
to carry the task-local backing provenance; private fields do not hide it.
Constructors infer that provenance from their slice arguments, and zero facades
contain empty views. No facade is stored with its owners in a self-referential
aggregate or placed in the server/router's Mutex state.

The lifetime sequence is:

1. Create the connection/state owners, then build the two facades with shared
   connection views and disjoint exclusive state views.
2. Call the owned handler with `mut` borrows of both facades. A temporary
   borrowed io.Reader/io.Writer conversion forwards those loans for the call.
   Across a suspended call, owners and facades remain live in the same task.
3. After the handler returns, finalize the response through the writer, then
   explicitly drop both facades. Only then inspect/reset the state slots,
   preserve prefetched input for the next request, or drop the connection slot.
4. On error or panic, reverse-order cleanup drops facades before their backing
   owners. Facade drop performs no protocol finalization and cannot close a
   borrowed Conn; the owning connection slot closes it exactly once.

Reject converting either facade into an owned io interface, placing it in a
channel/Mutex, spawning with it (including an own argument), returning a facade
backed by local slots, or dropping/replacing a backing slot while a view remains
live. These follow existing no-view and lifetime rules rather than HTTP-specific
compiler exemptions. The acceptance proof still needs compiler checks of this
exact shape, including suspension and unwind paths; this document does not
claim those checks have run.

To process a body in another task, read a bounded owned `Array<byte>` first
and pass it to an `own` worker parameter. Copy metadata strings separately.
Streaming a live body into another task is excluded: planned retrieval does
not prove a borrow safe (§18.4). A handler can wait for the worker without
transferring the request or writer.

`ResponseWriter` has a mutable `Write(data []byte) (int, error)` entry, so it
satisfies `io.Writer`, plus these methods:

| Method | Contract |
| --- | --- |
| `mut SetHeader(name string, value string) error` | Replace all values before commitment |
| `mut AddHeader(name string, value string) error` | Append one value before commitment |
| `mut WriteHeader(status int) error` | Commit a final status in 200–599 |
| `mut Write(data []byte) (int, error)` | Implicitly commit 200, then write bytes |
| `Committed() bool` | Whether final-response emission has started; remains true after a partial-write failure |

Repeated commitment and header mutation after commitment return errors. Invalid
names, values, status, or framing headers fail before emitting bytes. The server
owns `Connection` and `Transfer-Encoding`; the handler may set a single valid
`Content-Length`. For responses that carry content, it must then write exactly
that many bytes; overrun fails without emitting excess, underrun closes the
connection. Without a declared length, body responses use chunked encoding,
finalized after the handler returns. Mark the exchange committed immediately
before attempting the first final-header byte; a partial-header failure is
terminal and must never permit a second response or automatic 100.
An empty uncommitted successful response is 200 with length zero.

Body eligibility is determined before choosing framing:

| Response | Body writes | Wire framing and Content-Length |
| --- | --- | --- |
| HEAD | Count/discard writes; no content bytes | Never emit chunks; an explicit length describes the corresponding GET, and zero writes do not constitute an underrun |
| 204 | Reject nonempty writes | No content, chunks, or Content-Length; reject a supplied length at commitment |
| 205 | Reject nonempty writes | Emit Content-Length: 0, no chunks; reject a supplied nonzero length before commitment |
| 304 | Reject nonempty writes | No content or chunks; an optional length describes the corresponding 200 representation and is not checked against bytes written |

The 204/205/304 status restrictions take precedence over HEAD write suppression.
Empty writes on an active writer are no-ops. Errors after commitment never
append an error response. These rules follow [RFC 9110 §8.6](https://www.rfc-editor.org/rfc/rfc9110.html#section-8.6)
and [§15.3.6](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.3.6).
Response trailers, interim handler responses, upgrades, CONNECT, hijacking,
and flush/streaming control are outside version one.

## Headers and common names

`Header` owns an ordered list of entries. Proposed operations are
`NewHeader() Header`, shared `Get(name) (string, bool)`,
`Values(name) Array<string>`, and `Entries() Array<HeaderEntry>`, plus mutable
`Set(name, value) error`, `Add(name, value) error`, and `Del(name) error`.
`HeaderEntry` has exported `Name string` and `Value string` fields.
Lookup folds ASCII case only; field names are nonempty HTTP tokens. Preserve
entry arrival order and original spelling. `Get` returns the first value;
`Values` preserves all occurrences without comma-joining. `Set` removes all
matching entries and appends the replacement. Reject CR, LF, NUL and forbidden
controls; version one accepts ASCII values and horizontal tab, trims surrounding
space/tab on input, and rejects obs-text rather than creating invalid strings.
This ASCII restriction must be documented as interoperability policy.

Provide `StatusText(code int) string` and integer constants including `StatusOK`,
`StatusCreated`, `StatusNoContent`, `StatusResetContent`, `StatusBadRequest`,
`StatusNotFound`,
`StatusMethodNotAllowed`, `StatusRequestTimeout`, `StatusContentTooLarge`,
`StatusExpectationFailed`, `StatusRequestHeaderFieldsTooLarge`,
`StatusInternalServerError`, `StatusNotImplemented`, and
`StatusServiceUnavailable`. Unknown codes have empty status text. Common header
string constants are `HeaderContentType`, `HeaderContentLength`, `HeaderHost`,
`HeaderAllow`, and `HeaderLocation`. No JSON reflection/helper API is invented.

## URL package and routing

Version one URL support is request-target focused, not a general URL client API.
Proposed functions:

| Signature | Contract |
| --- | --- |
| `ParseRequestTarget(target string) (Target, error)` | Origin-form only: nonempty path starting `/`, optional query; reject fragments and controls |
| `PathUnescape(text string) (string, error)` | Decode `%HH`; `+` stays plus; reject malformed escapes, invalid decoded UTF-8 and NUL |
| `QueryUnescape(text string) (string, error)` | Same, with `+` decoded to space |
| `PathEscape(text string) string` | Escape one segment; retain only ASCII unreserved bytes; uppercase hex |
| `QueryEscape(text string) string` | Escape one query component; unreserved bytes retained, space becomes `+` |
| `ParseQuery(text string) (Values, error)` | Split on `&`, first `=` separates key/value; ignore empty pairs; bare key has empty value |

`Target` has `Path string` (escaped) and `RawQuery string`. `Values` owns ordered
key/value entries, preserves duplicates and order, and has `Get`, `Values`,
`Entries`, `Add`, `Set`, `Del`, and `Encode` equivalents to Header (case-sensitive
keys). Query decoding is all-or-error, returning an empty value on failure;
semicolons are ordinary data. Encode preserves entry order. Invalid escapes
anywhere in a request target are rejected before dispatch; decoded path segments
must be valid UTF-8. Query UTF-8 validation occurs in `ParseQuery`.

Patterns are `METHOD /literal/{name}`. Methods are case-sensitive tokens.
Parameters occupy a whole segment, have unique identifier names, and match one
nonempty segment. No wildcard, catch-all, regex, optional segment, or host pattern.
Split the escaped path on literal `/` before decoding each segment; reject
decoded `/`, backslash, NUL, `.` and `..` segments. Thus `%2F` cannot change route
boundaries. Preserve repeated/trailing slashes and never redirect or normalize.
Pattern literal segments obey the same decoding rules.

Literal matches outrank parameter matches. Reject duplicate routes and overlaps
that have no unique specificity winner during Build, independent of registration
order (including crossing patterns `/a/{x}` and `/{y}/b`). Compare specificity
as a partial order: a winner is at least as specific at every segment, and more
specific at one. No path match returns 404; path match with no method match
returns 405 and a sorted, deduplicated Allow list. Explicit HEAD wins, otherwise
HEAD uses GET with body suppression. OPTIONS is explicit, otherwise a matching
path returns 204 and Allow including OPTIONS and inferred HEAD.

## Framing, limits, and connection lifecycle

Use strict CRLF parsing. Accept HTTP/1.1 origin-form with exactly one nonempty
Host; reject whitespace before colon, folded fields, invalid tokens, fragments,
and unsupported target forms. Other protocol versions receive 505 and close.
Absolute-form proxies, CONNECT, and `OPTIONS *` are excluded and documented.
Validate the entire header block before creating a request or invoking a handler.

Reject both Transfer-Encoding and Content-Length, any repeated/comma-list
Content-Length (even identical values), signed/overflowing/nondecimal lengths,
and any Transfer-Encoding other than one `chunked` coding. These failures are
400 and close. No framing fields means an empty body. Enforce decoded body
limits for both framing modes. Chunk sizes are bounded unsigned hex; bound chunk
metadata by the header limit. Accept syntactically valid bounded chunk extensions
without interpreting them. Reject trailers in version one, except an empty
trailer section. Premature EOF and malformed chunks invalidate the connection.
Unsupported expectations receive 417 and close. An oversized known length
receives 413 before dispatch or 100. For a validated `Expect: 100-continue`
request with a body, the first nonempty-buffer Read sends at most one automatic
100, before waiting for body bytes, only while no final response has started.
Empty-buffer reads emit nothing. Final-response commitment permanently disables
automatic 100, including when final-header transmission subsequently fails.

Version one requires handlers to read the body before committing a final
response. A subsequent Read returns `http.Request.Read: response already started`
without reading the socket or emitting 100. The server does not drain the body
through another path; if unread bytes remain, it closes after the response.
For example, WriteHeader(403) followed by Read returns that error and never
places an informational response after 403. Repeated reads before commitment
cannot emit a second 100. Informational responses precede final responses
([RFC 9110 §15.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.2));
this library restriction also avoids interleaving request reads with a partially
written final response.

Config has these exported integer fields; construct it with DefaultConfig.
Zero selects the default; negative values are configuration errors. No unlimited
mode is part of this initial safety contract.

| Field | Default | Meaning |
| --- | --- | --- |
| `MaxHeaderBytes` | 32768 | Request line, header lines and terminating CRLF combined |
| `MaxBodyBytes` | 1048576 | Decoded body bytes per request |
| `MaxConnections` | 1024 | Accepted active connections |
| `ReadHeaderTimeout` | 5 seconds | First request: absolute budget from acceptance; subsequent requests: from first byte observed for that request |
| `ReadBodyTimeout` | 30 seconds | Absolute budget after header validation |
| `WriteTimeout` | 30 seconds | Absolute budget from handler entry through final write |
| `IdleTimeout` | 60 seconds | Wait for first byte of the next request |
| `AcceptPollInterval` | 100 milliseconds | Bounded shutdown/cancellation observation |

Durations are nanoseconds; the constructor checks range and deadline overflow.
For the first request, record the acceptance time and set the read deadline
before any socket read or task scheduling delay. The entire header block must
arrive by acceptance plus ReadHeaderTimeout; receiving the first byte does not
restart that budget. A silent peer is closed at that deadline without a 408.
Polling timeouts check control state but never extend the absolute deadline.

After a completed response, IdleTimeout bounds the wait for the next request's
first byte. Observing that byte starts a fresh ReadHeaderTimeout; when bytes
were prefetched while serving the preceding request, start this header budget
when the connection loop begins parsing the next request, without an idle wait.
An expired idle wait also closes silently. Shutdown polling applies in all phases,
including the first-byte wait immediately after acceptance.

Header count and chunk metadata are bounded by MaxHeaderBytes; no unbounded
allocation precedes validation. Exceeding connection capacity closes the newly
accepted connection without parsing it. Header overflow is 431 and close.
Once header bytes have arrived, a header/body timeout is 408 and close when no
final response was committed and a bounded write is possible. Unknown methods
may route if valid tokens.

The connection task owns the Conn, parser buffers, and response serialization.
It serves one request at a time, including pipelined input; responses stay in
request order. Keep-alive is default unless either side requests close or framing
fails. If the handler leaves body bytes unread, send its response and close;
do not start an unbounded drain. Once a streamed body exceeds the limit,
Read records a stable limit error and marks the connection for closure after a
bounded error response; emit 413 only if uncommitted. A handler cannot erase a
parser/body error by returning nil. Close immediately if a response cannot be
sent safely.

Set socket deadlines before the associated wait, using std/time's monotonic
clock. Context cancellation alone does not interrupt a socket wait (§37.4).
Bound waits to the earlier phase deadline or a control polling deadline, check
the context after each timeout, and retry only polling timeouts within the phase
budget. A write retry advances by the reported count and never repeats bytes.
Do not promise interruption of arbitrary handler computation or external I/O.
Request context expires at the handler/write budget and is cancelled on handler
completion or shutdown expiry. Successful handler completion does not cancel the
server's response-finalization writes: they retain the original absolute write
deadline and shutdown control. Graceful shutdown itself permits active requests
to finish. There is no preemptive handler cancellation.

## Errors, panics, and shutdown

A non-nil handler or factory error becomes a generic 500 if uncommitted,
except that sticky body/framing failures take precedence: malformed framing is
400, a body limit is 413, and an expired body-read budget is 408. This priority
also applies when a handler propagates a Read error with `?` or returns nil.
After commitment, close without appending another response. Never expose error
messages to clients. I/O failure affects one connection. Serve returns listener
or configuration failures; per-request failures do not stop the server.

Panics remain uncatchable. Detach connection task handles so a handler panic
unwinds that task, drops its socket, and is reported by the runtime; never
retrieve its handle in the accept loop. A panic does not promise a 500 response.
A private per-connection completion guard must release the capacity permit and
signal completion on normal exit, error, and panic. It must not panic during
drop. An application mutex held during panic is poisoned; other handlers using
that mutex may also fail. A factory panic while holding the registry lock poisons
the router. Dispatch checks isPoisoned before acquisition: if it observes poison,
return 503 without entering the mutex. That check is advisory, not atomic with
withLock. A dispatch that checked before another factory panicked, including one
already queued for the lock, may itself panic when acquiring the poisoned mutex
under §20.2. Its connection closes without a promised response; there is no
recovery or guaranteed 503 for these racing requests. Already-created handlers
outside the registry lock may finish normally.

The accept loop and completion guards never acquire the registry mutex. Their
private accounting state contains no user callbacks and is separate from it,
so router poison cannot cause a second panic during a connection's cleanup.
After poisoning, the router remains unusable; replace the server/router to
resume service. This differs from a handler panic outside the registry lock,
which does not itself poison the router.

Server is a Copy control handle with mutex/channel-backed internal state; Serve
is allowed once. It consumes the listener and owns it in the accepting task.
Shutdown transitions running to draining idempotently, rejects new admissions,
and waits for all admitted connections' completion guards. Accept uses finite
deadlines so its owner observes the transition and closes the listener. Shutdown
may return after the accepting loop acknowledges listener closure, not merely
after it requests closure. Idle connections close at their next bounded poll;
active connections finish their current response and close without dispatching
another request. Admission and the draining transition are synchronized.

Serve returns nil after draining completes. Shutdown returns nil only after the
accept loop and every admitted connection have completed. If its context expires,
Shutdown returns its context error and requests cooperative cancellation; the
server stays draining. It cannot promise all tasks have stopped, close a Conn
owned by another task, or kill a CPU-bound handler. A later Shutdown can continue
waiting. Main must remain alive until Serve and successful draining finish;
dropping handles or returning from main is not graceful shutdown (§18.11).

## Target REST API and dependencies

The target `examples/rest-api/` will expose GET /users, GET /users/{id}, and
POST /users with application/json. Use `UsersFactory`/`UsersHandler` sharing a
mutex-protected owned store. Snapshot store data under a short lock; serialize
and write after unlocking. Parse ids explicitly (bad syntax 400, absent user
404). POST reads bounded bytes, checks content type (415), parses JSON (400),
updates under the lock, then returns 201 with Location. GET returns 200;
unsupported methods use router 405. Specify JSON models after the separate
JSON design is accepted; do not invent codec signatures or pretend this example
currently builds. The target behavior is proposed for agreement here.

Dependencies and acceptance checkpoints:

1. Validate the concrete slice-backed facade representation above with compiler
   acceptance/rejection cases and suspension/unwind cleanup checks. It proposes
   no new view type; an implementation gap must be resolved against the existing
   rules rather than bypassing provenance.
2. Maintainer accepts the factory router and restricted function form, or locks
   a callable-sharing extension before promising persistent HandleFunc.
3. Add std/net/url contracts above with positive and negative conformance cases.
4. Calendar UTC formatting for Date needs new std/time functions: Now is
   monotonic and cannot produce an HTTP date. Specify a separate wall-clock
   source and IMF-fixdate formatter; until then HTTP implementation is blocked
   on emitting Date correctly, not permitted to fabricate it from Now.
5. SIGINT/SIGTERM integration needs a separately specified std/os signal channel.
   Shutdown itself works with a context/control task and does not depend on
   signals. Do not call an invented signal API in a runnable example.
6. JSON is an application/example dependency, not an HTTP framing dependency.

## Compiler impact and conformance plan

No changes to async matching, panic recovery, task borrowing, interface cloning,
or shared socket ownership are authorized. Existing waiting-call lowering must
work through Handle and io interfaces. The proposed facade layout uses
existing slice-containing structs and shared Conn methods; validate its inferred
provenance and cleanup before claiming implementation support. Library state must
obey Mutex's no-view/no-function restriction and ordinary zero-resource cleanup.

After acceptance, add spec sections under §37 and conformance documents for:

- Plain handlers and functions; reject async mismatch and stored callable views.
- Two concurrent connections using owned handlers and shared application mutexes;
  reject spawning a request/writer borrow even with an immediate wait.
- Header case folding, duplicate/order retention, CRLF injection and obs-text.
- Route precedence, ambiguous registrations, HEAD, OPTIONS, 404/405, escaped
  delimiters, dot segments, duplicate query keys and invalid UTF-8.
- Content-Length, chunked framing, empty bodies, TE/CL ambiguity, overflow,
  truncation, extensions, trailers, Expect, body limits and unread-body close.
- Response framing, partial writes, zero progress, commitment errors, HEAD,
  204/205/304, over/underwritten lengths and errors before/after commitment.
- Acceptance-to-header, body/write/idle deadlines, capacity, polling cancellation
  and pipelining.
- Shutdown racing admission, idle peers, active requests, deadline expiry,
  repeated calls, a noncooperative handler, and main completion ordering.
- Handler panic closes only its connection, releases admission/completion state,
  and preserves the accept loop; factory/application mutex poison is explicit.
- Inert zero values and deterministic normal/error/panic cleanup.

The following regression scenarios are required before implementation is accepted;
they are test specifications, not passing executable tests:

| Scenario | Required observation |
| --- | --- |
| Admit MaxConnections silent peers | Each closes at its acceptance/header deadline; permits are released and a new client can be admitted |
| Send the first byte just before the initial deadline | The original deadline still expires; the header does not receive a fresh budget |
| Start shutdown during an initial or keep-alive first-byte wait | The connection observes draining within a control poll and releases its completion guard |
| Queue dispatch B after a false poison check while factory A holds the lock, then panic A | A and B unwind only their connection tasks; permits/completion are released, the accept loop survives, and a later poison-observing dispatch receives 503 |
| Read an Expect body in several pieces, including an initial empty-buffer read | Exactly one 100 precedes the final response; the empty read emits nothing |
| Commit 403 before the first Expect body read | Read returns response-already-started; no 100 or socket read occurs and unread-body policy closes the connection |
| Fail partway through final-header transmission, then attempt Read | Commitment stays sticky; no automatic 100 or replacement final response |
| Commit 205, attempt a nonempty Write, then finalize | Write fails, the wire has Content-Length: 0 and no content or chunks |
| Supply nonzero Content-Length for 205 | Commitment fails before bytes are emitted |
| HEAD or 304 carries a valid representation length without body writes | No chunks or spurious content-length underrun |
| Propagate a body-limit error with ? | Uncommitted response is 413, not generic 500 |
| Complete a successful handler before final chunk emission | Request context cancellation does not suppress bounded server finalization |
| Construct both facades from the task-local slots, call through borrowed I/O interfaces, then drop them | Shared connection loans coexist; disjoint mutable state loans end before slot reuse |
| Attempt facade clone, task/channel/Mutex escape, owned-interface conversion, or premature owner drop | Compiler rejection under existing clone/provenance rules |
| Suspend or panic during a facade operation | Backing owners remain live through suspension; unwind drops facades before the sole Conn owner |

Runtime tests should use local loopback, controlled peers and bounded waits;
use synchronization gates for the poison race and a controlled clock for deadline
boundaries. Do not rely on Internet endpoints or sleeps as evidence of completion.
The original design PR added no implementation. The subsequent implementation
and executable coverage are described in `tests/conformance/http.md`.
