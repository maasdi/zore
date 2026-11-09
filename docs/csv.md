# CSV files

`std/encoding/csv` reads and writes CSV ("comma-separated values"), the plain
table format that spreadsheets and databases import and export. Each line is
a record, and commas split it into fields. The full contract is in the
[specification](../spec/language-spec.md#3725-csv-records--locked).

## Reading and writing

A `Reader` takes bytes from anything that can be read, such as a file or
standard input, and hands back one record at a time, so a big file never has
to fit in memory. A `Writer` sends records to anything that can be written.
This program copies a table from standard input to standard output and adds a
row with the sum of the second column:

```ore
package main

import "std/encoding/csv"
import "std/io"
import "std/os"
import "std/strconv"

func copyWithTotal() error {
    var reader = csv.NewReader(os.Stdin())
    var writer = csv.NewWriter(os.Stdout())
    var total = 0
    for {
        let record, err = reader.Read()
        if err == io.EOF {
            break
        }
        if err != nil {
            return err
        }
        if reader.Line() > 1 {
            total += strconv.Atoi(record[1])?
        }
        writer.Write(record[:])?
    }
    writer.Write(Array<string>{"total", strconv.Itoa(total)}[:])?
    return writer.Flush()
}

func main() {
    let err = copyWithTotal()
    if err != nil {
        panic("copy failed")
    }
}
```

Given

```text
name,score
Ada,7
"Lovelace, A",5
```

it prints the same three lines and then `total,12`.

## The rules

- A field that holds a comma, a quote or a line break is wrapped in quotes,
  and a quote inside it is written twice: `"say ""hi"""` means `say "hi"`.
- Lines may end with LF or with CR LF, as Windows programs write them. Empty
  lines are skipped, and so is the byte order mark Excel puts at the start of
  a file.
- Spaces are kept exactly. ` a ` is a field of three characters.
- Every record must have as many fields as the first one. Set
  `FieldsPerRecord` to `-1` to allow any count, or to a number to require
  exactly that many.
- Use `NewReaderWith` and `NewWriterWith` for a different separator, such as
  `;` (byte 59) or a tab (byte 9), or for comment lines and CR LF output.

## When something is wrong

Reading stops at the first problem and keeps returning the same error. The
message says where the problem is, for example
`csv: line 4, column 7: quote in unquoted field`. A record longer than
`MaxRecordBytes` (1 MiB by default) or with more than `MaxFields` fields
(4096 by default) is an error too, so a broken or hostile file cannot use up
memory.

If the source fails partway, every record that finished before the failure is
still returned, and then the failure itself; a half-read record never is.

## Records belong to you

Each record from `Read` is a new array. Keeping it, changing it or adding to
it never affects other records or the reader.

A `Writer` collects output and sends it in large pieces. Call `Flush` (or
`WriteAll`, which flushes) when you are done: dropping a writer does not send
what it still holds.
