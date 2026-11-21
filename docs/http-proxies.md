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

The same TLS constructor accepts `ProxyConfig.URL: "https://proxy.example:443/"`.
It verifies the proxy's certificate first, then verifies the target independently
inside the CONNECT tunnel. Use `https.NewDefaultProxyClient(config, proxy)` for
system roots, or `NewProxyClient(roots, config, proxy)` for explicit roots.
Neither constructor reads proxy environment variables.

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

- A proxy address must be a root `http://host:port/` or `https://host:port/`
  URL. Other forms fail with `http.Client: invalid proxy URL`. An HTTPS proxy
  needs a verified transport; plain and legacy tunnel constructors return
  `http.Client: HTTPS proxy requires verified TLS transport` before dialing.
- A proxy that refuses a tunnel fails with
  `http.Client: proxy refused tunnel with status 407` (or whatever status it
  sent).
- Connecting to the proxy, the tunnel request and the TLS handshake all count
  towards `ClientConfig.Timeout`.

## Opting into environment settings

Call `http.ProxyFromEnvironment(targetURL)` explicitly. It returns a proxy
configuration and `true`, or an empty configuration and `false` for a bypass or
unconfigured scheme. Construct a TLS proxy client when enabled, and an ordinary
TLS client otherwise. Call the helper for each target; an already-created proxy
client continues using its explicit configuration.

The helper reads one fresh environment snapshot. `HTTP_PROXY`/`http_proxy` and
`HTTPS_PROXY`/`https_proxy` select the proxy for each target scheme. Uppercase
wins when present, including an empty value. Names are matched exactly, including
the spelling preserved in the Windows environment. A URL's optional percent-
encoded username/password becomes Basic authorization and is removed from the
returned URL. Explicit `ProxyConfig.URL` still rejects user information.

`NO_PROXY`/`no_proxy` accepts comma-separated domains, literal IPs, CIDRs and `*`.
A bare domain matches itself and subdomains; `.example.test` or `*.example.test`
matches only subdomains. Matching ignores case and a terminal DNS dot. Optional
ports restrict matches; use brackets for an IPv6 port, such as `[::1]:443`, or
`[2001:db8::/32]:443` for a prefix. CIDR entries match literal targets without
DNS lookup; IPv4 and mapped IPv6 stay distinct.

Each selected variable is validated, even for the unused target scheme, with a
4096-byte limit and at most 256 nonempty bypass entries. Invalid values report
only the variable's name. In CGI (`REQUEST_METHOD` nonempty), uppercase
`HTTP_PROXY` is refused for an HTTP target unless bypassed by `NO_PROXY`.
Lowercase `http_proxy` and `HTTPS_PROXY` remain usable.
