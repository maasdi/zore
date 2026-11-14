# HTTP downloads

`client.Get` and `client.Do` read the whole response into memory, and stop at
1 MiB by default. For bigger files, `client.Open` hands the body over a piece at
a time instead, so a download of any size uses only a small, fixed amount of
memory. The full contract is in the
[specification](../spec/language-spec.md#3741-streamed-http-client-responses--locked).

## Saving a file

`Open` sends the request and returns as soon as the status and headers arrive.
The result is a `ResponseStream`, which works like any other reader, so
`io.Copy` can write it straight into a file:

```ore
package main

import "std/context"
import "std/io"
import "std/net/http"
import "std/os"

func download(target string, path string) error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let empty = Array<byte>{}
    var stream = client.Open(context.Background(), "GET", target,
                             http.NewHeader(), empty[:], http.DefaultStreamConfig())?
    if stream.StatusCode() != 200 {
        stream.Close()?
        return error("download failed")
    }
    let temporary = path + ".part"
    var file = os.Create(temporary)?
    io.Copy(file, stream)?
    file.Close()?
    stream.Close()?
    return os.Rename(temporary, path)
}

func main() {
    let err = download("http://localhost:8080/report.csv", "report.csv")
    if err != nil {
        panic("download failed")
    }
}
```

If the download fails halfway, the bytes already written stay in the file.
Writing to a temporary name and renaming it only after success, as above,
means a half-finished file never takes the real name.

HTTPS works the same way through a client made with `tls/https`.

## Limits and timeouts

`StreamConfig` controls a download:

- `MaxBodyBytes` is the largest body allowed, 64 GiB by default. A body of
  exactly this size is fine; one byte more is an error. The 1 MiB limit of
  `ClientConfig` does not apply here.
- `IdleTimeout`, 30 seconds by default, stops a download that receives nothing
  for that long: `http.Client: body idle timeout`. A slow download that keeps
  moving is never cut off by it.
- `Timeout` limits the whole download. Zero, the default, means no limit
  beyond the context you pass.

`ClientConfig.Timeout` still limits connecting and receiving the headers.
Cancelling the context stops a download with `context canceled`.

## Reading and errors

Each `Read` returns some bytes, or an error, never both. The end of the body is
`io.EOF`. After an error, every later `Read` returns the same error, so a
download can never quietly continue past a problem. A body that ends early is
`http.Client: unexpected EOF in body`, never a clean end.

`Trailers` returns any trailer fields the server sent after a chunked body,
and true once the body has been read to the end.

## Cleaning up

The connection closes as soon as the body ends, a `Read` fails, `Close` is
called, or the stream is dropped, whichever comes first. Unread data is thrown
away, not read to the end. Each stream uses its own connection, as `Do` does.
