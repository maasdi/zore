# REST API

Run `zore run examples/rest-api/main.ore`, then from another terminal:

```sh
curl -i -H 'Content-Type: application/json' --data '{"name":"Ada"}' http://127.0.0.1:8080/users
curl -i http://127.0.0.1:8080/users
curl -i http://127.0.0.1:8080/users/1
```

Press Enter in the server terminal to drain connections and wait for completion.
OS signal integration is not available yet. The example binds loopback and keeps
its store in memory. Each request gets an owned handler; handlers share a mutex
and release it before reading or writing the network.

POST accepts exactly one `name` field containing a nonempty JSON string of at
most 200 decoded UTF-8 bytes. Unknown and duplicate fields, malformed escapes,
unpaired surrogates, and trailing input are rejected. The token decoder from `std/encoding/json` enforces this endpoint's single-field
request shape; generated codecs serialize response structs and lists.
Content type must be `application/json` without parameters. GET snapshots data
under the store lock and serializes it afterwards. The server supplies bounded
HTTP/1.1 parsing, body limits, HEAD, OPTIONS, 404/405, and graceful shutdown.
