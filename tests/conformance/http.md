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

## Cookies and URL-encoded forms

The contract is locked under §37.16. Executable tests are in
`tests/codegen/http.rs`; compiler acceptance and rejection are in
`tests/packages/packages.rs::http_cookies_and_forms_check_their_receivers`.

| Scenario | Expected result |
| --- | --- |
| `FormatSetCookie` with every attribute and an `Expires` time in another zone | Attributes in the order `Path`, `Domain`, `Expires` (in GMT), `Max-Age`, `HttpOnly`, `Secure`, `SameSite`, with one leading `.` dropped from `Domain` |
| `MaxAge` negative, zero | `Max-Age=0`; left out |
| Values with space, `"`, `,`, `;`, `\`, or non-ASCII; empty or non-token names; paths with `;`, CR LF, a tab, or no leading `/` | `http.Cookie: invalid value`, `invalid name`, `invalid path` |
| Domains `-bad.com`, `a..b`, `.`, `a_b.com`, `a.b.`, a 64-byte label, a 254-byte name | `http.Cookie: invalid domain` |
| `SameSite` `lax`; `None` without `Secure` | `invalid SameSite`; `SameSite=None requires Secure` |
| `__Secure-` or `__SECURE-` without `Secure`; `__Host-` with a `Domain`, with `Path` `/x`, or without `Secure`; `__host-` with a `Domain` | Refused with the prefix rule |
| `Expires` at the end of 1600; at the start of 1601 | `Expires before 1601`; `Expires=Mon, 01 Jan 1601 00:00:00 GMT` |
| `SetCookie` twice, once with a bad cookie in between | Two `Set-Cookie` lines in order; the bad one adds nothing |
| `SetCookie` after `WriteHeader` | An error; no `Set-Cookie` sent |
| Two `Cookie` lines with quotes, tabs, empty pairs, a pair without `=`, an empty name, a name with a space, and a value with `"` inside | Well-formed pairs in order, quotes removed, the rest skipped |
| `Cookie("b")` with two `b` cookies; `Cookie("missing")`; no `Cookie` header; a lowercase `cookie` header | The first; an empty cookie and `false`; no cookies; read like `Cookie` |
| `ReadForm` of `a=1&b=x+y%21&a=2&&c` | `a=1`, `b=x y!`, `a=2`, `c=` in body order |
| Empty body; body exactly at the limit; one byte over | No fields; decoded; `io.LimitExceeded` (413) |
| `limit` 0 with an empty body; with one byte; `limit` -1 | No fields; 413; `http.Request.ReadForm: negative limit` |
| Media type in mixed case with `charset="UTF-8"`; a trailing `;` with or without `charset=utf-8` before it | Accepted |
| `charset="utf-8` with one quote; `x=a b`; `charset = utf-8`; `charset` twice | `http.Request.ReadForm: unsupported content type` |
| A quoted parameter holding `;` | Accepted |
| `application/json`, `charset=latin1`, no `Content-Type`, two `Content-Type` lines | `http.Request.ReadForm: unsupported content type` |
| Raw byte 255 in the body; `%FF`; `%zz`; raw UTF-8 `é` | `http.Request.ReadForm: body is not UTF-8`; `strings.FromBytes: invalid UTF-8` from `url.ParseQuery`; `url: invalid escape`; decoded |
| Body over the server `MaxBodyBytes` | 413 |
| Handler error after a good form | Generic 500 without the message |
| Server shutdown after all cases | Clean exit with leak checking on |

## Media types and file serving

The contract is locked under §37.17. Executable tests are in
`tests/codegen/http.rs`; compiler acceptance and rejection are in
`tests/packages/packages.rs::mime_types_and_file_serving_check_their_arguments`.

