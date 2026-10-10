# Time, operating system, buffered I/O, TCP, and coordination conformance cases

Authority: spec §37.3–§37.4. The cases below are covered by executable tests in
`tests/codegen/native.rs` and `tests/packages/packages.rs` unless marked pending.

## `zore/time`

| Scenario | Expected result |
| --- | --- |
| Hundreds of tasks each `Sleep(250 * time.Millisecond)` | All finish after about 250 ms in total, not the sum |
| `Sleep(0)`, `Sleep(-5)`, and the minimum `int` | Return at once |
| `Now()` read twice | The second value is not smaller |
| `Since(start)` after `Sleep(25 * time.Millisecond)` | At least `25 * time.Millisecond` |
| A task sleeping while another computes | The computing task finishes first |
| `Sleep("soon")`, `Sleep()` | Rejected: argument type and count |
| `time.After(60 * time.Millisecond).receive()` | `true, true` after at least 60 ms; a second receive gives `false, false` |
| `time.After(0)` | Fires at once |
| `select` over `After(5 * time.Second)` and `After(20 * time.Millisecond)` | The 20 ms case runs; the program does not wait 5 s |
| `select` on a silent channel and `After(40 * time.Millisecond)` | `After` wins; a pending timer is not a deadlock |

## Bytes

| Scenario | Expected result |
| --- | --- |
| `strings.Bytes("héllo")` | Six bytes; `[1]` is 195 and `[2]` is 169 |
| `strings.FromBytes(data[:])` for valid UTF-8, empty data, or a prefix cut at a character boundary | The text and `nil` |
| `FromBytes` of `{104, 255}` or of a cut character | `""` and an error equal to `error("strings.FromBytes: invalid UTF-8")` |
| `FromBytes(data)` with an `Array<byte>` argument | Rejected: arrays do not convert to slices implicitly |

## `zore/os`

| Scenario | Expected result |
| --- | --- |
| `WriteFile` then `ReadFile` | The bytes round-trip, including multi-byte characters |
| `WriteFile` of `0, 255, 10, 0, 128`, then `ReadFile` | The same five bytes and `nil`; nothing is checked |
| `WriteFile` over an existing file | The old contents are replaced |
| `ReadFile` of a missing path, `WriteFile` into a missing folder | Errors beginning `os.ReadFile: ` and `os.WriteFile: ` |
| `os.WriteFile(...)` as a statement | Rejected: the error must be used or discarded |
| `let data = os.ReadFile(path)` | Rejected: two results |
| `MkdirAll` of nested folders, files written into them, `ReadDir` | Entries sorted by name, folders marked |
| `Stat` of a file written with `0o640` | Its name, size, `Mode() == 0o640`, and not a folder |
| `Remove` of a file twice, of a non-empty folder | `nil`, then an error; an error |
| `RemoveAll` of a tree, then again | `nil` both times; the tree is gone |
| `Create`, `WriteString`, `Close`, `ReadFile`, in plain and async functions | The same results and cleanup |
| `Getenv` of a set and an unset variable, `Args()` with one argument | Its value and `""`; two elements, the second the argument |
| `Stdout().WriteString` | The text on standard output and its byte count |
| `Exit(4)` | The process ends with status 4; later statements do not run |

## `zore/os/exec`

| Scenario | Expected result |
| --- | --- |
| `Command("echo", ...).Output()` | What the program printed and `nil` |
| `Run` of a program that exits with 3 | `error("exit status 3")` |
| `CombinedOutput` of a program writing to both streams | Both outputs |
| `Dir` set to `/` with `pwd` | The program ran there |
| A program that does not exist | An error |

## `zore/io`

| Scenario | Expected result |
| --- | --- |
| `io.Copy` from a `bytes.Buffer` into a user type with a `mut Write` method | The byte count and `nil`; the writer received every byte |
| `io.Copy` from a buffer into `os.Stdout()` | The bytes are printed |
| `io.ReadAll` of a buffer | Every byte and `nil` |
| `io.ReadFull` of a buffer shorter than the slice, and of an empty one | The count with `unexpected EOF`; `0` with `io.EOF` |
| `io.WriteString` to a file | The byte count |
| A `string` where an `io.Reader` is expected | Reject: `string` does not satisfy `io.Reader` |

## `zore/bufio`

