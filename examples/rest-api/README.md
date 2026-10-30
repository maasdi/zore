# REST API

The package keeps HTTP concerns separate from its in-memory store:

| File | Responsibility |
| --- | --- |
| `main.ore` | Application startup and route registration |
| `handlers.ore` | HTTP validation, status mapping, and responses |
| `models.ore` | User, creation input, and problem response types |
| `store.ore` | Mutex-protected storage, snapshots, and user lookup |
| `json.ore` | Strict decoding of creation requests |

Run `zore run examples/rest-api/main.ore`, then from another terminal:

```sh
curl -i -H 'Content-Type: application/json' --data '{"name":"Ada"}' http://127.0.0.1:8080/users
curl -i http://127.0.0.1:8080/users
curl -i http://127.0.0.1:8080/users/1
```

Press Ctrl-C (or send SIGTERM on Unix) to drain connections and wait for completion.
The example uses function handlers and `http.ListenAndServe`, which manages signal
registration, graceful shutdown, and cleanup automatically. It binds loopback
and keeps its store in memory. Each request gets a fresh invocation; handlers
share a mutex and release it before reading or writing the network.
Startup or listener failure prints an error and exits with a nonzero status.
A `std/log` logger writes `listening`, `user created`, and `stopped` records
to standard error; copies of it are captured by the route handlers and share one
lock, so records from concurrent requests never mix.

The lookup handler uses `(let id, var err)` to declare an immutable ID and
a reusable error, then `(let user, err)` to declare the user and assign the next
error. It handles malformed IDs and missing users locally; other failures
propagate with `?` or an explicit return. No error result is silently ignored.

`http.ListenAndServe(address, router)` uses default limits and deadlines. Use
`http.ListenAndServeWithConfig(address, router, config)` to customize them;
both manage SIGINT/SIGTERM on Unix and console signals on Windows automatically.
Ctrl+C/Break allow orderly draining; Windows close/logoff/shutdown cleanup is best
effort under OS deadlines. `http.ListenAndServeContext` is the
entry point for applications that manage their own cancellation.
An address such as `":8080"` binds all IPv4 interfaces; use `"127.0.0.1:8080"`
to keep the service local, as this example does.

POST accepts exactly one `name` field containing a nonempty JSON string of at
most 200 decoded UTF-8 bytes. Unknown and duplicate fields, malformed escapes,
unpaired surrogates, and trailing input are rejected. The token decoder from `std/encoding/json` enforces this endpoint's single-field
request shape; generated codecs serialize response structs and lists.
Content type must be `application/json` without parameters. GET snapshots data
under the store lock and serializes it afterwards. The server supplies bounded
HTTP/1.1 parsing, body limits, HEAD, OPTIONS, 404/405, and graceful shutdown.
