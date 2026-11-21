# HTTP connection pooling

Every `client.Do` opens a new connection, sends one request, and closes the
connection again. When a program talks to the same service many times, opening
all those connections costs time. An `http.Pool` keeps finished connections open
for a while and uses them again for the next request to the same place. The full
contract is in the
[specification](../spec/language-spec.md#3743-pooled-http11-client-connections--locked).

## Using a pool

Make a client as usual and hand it to `http.NewPool`. The pool then has
`Do` and `Open` methods that work like the client's:

```ore
package main

import "std/context"
import "std/net/http"

func fetch(pool http.Pool, target string, done channel<int>) {
    let empty = Array<byte>{}
    let response, err = pool.Do(context.Background(), "GET", target,
                                http.NewHeader(), empty[:])
    if err != nil {
        done.send(0)
        return
    }
    done.send(response.StatusCode)
}

func main() {
    let client, err = http.NewClient(http.DefaultClientConfig())
    if err != nil {
        panic("client")
    }
    let pool, poolErr = http.NewPool(client, http.DefaultPoolConfig())
    if poolErr != nil {
        panic("pool")
    }
    let done = channel<int>(3)
    for var i = 0; i < 3; i += 1 {
        go fetch(pool, "http://localhost:8080/status", done)
    }
    for var i = 0; i < 3; i += 1 {
        let status, _ = done.receive()
        println(status)
    }
    pool.Close()
}
```

A `Pool` value is a handle: copies share one pool, so you can pass it to many
tasks. The pool takes the client over; you cannot use the client by itself
afterwards.

## Limits

`PoolConfig` keeps everything bounded. Zero picks the default:

- `MaxConnections` (64) and `MaxConnectionsPerHost` (8) limit how many
  connections are open at once, in use or waiting to be used again.
- `MaxIdleConnections` (32) and `MaxIdlePerHost` (4) limit how many unused
  connections are kept. When a limit is full, the oldest one is closed.
- `IdleTimeout` (90 seconds) closes a connection that has not been used for
  that long. The pool has no timer of its own: it closes old connections the
  next time you use it, or when `Close` is called.
- `MaxWaiters` (256) limits how many calls may wait for a free connection.
  One more fails at once with `http.Pool: too many waiters`.

When all connections for a place are busy, a call waits for one to come back,
first come first served. The wait counts towards `ClientConfig.Timeout` and the
context you pass, so a cancelled call stops waiting.

`pool.Stats()` tells how many connections are in use, how many are idle and how
many calls are waiting.

## When a connection is used again

A connection only goes back into the pool when it is certain that nothing is
left on it:

- the whole answer was read, and its length was known (a `Content-Length` or
  the chunked format),
- the server answered with HTTP/1.1 and did not say `Connection: close`,
- no extra bytes arrived after the answer, and nothing failed.

Anything else closes the connection: an error, a cancelled call, a stream that
you close or drop before the end, or an answer that ends only when the server
hangs up. For a stream from `pool.Open`, the connection goes back as soon as
you have read to the end. Connections are never shared between pools, so two
clients with different HTTPS settings never mix connections.

## A server may close an idle connection

A server can close a connection while it sits in the pool. Before using a
connection that has been idle for a second or more, the pool takes a very quick
look (at most a millisecond); if the server has hung up, the pool opens a new
connection instead. If the server hangs up without the pool noticing, the
request fails with `http.Pool: reused connection closed`.
The pool never sends the request again by itself, because it cannot know whether
the server already acted on it. If the request is safe to repeat, such as a
`GET`, you can simply call again; the pool will open a fresh connection.

## Shutting down

`pool.Close()` closes every idle connection, wakes every waiting call with
`http.Pool: closed`, and makes later calls fail the same way. Calls that are
already running finish normally, and their connections are closed when they are
done. `pool.CloseIdle()` only closes the idle connections. When the last copy
of the pool is gone, its idle connections are closed too.

## Reader bodies

`Pool.Upload` applies the same leases and waiter bounds to owned reader bodies,
including unknown lengths. A complete reusable response returns the connection;
a source error or partial upload retires it. `Pool.UploadRetry` additionally
requires an explicit fresh-body factory. See
[pooled and replayable uploads](http-upload-retries.md).
