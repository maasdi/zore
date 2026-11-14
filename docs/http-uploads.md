# HTTP uploads

`client.Do` needs the whole request body in memory as a byte slice, and stops at
1 MiB by default. `client.Upload` instead reads the body from a file or any
other reader while it sends it, so an upload of any size uses only a small,
fixed amount of memory. The full contract is in the
[specification](../spec/language-spec.md#3742-streamed-http-client-uploads--locked).

## Sending a file

Give `http.NewRequestBody` the reader and the exact number of bytes to send,
then pass it to `Upload`. The answer is an ordinary response, just as from `Do`:

```ore
package main

import "std/context"
import "std/net/http"
import "std/os"

func upload(path string, target string) error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let info = os.Stat(path)?
    let file = os.Open(path)?
    let body = http.NewRequestBody(file, info.Size())?
    var headers = http.NewHeader()
    headers.Set("Content-Type", "text/csv")?
    let response = client.Upload(context.Background(), "PUT", target,
                                 headers, body, http.DefaultUploadConfig())?
    println(response.StatusCode)
    return nil
}

func main() {
    let err = upload("report.csv", "http://localhost:8080/reports/today.csv")
    if err != nil {
        panic("upload failed")
    }
}
```

`Upload` takes the body over. Whatever happens, success or failure, the file is
closed before `Upload` returns, and the body cannot be sent a second time.

## Exactly the size you promised

The server is told the size before any byte is sent, so the client sends exactly
that many bytes:

- If the reader ends early, the upload fails with
  `http.Client: request body shorter than declared length`.
- If the reader has more, the extra bytes are never read or sent.
- A reader that keeps returning nothing gives up after 100 tries:
  `http.Client: request body made no progress`.
- Any other error from the reader stops the upload and is returned as it is.

## Limits and timeouts

`UploadConfig` controls an upload:

- `MaxBodyBytes` is the largest size allowed, 64 GiB by default. A bigger size
  fails before connecting. The 1 MiB limit of `ClientConfig` does not apply.
- `IdleTimeout`, 30 seconds by default, stops an upload when the network takes
  nothing for that long: `http.Client: upload idle timeout`. A slow upload that
  keeps moving is never cut off by it.
- `Timeout` limits the whole call. Zero, the default, means no limit beyond the
  context you pass.

`ClientConfig.Timeout` still limits connecting, and waiting for the answer once
everything is sent.

## When the server says no early

A server may reply, for example with 413 "too large", before it has read
everything. If it still reads the rest, you get its reply as a normal response.
If it stops reading, the upload fails with a network error instead. Either way
the server may already have acted on part of the request; the client never sends
it again by itself.
