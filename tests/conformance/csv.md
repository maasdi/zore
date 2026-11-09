# CSV records

The locked contract is specification §37.25. Native cases are in
`tests/codegen/csv.rs`; checking cases are in `tests/packages/packages.rs`.

| Case | Required result |
| --- | --- |
| Plain records, CR LF, a leading byte order mark, empty lines, no final line ending | The records, with the line each started on |
| Quoted delimiters, doubled quotes, LF and CR LF inside quotes | Kept exactly; later line numbers count them |
| `a,`, `,b`, `,`, `""`, `""""` | Empty fields where the text has them; one field `"` |
| A lone CR, spaces around fields | Ordinary data; never trimmed |
| Non-ASCII UTF-8 | Returned unchanged |
| `;` and tab delimiters, a comment byte, `FieldsPerRecord` of `-1`, `0` and `3` | Records split and counted as configured |
| A quote in an unquoted field, text after a closing quote, an unclosed quote | The error with line and column |
| Invalid UTF-8, too many bytes or fields, a wrong field count | The error naming the record's line |
| A second `Read` after an error, `ReadAll` past its cap | The same error again |
| Invalid delimiter, comment, field count or limits | The `NewReaderWith` or `NewWriterWith` error, kept |
| A source giving one byte at a time | The same records |
| A source failing after a complete record, after a partial one, inside quotes | Complete records, then the source error |
| A source failing right after a CR, inside a UTF-8 sequence, or after a quote | The source error, except for a real syntax error already received |
| A source returning its last bytes together with `io.EOF` | Every record, then `io.EOF` |
| A source with a negative or too large count, or no progress | `invalid read count` or `no read progress` |
| `Line()` before any record and after a failed `Read` | `0`; the last returned record's line |
| A comment line with CR LF, at the end without a line ending, after a byte order mark | Skipped |
| A quoted field exactly at `MaxRecordBytes`, and one byte over through `""` | Accepted; the length error |
| `ReadAll` on a reader with invalid options | The options error |
| Writing fields with delimiters, quotes, CR, LF, leading space or tab, empty fields, a lone empty field | Quoted only where required; `""` for the lone empty field |
| An empty record | `csv.Writer.Write: empty record`; the writer keeps working |
| Short, failing and lying sinks | Sticky errors; untaken bytes stay buffered |
| 80 KB of records, and a field starting with a byte order mark | Sent once the buffer passes 64 KiB; quoted |
| A Python-written table | Read and written back byte for byte with `CRLF` |
| A changed or kept record | Never affects other records |
| A reader or writer in a `let`, private fields, a non-byte delimiter, non-string records | Rejected by the checker |