| Scenario | Expected result |
| --- | --- |
| `ParseMediaType` of `Text/HTML; Charset=UTF-8` | `text/html` with `charset=UTF-8`; writes back as `text/html; charset=UTF-8` |
| A quoted `boundary` with space and `;`; escaped `"` and `\`; an empty quoted value | Values decoded; written back quoted and escaped; parsing the output gives the same type and parameter count |
| Trailing `;` and empty parameters | Skipped |
| `""`, `text`, `text/`, `/x`, `a/b/c`, `a b/c` | `mime: invalid media type` |
| `a/b; x`, `a/b; x =1`, `a/b; x=1 y`, a quoted control byte, a quoted `é` | `mime: invalid parameter` |
| `a/b; x=1; X=2`; `a/b; x="open` | `mime: duplicate parameter`; `mime: unterminated quoted string` |
| `Parameter("CHARSET")`; a missing name; `charset` on a built value whose name is `Charset`; `Kelvin sign + "ey"` on `key` | The value and `true`; `""` and `false`; the value and `true`; `false` |
| `FormatMediaType` with type `text`, a name with a space, a value with a line break, a repeated name | Refused |
| `TypeByExtension` of `.html`, `.HTM`, `.js`, `.png`, `.JPEG`, `.woff2`; `.exe`; `""` | The table types; `""` and `false` for the last two |
| `GET` of `index.html`, `css/app.css`, a file with an unknown extension, an empty file, a 200,000-byte file | `200` with type, `nosniff`, exact length, `Last-Modified` in GMT, and the exact bytes; `application/octet-stream` for the unknown one |
| `HEAD` of the large file | `200`, `Content-Length: 200000`, no body |
| `POST` | `405` with `Allow: GET, HEAD` |
| `ServeFile` with a fixed path | The file |
| `ServeFileIn` with an empty root | An error and no response from it |
| Missing file, a folder, `.env`, `.git/config`, a missing file in a folder, `CON`, `nul.txt`, `Com1.log`, a trailing `.` or space, `:`, a tab | `404` with `404 not found`, no file bytes |
| A link to a file outside the root; a path through a linked folder; a named pipe (Unix) | `404`; the pipe is never opened |
| Names given straight to `ServeFileIn`: `""`, `/`, `a/`, `../secret.txt`, `/etc/passwd`, `a//b`, `css/../index.html`, `C:x`, `a:b:$DATA`, `CON .txt`, `conin$`, `CONOUT$.log`, `COM0`, `LPT9.txt`, `COM¹`, `GIT~1`, `ENV~1/config`, `index.html~`, `a\b` | `404` |
| `ServeFileIn` with `css/app.css` from the query | The file |
| `HEAD` of an empty file; `POST` to `ServeFileIn` with a missing name | `200` with `Content-Length: 0`; `405` |
| `%2e%2e`, `..%2f`, `%5c` in the request path | `400` from the server before the handler runs |
| Server shutdown after all cases | Clean exit with leak checking on |

## Multipart forms and uploads

The contract is locked under §37.18. Executable tests are in
`tests/codegen/http.rs::http_multipart_forms_stream_files_and_clean_up`;
compiler acceptance and rejection are in
`tests/packages/packages.rs::multipart_forms_own_their_uploads`.

| Scenario | Expected result |
| --- | --- |
| A preamble, a boundary line with trailing space and tab, a value holding CRLFs and `--XyZ-not-a-boundary`, a file named `C:\Users\ada\résumé.bin` of 150,000 binary bytes, a second file with no `Content-Type` and no bytes, an empty file input, a repeated `name`, and an epilogue | Values in order with `empty=`; files `résumé.bin` (`application/x-test`, exact size and checksum) and `second.txt` (`application/octet-stream`, 0 bytes); `Value` and `File` give the first |
| The same body in 997-byte pieces; a body with a 400-byte file in 3-byte pieces | The same results |
| `RemoveAll`, then `Open` | The file is still listed; `Open` fails |
| A handler error or a panic after reading the form | The upload folder is empty afterwards |
| 4 parts with `MaxParts` 3; a 17-byte file with `MaxFileBytes` 16; values of 9 bytes with `MaxValueBytes` 8; a part header over 128 bytes; a body over `MaxBytes` 40 | `io.LimitExceeded` (413), no files left |
| A 16-byte file with `MaxFileBytes` 16 | Accepted |
| No boundary, an empty or 71-character boundary, one ending in space, a `\` in it | `invalid boundary` |
| `multipart/mixed`, a form type, a malformed media type | `unsupported content type` |
| A negative limit | `negative limit` |
| No closing line, no boundary at all, a file cut short, `--XyZjunk` | `unexpected end of body` |
| `--XyZ` followed by space, tab, and text | `invalid boundary line` |
| No `Content-Disposition`, two of them, a header name with a space, a folded line | `invalid part header` |
| `attachment`, no `name`, an unterminated quote | `invalid content disposition` |
| A value byte 255, also after a file part | `value is not UTF-8`, no files left |
| A header byte 255 | `part header is not UTF-8` |
| `ParseDisposition` of `form-data; name="doc"; filename="résumé.pdf"`, `Form-Data;Name=a`, `form-data` | Lowercased type and names, UTF-8 file name kept |
| `ParseDisposition` of `""` and `form/data; name=a` | `mime: invalid disposition`; `mime: invalid parameter` |
