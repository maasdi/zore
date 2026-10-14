# Performance baseline — 2026-10-14

The later [#127/#128 report](performance-followups.md) records internal LLVM
linkage, cancellable request deadlines and their measured effects. The original
baseline and raw data below are preserved at their recorded revisions.

Issue [#118](https://github.com/maasdi/zore/issues/118) now has an opt-in
[runner and measurement guide](../benchmarks/performance/README.md).
The [raw JSON, compressed with gzip](../benchmarks/performance/baseline-linux-2026-10-14.json.gz)
contains every compiler repetition, JSON batch, REST request and memory sample,
frame layout, cooperative workload repetition and frame-analysis duration.

    gzip -dc benchmarks/performance/baseline-linux-2026-10-14.json.gz \
      > /tmp/zore-baseline.json

Decompressed SHA-256:
2f4e03fec7236d74ca7fbb7b923c768e6569b289528e96b07cb843e01ded97ad.

## Identity and reproduction

Compiler: 8d8d036ae180dd0af53c93b568dd1a4945b5e056.
Runner: 2fcb521189f57b89be3f58deddb824c053b69063, clean at measurement start.
Host: Linux 6.18.44 x86-64, AMD EPYC 9V74, three CPUs in process affinity,
two-CPU cgroup quota, 8 GiB memory limit.
Toolchain: rustc 1.98.1, clang 19.1.7, Python 3.12.14.
The raw report includes binary, runner-script and workload-file hashes.

From the runner checkout, using a release compiler built from that revision:

    python3 benchmarks/performance/run.py \
      --compiler /path/to/compiler-built-from-8d8d036 \
      --compiler-revision 8d8d036ae180dd0af53c93b568dd1a4945b5e056 \
      --repetitions 3 --iterations 10000 --requests 100 \
      --concurrency 1 4 --counts 100 1000 \
      --output /tmp/zore-performance.json

The default suites generated 15 compile observations, nine JSON batches,
4,200 measured REST requests, 30 waiting-task observations, 18 cooperative
process timings and 12 frame-analysis timings. Later harness hardening bounds
partial protocol lines without changing the fixtures or measured code paths.
These are one host's observations, not portable expectations or CI thresholds.

## Compiler

Entries are medians of three fresh command processes. Cold builds clear only
the runtime-artifact cache, not the OS page cache. Clang time covers LLVM-to-object
work; full builds include linking and the instrumenting wrapper's overhead.

| Workload | Check, s | Warm build, s | Cold runtime-cache build, s | Clang, s | IR bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.0014 | 0.164 | 2.632 | 0.026 | 3,205 |
| JSON fixture | 0.122 | 3.032 | 5.426 | 2.709 | 3,781,750 |
| REST fixture | 0.313 | 34.401 | 37.263 | 33.887 | 12,323,909 |
| Multi-file REST sample | 0.308 | 34.002 | 37.123 | 33.487 | 12,010,546 |
| Suspended payload | 0.0173 | 0.834 | 3.294 | 0.666 | 1,481,313 |

Clang accounts for about 98% of warm REST build time. The emitter visits every
compiled body, including imported functions; ordinary definitions have external
LLVM linkage. The selected focused optimization is
[#127: evaluate internal function linkage](https://github.com/maasdi/zore/issues/127)
so LLVM can discard unused bodies before expensive function optimization.
Preserve the external entry/runtime ABI, all referenced callbacks/vtables,
async/cleanup helpers and full frontend diagnostics. Measure that change before
adding a custom reachability pass or claiming a speedup.
This baseline implements no compiler/runtime optimization.

## JSON

Each mode ran three 10,000-operation batches on one warmed process. Time includes
codec work, validation and destruction. Latency is the median of batch means,
not a tail distribution.

| Mode | Operations/s, median | Mean latency, microseconds | Settled RSS before/after all batches, KiB |
| --- | ---: | ---: | ---: |
| Encode | 286,273 | 3.49 | 1,712 / 1,712 |
| Strict decode | 57,903 | 17.27 | 1,788 / 1,788 |
| Reject late unknown field | 48,031 | 20.82 | 1,820 / 1,820 |

Every after-batch RSS equaled its mode's initial settled reading.
All processes exited cleanly with tracked runtime-resource checks enabled.

## REST

Each case ran three 100-request batches. Values are medians of batch throughput
and batch p95 respectively, not pooled quantiles. Clients validate responses
and reuse warmed HTTP/1.1 connections when allowed. Handler errors reconnect
for every request; other measured cases reuse their warmed sockets.

| Case | One client, requests/s | One client p95, ms | Four clients, requests/s | Four clients p95, ms |
| --- | ---: | ---: | ---: | ---: |
| GET user | 3,727 | 0.320 | 2,766 | 3.270 |
| POST valid input | 2,167 | 0.499 | 1,586 | 6.092 |
| Reject unknown field | 2,373 | 0.453 | 1,662 | 4.971 |
| Reject malformed JSON | 2,471 | 0.457 | 1,778 | 4.917 |
| Reject media type | 3,238 | 0.355 | 2,659 | 3.461 |
| Missing user | 3,406 | 0.307 | 2,216 | 4.116 |
| Returned handler error | 2,084 | 0.556 | 2,641 | 3.326 |

These are loopback client/server observations. Python client work, the GIL,
worker compensation and the two-CPU quota affect concurrent results; this
establishes neither server-only capacity nor a scalability regression.

REST settled RSS increased across the 21 batches per server: 2,756 to 4,984 KiB
with one client, and 2,852 to 5,396 KiB with four. Every after-batch phase had
seven descriptors and one socket, the listener. Thread counts varied from six
to ten. Servers drained/exited without tracked-resource leak diagnostics.
Those checks do not cover all heap allocations, and the 250 ms stability window
does not distinguish allocator/worker retention from continued growth.
The runtime skips tracked-resource checks while tasks remain. Code inspection
shows that request contexts cancel their context watcher, while the detached
time.After job can remain asleep until the 30-second write deadline. Short
poll-alarm jobs can also outlive the selected branch. This is a likely source
of temporary retention, not proof of an unbounded leak. The clean REST exit
does not establish that the resource checks actually ran.
[#128](https://github.com/maasdi/zore/issues/128) tracks longer fixed-case runs
and timer/task/worker/allocation profiling before choosing a fix.

## Suspended frames

Bytes are LLVM ABI sizes, excluding separate payloads, task blocks and runtime
overhead. RSS is the median of three fresh waiting processes per task count.

| Workload | Worker / child / spawn-wrapper bytes | RSS at 100 tasks, KiB | RSS at 1,000 tasks, KiB |
| --- | --- | ---: | ---: |
| Channel wait | 208 / — / 160 | 1,800 | 2,920 |
| Dead 2 KiB array before wait | 208 / — / 160 | 1,796 | 2,904 |
| Cooperative loop then wait | 232 / — / 160 | 1,796 | 2,912 |
| Disjoint retained fixed arrays | 8,464 / — / 160 | 2,640 | 10,920 |
| Owned 1 KiB payload with borrowed child wait | 272 / 208 / 176 | 1,928 | 4,160 |

Processes had four threads, except the disjoint-array workload's five.
The payload remains owned by the parent while its child borrows the slice.
Each workload releases/joins all tasks before clean resource-checked exit.
The report also retains imported frame layouts; emitted types do not imply
allocation. These readings establish a baseline for later frame/liveness work.

## Existing workloads

Historical [frame](../benchmarks/frame-storage/README.md),
[budget](../benchmarks/cooperative-budget/README.md), and
[task sample](../benchmarks/suspended-tasks/sample-linux.json) results are
preserved. New budget runs use the same compiler in both alternating comparison
positions; these are two sample sets, not an optimization comparison.

| Budget workload | First position median, ms | Second position median, ms |
| --- | ---: | ---: |
| Compute | 6.446 | 7.357 |
| I/O | 338.110 | 332.233 |
| Mixed | 114.191 | 116.001 |

Frame analysis uses the release library from the runner checkout.
All three lowering durations per row are in the raw report.

| Declared locals | MIR locals / blocks | Work count | Median lower time, ms |
| ---: | --- | ---: | ---: |
| 100 | 203 / 104 | 44,821 | 0.188 |
| 200 | 403 / 204 | 174,436 | 0.541 |
| 400 | 803 / 404 | 688,066 | 1.771 |
| 800 | 1,603 / 804 | 2,739,331 | 6.150 |

Timing/RSS values never become correctness assertions. Forced preemption remains
separate runtime/design work.