| Scenario | Expected result |
| --- | --- |
| `Scanner` over standard input with lines ending `\n`, `\r\n`, and a last line with none | Each is returned without its terminator; `Err()` is `nil` at the end |
| A line that is not UTF-8 | `Scan` is `false` and `Err()` is `error("bufio.Scanner: invalid UTF-8")`; later `Scan` calls stay `false` |
| A task waiting in `Scan` | Other tasks keep running |
| `Reader.ReadBytes` and `ReadString` on bytes, a line, and a final line, in plain and async functions | The bytes, the text, then the rest with `EOF`, then `EOF` |
| `Writer` with three writes, `Flush` | Nothing pending afterwards; the file holds every byte |
| `Scanner` and `Reader` over a `bytes.Buffer` | Lines, text, single bytes, and `Read` work as over a file |
| A `bufio.Reader` made over another `bufio.Reader`, read with `io.ReadAll` | Every byte |
| An async task scanning lines from a `net.Conn` while a `bufio.Writer` over the other end writes them | Each line arrives; the scanner ends when the writer's connection is dropped |

## `zore/net`

| Scenario | Expected result |
| --- | --- |
| Echo server and three clients, one reading one byte at a time | Each client gets its bytes back unchanged |
| 200 clients at once | Every byte is echoed; one event loop serves them all |
| Dropping a `Conn` | The peer reads `error("EOF")` after any data |
| `CloseWrite` | The peer reads `EOF`; the caller can still read |
| Bytes that are not UTF-8 sent through a connection in both directions | Every byte arrives unchanged |
| `Write` of eight bytes | `8` and `nil` |
| `Read` into an empty view | `0` and `nil` at once |
| `RemoteAddr` of a dialed connection | The listener's `Addr()` |
| Dial to a closed port, `Listen("tcp", "not an address")` | Errors |
| `Listen("udp", ...)`, `Dial("unix", ...)` | `net.Listen: unknown network udp`, `net.Dial: unknown network unix` |
| `Listen("tcp6", "127.0.0.1:0")` | An error: no IPv6 address |
| Zero-value `Conn` or `Listener` (from a drained closed channel) | Every operation fails with a `not an open` error; addresses are `""` |
| `Close` of an open listener; of a zero-value `Conn` | `nil`; `net.Close: not an open connection or listener` |
| Two `Close` calls on one `Conn` | Rejected: use of moved value |
| A task waiting in `Accept` | Other tasks keep running |
| `net.Conn{id: 1}`, `net.listen(...)` | Rejected: not exported |
| Two uses of one `Conn` after a move | Rejected: use of moved value |

## Deadlines

| Scenario | Expected result |
| --- | --- |
| `Listener.SetDeadline(now + 40 ms)` then `Accept()` with no client | `error("net.Accept: timed out")` after at least 40 ms |
| `SetReadDeadline(now + 50 ms)` then `Read` on a silent peer, twice | `error("net.Read: timed out")` both times |
| `SetDeadline(0)` after a timeout, then the peer sends | The next `Read` returns the data; nothing was lost |
| A deadline already in the past | The next wait fails at once |
| Writing 64 KiB chunks to a peer that never reads, with a write deadline | Eventually `error("net.Write: timed out")`, with the bytes sent so far |
| `SetDeadline` on a zero-value `Conn` | `error("net.SetDeadline: not an open connection or listener")` |
| `DialTimeout` to a closed port or `"not an address"` | An error |

## `zore/context` and `zore/sync`

| Scenario | Expected result |
| --- | --- |
| A cancel function called twice | `Err()` is `context canceled`; no panic |
| A worker waiting on `Done()` in a `select`, then the cancel function | The worker ends |
| `WithTimeout(30 ms)` with a child and a grandchild | All three report `context deadline exceeded`; an unrelated context is still active |
| `Deadline()` of a child of a timed context; of `Background()` | The deadline and `true`; `false` |
| Cancelling a child | The parent stays active |
| 1000 children of one timed context | All are cancelled when it expires |
| A task cancelling a context while another waits on `Done()`, 200 times | The waiter wakes exactly once each time |
| 50 tasks each `Done()` on a `WaitGroup` | `Wait` returns after the last; a second `Wait` returns at once |
| `Done()` on a new `WaitGroup` | Panics with `sync: negative WaitGroup counter` |
| `Once.Do` three times | The function runs once |

Pending: behavior on targets other than Linux and macOS, and a mixed
read-while-write workload on one connection, which the ownership rules do not
allow a program to express yet.
