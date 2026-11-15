# HTTP content decoding

Servers can squeeze an answer with gzip to make it smaller on the wire. `client.Do`
gives you exactly what was sent, squeezed or not. `client.DoDecoded` asks the
server for gzip and unpacks the answer for you, with limits so that a tiny
download cannot blow up into gigabytes in memory. The full contract is in the
[specification](../spec/language-spec.md#3746-http-client-content-decoding--locked).

## Unpacking an answer

```ore
package main

import "std/context"
import "std/net/http"
import "std/strings"

func fetch(target string) error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let empty = Array<byte>{}
    let response = client.DoDecoded(context.Background(), "GET", target,
                                    http.NewHeader(), empty[:],
                                    http.DefaultDecodeConfig())?
    println(strings.FromBytes(response.Body[:])?)
    return nil
}

func main() {
    let err = fetch("http://localhost:8080/report")
    if err != nil {
        panic("download failed")
    }
}
```

A `Pool` has `DoDecoded` too. To unpack the answer of another call, such as
`Follow` or `DoRetry`, send `Accept-Encoding: gzip` yourself and pass the
answer to `http.Decode`.

## What happens

- The request says `Accept-Encoding: gzip`. The client sets this header itself;
  passing your own `Accept-Encoding` is an error.
- An answer with `Content-Encoding: gzip` is unpacked, and the
  `Content-Encoding` and `Content-Length` headers are removed, because they
  described the packed bytes.
- An answer without `Content-Encoding`, or with `identity`, is returned
  unchanged. So are `HEAD` answers, empty bodies and 206 partial answers
  (a piece of a gzip file cannot be unpacked on its own).
- Any other encoding, such as `br`, or more than one, fails with
  `http.Client: unsupported content encoding`.
- Broken gzip data fails with the gzip error, for example
  `gzip: checksum mismatch` or `gzip: truncated stream`. Several gzip parts one
  after another are joined; anything else after the data is an error.

To see the packed bytes as they came, use `Do` instead.

## Limits

`DecodeConfig` keeps the unpacked answer bounded:

- `MaxBytes` (16 MiB) is the largest unpacked size:
  `http.Client: decoded body too large`.
- `MaxRatio` (200) is how many times larger than the packed bytes the answer
  may grow: `http.Client: decoded body expands too much`.

The packed answer is still limited by `ClientConfig.MaxBodyBytes`, like any
`Do`, and cannot be unpacked above 1 GiB. Cancelling the context also stops the
unpacking.
