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
| Record filtered by level | 3.1–3.6 µs |
| Discarding writer, no timestamp | 12.7–14.5 µs |
| Discarding writer, with timestamp | 19.4–20.4 µs |
| `os.File` | 75.8–79.2 µs |
| `rolling`, 64 KiB buffer | 13.8–14.8 µs |
| `rolling`, 64 KiB buffer, `Daily` | 14.1–15.4 µs |
| `rolling`, unbuffered | 75.8–79.3 µs |

Each `os.File` write waits for a helper thread, roughly 45–60 µs per call on
this host, so an unbuffered file sink is bound by that hand-off. The buffered
rolling writer makes one file write per 64 KiB, about 1,400 records here, and
costs within about 2 µs of discarding the records: roughly five times faster
than writing each record to a file. `Daily` reads the wall clock for each
record; its cost is within the run-to-run spread here, at most about 1 µs.

The remaining cost is building the record. A callgrind profile of the
discarding case attributes about half of its instructions to the runtime's
string ownership registry: every string retain and release looks up one
process-wide map behind one lock. That cost is shared by all string-heavy
code and is not specific to logging.

## String ownership without a lock — 2026-10-30

The runtime now keeps each text buffer's owner count in a header in front of
the text and finds that header through a lock-free granule table, instead of
one locked map searched on every retain and release. Same host and toolchain
as above; three runs of each build, alternating, from the same checkout
(lowest–highest). Local evidence for one host only.

| Sink | Before | After |
| --- | --- | --- |
| Record filtered by level | 3.1–3.2 µs | 1.3–1.4 µs |
| Discarding writer, no timestamp | 12.8–14.8 µs | 8.2–8.5 µs |
| Discarding writer, with timestamp | 20.1–20.4 µs | 10.4–10.9 µs |
| `os.File` | 72.6–76.0 µs | 66.1–71.3 µs |
| `rolling`, 64 KiB buffer | 14.1–15.8 µs | 9.4–9.9 µs |
| `rolling`, 64 KiB buffer, `Daily` | 14.0–14.4 µs | 9.2–9.5 µs |
| `rolling`, unbuffered | 73.5–75.8 µs | 68.8–74.0 µs |

Callgrind on 20,000 discarded records (no timestamp) counts 1,352 million
instructions before and 723 million after. Before, the map search, `retain`
and `release` took about 50% of them; after, `retain` and `release` take
about 19%.

Contention was measured with a program that builds 200,000 short strings
(concatenation, `strconv.Itoa` and a slice) per task, once in one task and
then in four tasks at the same time:

| Run | Before | After |
| --- | --- | --- |
| One task | 215–246 ms | 94–118 ms |
| Four tasks at once | 4,583–4,660 ms | 96–144 ms |

Before, four tasks took about twenty times as long as one because every
retain and release waited for the same lock; after, they run side by side.

## String storage by page — 2026-10-30

Issue #237 replaces the one-entry-per-32-byte-granule table with one entry per
4 KiB page, and puts buffers up to 32 KiB (header included) in slots of 38
sizes carved from 256 KiB spans, reused through per-thread free lists. The
table itself drops from 4 bytes per 32 bytes of text (12.5%) to 4 bytes per
4 KiB (about 0.1%); a slot rounds a buffer up by less than a quarter, and a
buffer over 32 KiB by less than one page.

Same host as above (four CPUs in process affinity this time), a release `zore`
binary of each build, three runs each (lowest–highest), from the same checkout.
Peak resident memory is the process's maximum RSS. Local evidence for one host
only.

| Workload | Before | After |
| --- | --- | --- |
| 1,000,000 strings `"key-" + strconv.Itoa(i)` held in an array, peak RSS | 157.2 MiB | 64.8–64.9 MiB |
| the same, wall time | 484–2,203 ms | 233–330 ms |
| 4,000 strings of 20,005 bytes held in an array, peak RSS | 90.2–90.3 MiB | 88.2–88.3 MiB |
| the same, wall time | 94–117 ms | 94–98 ms |
| 200,000 short strings in one task | 101–112 ms | 86–126 ms |
| the same in four tasks at once (six alternating runs) | 174–243 ms | 188–211 ms |

The logging benchmark, four alternating runs each on an otherwise idle host:

| Sink | Before | After |
| --- | --- | --- |
| Record filtered by level | 1.2–2.0 µs | 1.4–1.6 µs |
| Discarding writer, no timestamp | 8.1–8.6 µs | 8.3–9.4 µs |
| Discarding writer, with timestamp | 10.1–11.7 µs | 9.9–11.7 µs |
| `os.File` | 59.7–74.7 µs | 59.2–65.4 µs |
| `rolling`, 64 KiB buffer | 9.1–11.5 µs | 10.3–12.0 µs |
| `rolling`, 64 KiB buffer, `Daily` | 9.3–11.5 µs | 10.0–11.1 µs |
| `rolling`, unbuffered | 58.7–65.2 µs | 60.1–66.8 µs |

The logging and four-task differences are within run-to-run spread. Freed
slots and spans stay with the program for reuse rather than returning to the
system, and table nodes are never freed, as before.

The 20 runtime string tests, including slot lookup from every byte, large
multi-page buffers, and slots freed on one thread and reused on another, pass
under Miri (`MIRIFLAGS=-Zmiri-permissive-provenance cargo +nightly miri test
-p zore-runtime --lib string::`, about 7 minutes) with no undefined behavior
reported; the heaviest loops run fewer rounds under Miri. The flag permits the
exposed-address round trips the table relies on, as before.
