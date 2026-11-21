# Pooled and replayable uploads

`Pool.Upload` accepts the same owned `RequestBody` as `Client.Upload`, including
`NewUnknownLengthBody`. It reuses the connection after a completed upload and a
fully read, reusable response. A failed or canceled upload closes its connection.
The pool's connection and waiter limits apply while the source is being sent.

Use `UploadRetry` when the caller can create a fresh body for every attempt.
It is available on `Client`, `Pool` and a fixed `HTTP2Conn`. The caller supplies
an owned `RequestBodyFactory`; each `NewBody(ctx)` returns a new owned body:

```ore
package main

import "std/context"
import "std/errors"
import "std/net/http"
import "std/os"

type FileBodies struct {
    path string
}

func (f mut FileBodies) NewBody(ctx context.Context) (http.RequestBody, error) {
    ctx.Err()?
    let info = os.Stat(f.path)?
    let file = os.Open(f.path)?
    return http.NewRequestBody(file, info.Size())
}

func run() error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let pool = http.NewPool(client, http.DefaultPoolConfig())?
    let response = pool.UploadRetry(context.Background(), "PUT",
        "http://localhost:8080/object", http.NewHeader(),
        FileBodies{path: "payload.bin"}, http.DefaultUploadConfig(),
        http.DefaultRetryConfig())?
    pool.Close()
    println(response.StatusCode)
    return nil
}

func main() {
    let err = run()
    if err != nil {
        panic(errors.Message(err))
    }
}
```

Each file closes before waiting or reopening it, and the factory is dropped
when the call finishes. Keep the file stable if every attempt must send
identical bytes. Factories can also create fresh generator or pipe readers;
`NewUnknownLengthBody` discovers their lengths through EOF. The client keeps
no copy of an earlier body.

The [buffered retry policy](http-retries.md) also applies here: safe methods,
`RetryUnsafe`, retried statuses, attempt limits, backoff and `Retry-After`.
An unsafe method can also retry when an error proves no request byte was written,
or HTTP/2 explicitly refused the stream without processing it. A partial header
write loses that proof. Ordinary `Upload` sends once.

Reader errors, invalid read counts, short sources, no-progress readers, body
limits and factory errors stop retries. This also holds when a transport error
masks an error returned with source bytes. The body and connection are released
before a retry wait. A fixed `HTTP2Conn` can retry refused streams while it stays
usable; after GOAWAY it returns the unprocessed error to its caller.

`RetryConfig.Timeout` covers the entire operation. `UploadConfig.Timeout` covers
each attempt, beginning before `NewBody`; the factory receives that context.
Source reads and factories must cooperate with cancellation themselves. Upload
idle time excludes time in them. Setup and the buffered response retain the
client's phase limits. A per-attempt transport timeout follows the retry policy;
caller cancellation and the total retry deadline stop further attempts.

The full contract is in [specification §37.54](../spec/language-spec.md#3754-pooled-and-explicitly-replayable-http-uploads--locked).
