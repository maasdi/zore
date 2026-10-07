# Time, input, file, and TCP conformance cases

Authority: spec §37.3. The cases below are covered by executable tests in
`tests/codegen/native.rs` and `tests/packages/packages.rs` unless marked pending.

## `zore/time`

| Scenario | Expected result |
| --- | --- |
| Hundreds of tasks each `Sleep(250)` | All finish after about 250 ms in total, not the sum |
| `Sleep(0)` and `Sleep(-5)` | Return at once |
| `Millis()` read twice | The second value is not smaller |
| A task sleeping while another computes | The computing task finishes first |
| `Sleep("soon")`, `Sleep()` | Rejected: argument type and count |

## `zore/io`

| Scenario | Expected result |
| --- | --- |
| Lines ending `\n`, `\r\n`, and a last line with none | Each is returned without its terminator |
| End of input with nothing read | `""` and an error equal to `error("EOF")` |
| A line that is not UTF-8 | `""` and an error equal to `error("io.ReadLine: invalid UTF-8")`; the line is consumed |
| A task waiting for a line | Other tasks keep running |
| `let line = io.ReadLine()` | Rejected: two results |

## `zore/os`

| Scenario | Expected result |
| --- | --- |
| `WriteFile` then `ReadFile` | The text round-trips, including multi-byte characters |
| `WriteFile` over an existing file | The old contents are replaced |
| `ReadFile` of a missing path | An error whose message begins `os.ReadFile: ` |
| `ReadFile` of a file that is not UTF-8 | `""` and `error("os.ReadFile: invalid UTF-8")` |
| `WriteFile` into a missing folder | An error whose message begins `os.WriteFile: ` |
| `os.WriteFile(...)` as a statement | Rejected: the error must be used or discarded |

## `zore/net`

| Scenario | Expected result |
| --- | --- |
| Echo server and three clients, one reading one byte at a time | Each client gets its text back with multi-byte characters intact |
| 200 clients at once | Every byte is echoed; one event loop serves them all |
| Dropping a `Conn` | The peer reads `error("EOF")` after any data |
| `CloseWrite` | The peer reads `EOF`; the caller can still read |
| Dial to a closed port, `Listen("not an address")` | Errors beginning `net.Dial: ` and `net.Listen: ` |
| `Read(0)` | An error |
| Peer sends bytes that are not UTF-8, or stops inside a character | `net.Read: invalid UTF-8` |
| Zero-value `Conn` or `Listener` (from a drained closed channel) | Every operation fails; `Port()` is `-1` |
| A task waiting in `Accept` | Other tasks keep running |
| `net.Conn{id: 1}`, `net.listen(...)` | Rejected: not exported |
| Two uses of one `Conn` after a move | Rejected: use of moved value |

Pending: behavior on targets other than Linux and macOS, and a mixed
read-while-write workload on one connection, which the ownership rules do not
allow a program to express yet.
