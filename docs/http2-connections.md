# HTTP/2 connections

HTTP/1.1 handles one request at a time on a connection. HTTP/2 lets many
requests share one connection: each request and its answer travel as small
numbered pieces, mixed together, so a slow answer does not hold up the others.
`HTTP2Conn` runs such a connection for you. The full contract is in the
[specification](../spec/language-spec.md#3749-http2-client-connections--locked).

Most programs should not build `HTTP2Conn` themselves: use the automatic
client below, which asks each server whether it speaks HTTP/2. `HTTP2Conn` is
for a connection on which both sides have already agreed to speak HTTP/2 (with
TLS, the `h2` protocol name; see the `tls` package's `SetProtocols`). The
ordinary client calls (`Do`, `Pool`, `Follow` and the others) still speak
HTTP/1.1.

## The automatic client

`https.NewAutoClient(roots, config, limits)` from the `tls` package (or
`https.NewDefaultAutoClient(config, limits)` with the system's trust roots)
does the asking for you:

- For each `https` server it connects to, it asks for HTTP/2. If the server
  agrees, it keeps that one connection and sends every later request to that
  server over it, many at the same time.
- If the server prefers HTTP/1.1, that request goes over HTTP/1.1 and the
  connection closes afterwards, as with the ordinary client.
- Plain `http` addresses always use HTTP/1.1.
- When several requests start at once for a new server, only the first one
  connects; the others wait for its connection.
- It keeps connections to at most 64 servers. `Stats().HTTP2Connections` says
  how many it holds; `Close` closes them all.

```ore
let client = https.NewDefaultAutoClient(http.DefaultClientConfig(), http.DefaultHTTP2Config())?
let empty = Array<byte>{}
let response = client.Do(context.Background(), "GET", "https://example.test/", http.NewHeader(), empty[:])?
client.Close()
```

## Using a connection directly


```ore
package main

import "std/context"
import "std/net/http"

func fetchBoth(client http.Client, agreed own http.ClientConn) (int, error) {
    let conn = client.NewHTTP2Conn(agreed, http.DefaultHTTP2Config())?
    let empty = Array<byte>{}
    let first, firstErr = conn.Do(context.Background(), "GET", "https://example.test/a", http.NewHeader(), empty[:])
    if firstErr != nil {
        conn.Close()
        return 0, firstErr
    }
    let second, secondErr = conn.Do(context.Background(), "GET", "https://example.test/b", http.NewHeader(), empty[:])
    conn.Close()
    if secondErr != nil {
        return 0, secondErr
    }
    return first.StatusCode + second.StatusCode, nil
}

func main() {}
```

`conn` is a small handle you can copy and use from several tasks at once; that
is the point of HTTP/2. `Do` takes the same arguments as `client.Do` and returns
the same `ClientResponse`, with the whole body in memory.

Call `Close` when you are done. It stops new requests, lets running ones finish
and then closes the connection. A connection that is never closed keeps one
background task alive until the server hangs up.

## Limits

`HTTP2Config` bounds what one connection may use. Zero means the default.

| Field | Default | Meaning |
| --- | --- | --- |
| `MaxStreams` | 32 | Requests running at once. The server may set a lower number. |
| `MaxWaiters` | 256 | Requests waiting for a free slot. More fail at once with `http2: too many waiting requests`. |
| `StreamWindow` | 262,144 | Bytes of one answer the server may send before the client has taken them in. |

The client's own settings still apply to each request: `Timeout`,
`MaxHeaderBytes`, `MaxBodyBytes` and `MaxRequestBodyBytes`.

## What can go wrong

Errors come in two sizes.

**One request fails, the others carry on:**

| Error | Meaning |
| --- | --- |
| `http2: request not processed` | The server refused the request or announced it is shutting down before looking at it. Nothing happened on the server, so any request may be sent again on a new connection. |
| `http2: stream reset by server (code N)` | The server gave up on this request. |
| `http2: malformed response` | The answer broke the HTTP/2 rules, for example a missing status or a body of the wrong length. |
| `http.Client: body too large` | The answer is larger than `MaxBodyBytes`. |
| `context deadline exceeded`, `context canceled` | The request's time ran out or it was cancelled. The server is told to stop; other requests are not affected. |

**The connection fails, and every request on it fails with the same error:**

| Error | Meaning |
| --- | --- |
| `http2: protocol error: ...` | The server sent something the protocol forbids, such as server push. |
| `http2: frame too large`, `http2: frame size error` | A piece had an impossible size. |
| `http2: flow control error` | The server sent more than it was allowed to. |
| `http2: hpack: ...` | The packed headers could not be unpacked. |
| `http2: header block too large` | Packed headers larger than `MaxHeaderBytes`. |
| `http2: too many control frames` | The server flooded the connection with housekeeping messages. |
| `http2: connection closed` | The server hung up. |

After a connection failure, and after `Close`, every later `Do` fails right
away: with the same error after a failure, with `http2: closed connection`
after `Close`.

## Good to know

- One background task serves each connection. It takes turns reading and
  writing, because a connection value has a single owner. A request that starts
  while the task is waiting for the server may wait up to 16 milliseconds
  before it is sent.
- When the connection ends, the client keeps reading and throwing away
  incoming data for up to a tenth of a second first. Without that, some systems
  (Windows in particular) reset the connection and the server never sees the
  client's last message.
- Request headers named `Host`, `Connection`, `Transfer-Encoding`, `TE` and the
  like are refused; HTTP/2 has its own way to carry that information.
- Header names in an answer are lowercase, as HTTP/2 requires. `Headers.Get`
  finds them whatever case you ask with.
- Answers are always read whole. Streaming a large answer piece by piece over
  HTTP/2 is not available yet.
