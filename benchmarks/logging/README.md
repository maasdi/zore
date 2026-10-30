# Logging throughput

`workload.ore` logs 200,000 records of the form
`level=INFO msg="request N took 42 ms" id=N ms=42` through `std/log` into
several sinks and prints the average time per record. It writes into a
`logging-benchmark-files` folder under the current directory and removes it
at the end.

    zore run benchmarks/logging/workload.ore

## Recorded run — 2026-10-30

Three consecutive local runs on Linux 6.18.44 x86-64 (three CPUs in process
affinity), rustc 1.98.1 and clang 18.1.3, with a debug `zore` binary (the
generated program is always built with `-O2`). The table gives the lowest and
highest of the three. It is evidence for that host only, without variance
analysis.

| Sink | Time per record |
| --- | --- |
| Record filtered by level | 3.1–3.2 µs |
| Discarding writer, no timestamp | 13.2–13.5 µs |
| Discarding writer, with timestamp | 18.6–19.2 µs |
| `os.File` | 72.1–72.3 µs |
| `rolling`, 64 KiB buffer | 14.1–15.2 µs |
| `rolling`, unbuffered | 73.6–76.7 µs |

Each `os.File` write waits for a helper thread, roughly 45–60 µs per call on
this host, so an unbuffered file sink is bound by that hand-off. The buffered
rolling writer makes one file write per 64 KiB, about 1,400 records here, and
costs within about 2 µs of discarding the records: roughly five times faster
than writing each record to a file.

The remaining cost is building the record. A callgrind profile of the
discarding case attributes about half of its instructions to the runtime's
string ownership registry: every string retain and release looks up one
process-wide map behind one lock. That cost is shared by all string-heavy
code and is not specific to logging.
