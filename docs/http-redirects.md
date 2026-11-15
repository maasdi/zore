# HTTP redirects

A server can answer "this has moved, look over there" instead of sending what
you asked for. `client.Do` gives you that answer as it is. `client.Follow` goes
to the new place for you, again and again if needed, and returns the final
answer together with the address it came from. The full contract is in the
[specification](../spec/language-spec.md#3744-http-client-redirects--locked).

## Following redirects

```ore
package main

import "std/context"
import "std/net/http"

func fetch(target string) error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let empty = Array<byte>{}
    let response, final = client.Follow(context.Background(), "GET", target,
                                        http.NewHeader(), empty[:],
                                        http.DefaultRedirectConfig())?
    println(response.StatusCode)
    println(final)
    return nil
}

func main() {
    let err = fetch("http://localhost:8080/latest")
    if err != nil {
        panic("download failed")
    }
}
```

`OpenFollow` does the same for a streamed download, and a `Pool` has `Follow`
and `OpenFollow` too.

## What is followed

Only the answers 301, 302, 303, 307 and 308 with one `Location` are followed.
Any other answer, including a redirect with a missing or empty `Location`, is
returned to you unchanged. The new address may be relative, such as `/files/v2`; it is worked
out from the address you were at.

- 307 and 308 repeat the same request, body included.
- 303 turns the request into a `GET` without a body (a `HEAD` stays a `HEAD`).
- 301 and 302 turn a `POST` into a `GET` without a body; other methods stay.

When the body is dropped, your `Content-...` headers are dropped with it.

## Safety rules

- Your `Authorization`, `Cookie` and `Proxy-Authorization` headers are only sent
  to the place you first asked. As soon as a redirect leads to another scheme,
  host or port, they are removed, and they stay removed for the rest of the
  chain.
- A redirect from `https` to plain `http` is refused with
  `http.Client: redirect from HTTPS to HTTP`, unless you set
  `AllowHTTPSDowngrade`.
- Every new HTTPS server is checked with its own certificate.
- `MaxRedirects` (10 by default) limits the chain: one more fails with
  `http.Client: too many redirects`. Coming back to an address already visited
  fails with `http.Client: redirect loop`.
- A `Location` that is not a valid `http` or `https` address fails with
  `http.Client: invalid redirect location`.

## Time limits

The whole chain shares one time budget: `ClientConfig.Timeout` for `Follow`,
and for `OpenFollow` until the final headers arrive. The `Timeout` of a
`StreamConfig` also counts from the first request.
