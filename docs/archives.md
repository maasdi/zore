# Archive I/O

`std/archive` reads and writes canonical USTAR and classic stored ZIP archives.
Both formats support regular files and directories. Archive operations work
through I/O interfaces and never open entry paths or extract files.

ZIP DEFLATE, ZIP64, encryption, links and special nodes, GNU/PAX/sparse TAR
extensions and filesystem extraction are follow-ups to
[#224](https://github.com/maasdi/zore/issues/224). Unsupported forms return errors.

The shared `Header` contains `Name string`, `Size int`, and `Directory bool`.
Readers return metadata by value. Names must be relative UTF-8 paths with `/`
separators and a trailing `/` for directories. `ValidName` rejects traversal,
absolute paths, empty components, Windows-forbidden characters/device names,
Unicode controls and trailing component spaces/dots. It does not resolve paths
against a filesystem. Duplicate entry names remain distinct entries; readers do
not impose an extraction overwrite policy.

## Streaming entries

Construct a `TARReader` from an owned `io.Reader`, or a `ZIPReader` from an owned
`io.ReadSeeker`. ZIP reads its central directory first and treats the complete
source, starting at offset 0, as one archive. Constructors validate limits
without I/O; the first `Next()` starts parsing.

`Next()` returns `(Header, bool, error)`. False with nil means the complete
archive has been validated; false with an error means failure. Call `Read` or
`io.Copy`/`io.ReadAll` to consume only the current entry. Entry EOF allows the
next entry. Call `Skip()` to explicitly discard the remaining entry bytes.
Calling `Next()` with unread entry data fails. Reading exactly the declared size
allows `Next()` to validate remaining padding, descriptors and checksums.

ZIP data is unverified until entry EOF or successful advancement. CRC32 and
sizes must agree between local headers, descriptors and the central directory.
USTAR verifies headers and zero padding but has no payload checksum. Reaching
entry EOF is insufficient to validate the entire archive: continue `Next()`
until presence is false with nil error. `Close()` drops the source and reports
a prior terminal failure; it never drains unread input.

ZIP entries must be contiguous from offset 0, in central-directory order. Extra
unindexed local records, prefixes, gaps, overlaps and split archives fail.
Stored entries may use signed or unsigned data descriptors. Names without the
UTF-8 flag must be ASCII; CP437 is outside this slice. Well-formed opaque extra
fields and comments are bounded and ignored; ZIP64 fields and ambiguous end
records fail. USTAR requires canonical markers, octal fields, two zero end
records and only zero padding afterward. Concatenated TAR archives are rejected.

## Writing and finalization

`NewTARWriter<W io.Writer>` and `NewZIPWriter<W io.Writer>` infer `W` from their
owned sink. `WriteHeader(header)` starts an entry. Write exactly its declared
size, then start the next entry or finish. A directory must have size zero.
Oversized writes fail before accepting bytes; starting another entry or
finishing with unwritten bytes also fails.

`Finish()` writes padding, descriptors and archive end structures. Successful
finish is idempotent. `Close()` calls it and drops the sink. Drop alone never
finalizes. `Sink()` consumes the archive writer, calls `Finish()`, and returns
`(W, error)` even on failure. This retains the concrete type, allowing a caller
to finalize another compression/buffering layer or observe a file's Close error.
A recovered sink can hold an incomplete archive prefix after failure.

This complete example writes `.tar.gz` and reads it to validated archive end:

```ore
package main

import "std/archive"
import "std/compress/gzip"
import "std/errors"
import "std/io"
import "std/os"

func write(path string) error {
    let file = os.Create(path)?
    let compressed = gzip.NewWriter(file, 2097152, 1048576)?
    var writer = archive.NewTARWriter(compressed, 2097152, 1048576, 100)?
    writer.WriteHeader(archive.Header{Name: "hello.txt", Size: 14, Directory: false})?
    let _, err = io.WriteString(writer, "hello archive\n")
    if err != nil { return err }
    let recovered, finishErr = writer.Sink()
    if finishErr != nil { return finishErr }
    return recovered.Close()
}

func read(path string) (int, error) {
    let file = os.Open(path)?
    let compressed = gzip.NewReader(file, 1048576, 2097152)?
    var reader = archive.NewTARReader(compressed, 2097152, 1048576, 100)?
    var total = 0
    for {
        let header, present, err = reader.Next()
        if err != nil { return total, err }
        if !present { return total, nil }
        let data = io.ReadAll(reader)?
        total += data.len()
        println(header.Name)
    }
}

func main() {
    let args = os.Args()
    let err = write(args[1])
    if err != nil { panic(errors.Message(err)) }
    let count, readErr = read(args[1])
    if readErr != nil { panic(errors.Message(readErr)) }
    println(count)
}
```

Writer output is deterministic. USTAR uses uid/gid/mtime 0 and file/directory
modes 0644/0755; ZIP uses stored payloads, UTF-8 names, the DOS epoch, signed data
descriptors and no archive comment. No permissions, ownership, timestamps or
other archive metadata are applied to the filesystem.

## Bounds and errors

Every constructor takes `archiveLimit`, `dataLimit`, and `entryLimit`. These are
inclusive whole-archive byte, summed data-byte and entry ceilings. Byte limits
must be between 0 and `MaxBytes` (1 GiB), and entry limits between 0 and
`MaxEntries` (65,534). Names are at most `MaxNameBytes` (1024 bytes); USTAR must
also fit a 100-byte name and optional 155-byte prefix. ZIP central-directory
bytes are additionally capped at `MaxMetadataBytes` (16 MiB).

ZIP retains bounded directory metadata; TAR retains a bounded working record.
Neither retains all payload data. Generics keep the sink's ordinary ownership
and borrowing rules; Move sink arguments transfer, Copy sink arguments copy.
All wrappers are Move values, including writers over Copy sinks.

Format, progress, limit and I/O failures are sticky. Positive short reads/writes
work normally; invalid counts, zero write progress and 100 consecutive empty
source reads fail. Partial output is preserved with an error and cannot be
rolled back. Sources/sinks remain owned and are dropped on failure or panic.
Invalid constructors return inert wrappers and perform no I/O. CPU work runs
inline; underlying I/O retains ordinary plain/async behavior.

See the [conformance map](../tests/conformance/archives.md) and
[remaining archive design](design/archives.md).
