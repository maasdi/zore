# HTTP proxies

Some networks only let programs reach the outside through a proxy server. A
client made with `http.NewProxyClient` sends every request through the proxy you
name; nothing is read from environment variables such as `HTTP_PROXY`. The full
contract is in the
[specification](../spec/language-spec.md#3747-http-client-proxies--locked).

## Using a proxy

```ore
package main

import "std/context"
import "std/net/http"

func fetch(target string) error {
    let proxy = http.ProxyConfig{URL: "http://proxy.example:3128/",
                                 Authorization: ""}
    let client = http.NewProxyClient(http.DefaultClientConfig(), proxy)?
    let empty = Array<byte>{}
    let response = client.Do(context.Background(), "GET", target,
                             http.NewHeader(), empty[:])?
    println(response.StatusCode)
    return nil
}

func main() {
    let err = fetch("http://example.test/status")
    if err != nil {
        panic("request failed")
    }
}
```

Every call of the client goes through the proxy: `Do`, `Open`, `Upload`,
`Follow`, `DoRetry`, `DoDecoded`, and a `Pool` made from it.

## HTTPS through a proxy

For an `https` address the client asks the proxy to open a tunnel (`CONNECT`)
and then talks TLS with the real server through it. The server's certificate is
checked exactly as without a proxy, so the proxy cannot read or change the
traffic. That needs a TLS transport, so use `https.NewProxyClient` from the
`tls` package:

```
let client = https.NewProxyClient(roots[:], http.DefaultClientConfig(), proxy)?
```

A client from `http.NewProxyClient` has no TLS and refuses `https` addresses with
`http.Client: HTTPS requires verified TLS transport`. There is never a fallback
to an unchecked connection.

## The proxy's password

Put the value of the `Proxy-Authorization` header, for example
`Basic dXNlcjpwYXNz`, in `ProxyConfig.Authorization`. It is sent only to the
proxy: with plain `http` requests, and on the `CONNECT` request for `https`. It
never goes inside an HTTPS tunnel to the real server. Setting a
`Proxy-Authorization` header yourself on a proxy client is an error.

## Errors

- A proxy address must look like `http://host:port/`; anything else fails with
  `http.Client: invalid proxy URL`. HTTPS proxies are not supported yet.
- A proxy that refuses a tunnel fails with
  `http.Client: proxy refused tunnel with status 407` (or whatever status it
  sent).
- Connecting to the proxy, the tunnel request and the TLS handshake all count
  towards `ClientConfig.Timeout`.
