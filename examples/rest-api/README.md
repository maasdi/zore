# REST API

Run `zore run examples/rest-api/main.ore`, then from another terminal:

```sh
curl -i -H 'Content-Type: application/json' --data '{"name":"Ada"}' http://127.0.0.1:8080/users
curl -i http://127.0.0.1:8080/users
curl -i http://127.0.0.1:8080/users/1
```

Press Ctrl-C or send SIGTERM to drain connections and wait for completion.
The example uses function handlers and `http.ListenAndServe`, which manages signal
registration, graceful shutdown, and cleanup automatically. It binds loopback
and keeps its store in memory. Each request gets a fresh invocation; handlers
share a mutex and release it before reading or writing the network.

`http.ListenAndServe(address, router)` uses default limits and deadlines. Use
`http.ListenAndServeWithConfig(address, router, config)` to customize them;
both manage SIGINT/SIGTERM automatically. `http.ListenAndServeContext` is the
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
