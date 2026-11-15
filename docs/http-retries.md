# HTTP retries

Sometimes a server is busy for a moment and answers "try again later", or the
connection breaks halfway. `client.Do` reports that to you at once.
`client.DoRetry` waits a little and tries again, a few times, before giving up.
The full contract is in the
[specification](../spec/language-spec.md#3745-http-client-retries--locked).

## Trying again

```ore
package main

import "std/context"
import "std/net/http"

func check(target string) error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let empty = Array<byte>{}
    let response = client.DoRetry(context.Background(), "GET", target,
                                  http.NewHeader(), empty[:],
                                  http.DefaultRetryConfig())?
    println(response.StatusCode)
    return nil
}

func main() {
    let err = check("http://localhost:8080/health")
    if err != nil {
        panic("still failing")
    }
}
```

A `Pool` has `DoRetry` too, and then every new try may reuse a connection.

## What is tried again

A request is sent again only when the last try:

- failed on the network, for example a broken or timed-out connection, or
- got one of the answers 429 (too many requests), 502, 503 or 504.

Every other answer, such as 404 or 500, is returned to you right away. So is a
mistake in the request itself, such as a bad address: it is found before
anything is sent, and trying again would not help.

## Only requests that are safe to repeat

A request that broke halfway may still have reached the server. Doing it twice
is harmless for `GET`, `HEAD`, `OPTIONS`, `PUT` and `DELETE`, so only those are
tried again. A `POST` or `PATCH` is sent once, unless you set `RetryUnsafe`
because you know the server can cope with a repeat.

## How long it waits

`RetryConfig` controls the tries:

- `MaxAttempts` (3) is the total number of tries, the first one included.
- `Backoff` (200 ms) is the first wait; it doubles after every try. No wait is
  longer than `MaxBackoff` (5 seconds).
- When a retried answer says `Retry-After: <seconds>`, the client waits at
  least that long. If the server asks for more than `MaxBackoff`, the answer is
  returned to you instead of waiting.
- `Timeout`, when set, limits the whole call, waits included. The client never
  starts a wait that would end after it; you get the last answer instead.

Each try is still limited by `ClientConfig.Timeout`, and the context you pass
stops everything, waits included. When all tries are used up, you get the last
answer or the last error.
