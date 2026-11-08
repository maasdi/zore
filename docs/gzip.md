# Streaming gzip

`std/compress/gzip` wraps any `io.Reader` or `io.Writer`. It implements gzip
framing, DEFLATE and CRC32 in Zore without an external provider. It accepts
ordinary gzip files, including stored/fixed/dynamic DEFLATE, optional headers
and concatenated members.

Choose independent compressed and uncompressed byte limits. Both are inclusive
and must be between 0 and `gzip.MaxBytes` (1 GiB). The reader accepts at most
`gzip.MaxMembers` (1024) members across the whole stream.

```ore
package main

import "std/compress/gzip"
import "std/errors"
import "std/io"
import "std/os"

func compress(path string) error {
    let file = os.Create(path)?
    var writer = gzip.NewWriter(file, 1048576, 2097152)?
    let _, err = io.WriteString(writer, "hello gzip\n")
    if err != nil { return err }
    return writer.Close()
}

func decompress(path string) (Array<byte>, error) {
    let file = os.Open(path)?
    var reader = gzip.NewReader(file, 2097152, 1048576)?
    return io.ReadAll(reader)
}

func main() {
    let args = os.Args()
    let err = compress(args[1])
    if err != nil { panic(errors.Message(err)) }
    let data, readErr = decompress(args[1])
    if readErr != nil { panic(errors.Message(readErr)) }
    println(data.len())
}
```

`NewReader` and `NewWriter` consume their source or sink, including on invalid
limits. Their wrappers are Move values. Dropping them releases their owned
resources. Constructors perform no I/O. An owned file closes when its wrapper
is dropped, but a concrete resource's `Close` method is not explicitly invoked
through an interface lacking that method.

Call `Writer.Close()` to finalize and release the sink, or `Finish()` to keep
the writer alive after finalization. Successful `Finish()` can be repeated;
`Write` after it returns `gzip: writer finished`. Drop never writes a trailer.
A successful finish writes to the sink; it does not flush another buffering
layer. Use an unbuffered file or a sink whose buffering policy you control.

Read until `io.EOF` to verify the complete stream. `io.ReadAll` and `io.Copy`
do this for you, turning a clean EOF into nil. A successful read of exactly the
expected number of bytes does not validate the checksum, trailer or subsequent
members. `Reader.Close()` does not drain or validate unread input. Empty reads
return `(0, nil)` and do not advance validation.

Malformed headers, CRC32/ISIZE mismatches, invalid DEFLATE and truncation return
errors. Non-member trailing bytes are rejected, including zero padding. Bytes
already delivered are unverified partial output until EOF. For trusted output,
write to a temporary destination, consume to EOF, then publish that destination.
A reader failure is sticky; later nonempty reads return the same error.

Writers can also leave a partial compressed prefix when a sink or limit fails.
Their write count measures accepted input, including the chunk whose output
failed. An input-limit failure rejects the entire offered write before accepting
bytes. Sink errors and finalization failures are sticky and cannot be retried.
The reader consumes at most one compressed byte beyond its compressed ceiling
to detect overflow, and never delivers bytes beyond the output ceiling.

Positive short writes are retried. Zero write progress, invalid I/O counts and
100 consecutive `(0, nil)` source reads fail. Underlying I/O errors are
preserved, including an error reported with the final source byte. This permits
bounded cleanup without silently treating a failing source as a valid stream.

The writer uses deterministic headers and fixed Huffman codes with a bounded
LZ77 search. There are no compression levels or metadata knobs. Compression
ratio is intentionally modest; incompressible input may grow, so allow enough
compressed space for overhead. Processing uses bounded chunks, a 32 KiB history
and bounded Huffman tables rather than retaining the whole stream. CPU work
runs inline; underlying interface I/O keeps its ordinary plain/async behavior.

ZIP/TAR and extraction remain tracked by [#224](https://github.com/maasdi/zore/issues/224).
See the [archive follow-up design](design/archives.md) for the proposed boundaries
and policy questions that must be locked before those APIs are implemented.
