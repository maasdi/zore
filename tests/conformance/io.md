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

## Bytes

| Scenario | Expected result |
| --- | --- |
| `strings.Bytes("héllo")` | Six bytes; `[1]` is 195 and `[2]` is 169 |
| `strings.FromBytes(data[:])` for valid UTF-8, empty data, or a prefix cut at a character boundary | The text and `nil` |
| `FromBytes` of `{104, 255}` or of a cut character | `""` and an error equal to `error("strings.FromBytes: invalid UTF-8")` |
| `os.WriteBytes` of `0, 255, 10, 0, 128`, then `os.ReadBytes` | The same five bytes and `nil` |
| `os.ReadFile` of that file | `""` and `error("os.ReadFile: invalid UTF-8")` |
| `os.ReadBytes` of a missing file, `os.WriteBytes` into a missing directory | Errors beginning `os.ReadBytes: ` and `os.WriteBytes: ` |
| `FromBytes(data)` with an `Array<byte>` argument | Rejected: arrays do not convert to slices implicitly |

## Time limits and cancellation

Authority: spec §37.3 (`After`, `DialTimeout`, `SetTimeout`) and §37.4.

| Scenario | Expected result |
| --- | --- |
| `time.After(60).receive()` | `true, true` after at least 60 ms; a second receive gives `false, false` |
| `time.After(0)` | Fires at once |
| `select` over `After(5000)` and `After(20)` | The 20 ms case runs; the program does not wait 5 s |
| `select` on a silent channel and `After(40)` | `After` wins; a pending timer is not a deadlock |
| `Listener.SetTimeout(40)` then `Accept()` with no client | `error("net.Accept: timed out")` after at least 40 ms |
| `Conn.SetTimeout(50)` then `Read` or `ReadBytes` on a silent peer | `error("net.Read: timed out")` and `error("net.ReadBytes: timed out")` |
| `SetTimeout(0)` after a timeout, then the peer sends | The next `Read` returns the data; nothing was lost |
| Writing 64 KiB chunks to a peer that never reads, with `SetTimeout(100)` | Eventually `error("net.WriteBytes: timed out")` |
| `SetTimeout` on a zero-value `Conn` | `error("net.SetTimeout: not an open connection or listener")` |
| `DialTimeout` to a closed port or `"not an address"` | An error beginning `net.Dial: ` |
| `Token.Cancel()` twice | `Cancelled()` is `true`; no panic |
| A worker looping on `token.Sleep(10)`, then `Cancel()` | `Sleep` returns `false` and the worker ends |
| `token.Sleep(5000)` on a cancelled token | `false` at once |
| `WithTimeout(30)` with a child and a grandchild | All three are cancelled; an unrelated token is not |
| Cancelling a child | The parent stays uncancelled |

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
| `WriteBytes` of bytes that are not UTF-8, `ReadBytes(3)` on the other side until `EOF` | Every byte arrives unchanged, at most 3 per call |
| `ReadBytes(0)` | An error |
| Peer sends bytes that are not UTF-8, or stops inside a character | `net.Read: invalid UTF-8` |
| Zero-value `Conn` or `Listener` (from a drained closed channel) | Every operation fails; `Port()` is `-1` |
| A task waiting in `Accept` | Other tasks keep running |
| `net.Conn{id: 1}`, `net.listen(...)` | Rejected: not exported |
| Two uses of one `Conn` after a move | Rejected: use of moved value |

Pending: behavior on targets other than Linux and macOS, and a mixed
read-while-write workload on one connection, which the ownership rules do not
allow a program to express yet.
