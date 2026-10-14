# Reproducible performance baselines

This opt-in Linux runner extends the existing suspended-task, frame-storage and
cooperative-budget workloads. It changes no compiler or scheduler semantics.
Use Python 3.9+, Linux with /proc, the pinned Rust toolchain and clang/LLVM 15+.
No package dependencies are added.

From the repository root:

    cargo build --release --locked
    python3 benchmarks/performance/run.py \
      --compiler target/release/zore \
      --compiler-revision "$(git rev-parse HEAD)" \
      --repetitions 3 --iterations 10000 --requests 100 \
      --concurrency 1 4 --counts 100 1000 \
      --output /tmp/zore-performance.json

ZORE_CC and ZORE_RUSTC follow the normal build commands. The runner uses private
runtime-artifact caches and does not rebuild the supplied compiler. An omitted
compiler revision is reported as unverified; the binary's SHA-256 is recorded.
Compare compilers on the same idle host with identical arguments and workload
hashes; repeat in alternating order before attributing differences to a change.

Select suites with --suites compile json rest frames legacy. For a smoke run,
use --repetitions 1 --iterations 10 --requests 4 --concurrency 1 2 --counts 2.
Build/child failures, unexpected results, leak diagnostics and timeouts fail
the command. Output is written only after all selected suites succeed.

## Measurements

| Suite | Work and raw evidence |
| --- | --- |
| compile | Fresh check/build processes for hello, JSON, REST, the multi-file REST sample, and suspension. Each check/warm/cold build duration, IR byte count, and clang object-compilation duration/command. |
| json | Encode, strict decode, and late unknown-field rejection for a 60-byte document with an owned string array. Warm process per mode; every batch's elapsed nanoseconds, count, checked checksum and memory/resources. |
| rest | Stateless user lookup/create, unknown-field and malformed-JSON rejection, media-type rejection, 404 and handler errors. Closed-loop clients warm/reuse connections. Every request's latency/body bytes, reconnections, throughput, nearest-rank p50/p95/p99 and memory/resources. |
| frames | Existing channel, dead-array, cooperative-loop and disjoint-array workloads, plus a request retaining a 1 KiB owned payload through an awaited child borrowing its slice. Emitted frame types, clang-folded LLVM ABI sizes, and unchanged settled-memory sampler output. |
| legacy | Existing compute/I/O/mixed budget workloads with the same compiler in both comparison positions, preserving their schema and raw repetitions. Frame-reuse analysis at 100/200/400/800 locals with optional --json output recording every lowering duration and work count. |

Metadata includes runner commit/dirty state, compiler revision/binary hash,
workload package-file and runner-script hashes, host/CPU affinity/cgroup limits,
toolchain versions, configuration and repetitions. Frame-reuse scaling uses the
release library from the runner checkout, not the external supplied compiler.

Warm builds first populate a dedicated runtime-artifact cache. Cold builds use
a new empty artifact cache each time, including rustc's runtime build. “Cold”
does not flush page/filesystem caches. All build times include process startup,
import loading/checking, IR emission and linking. The clang wrapper records
LLVM-to-object work, excluding final linking; its startup/capture adds overhead
to warm wall time. Subtracting check from build does not isolate one stage.

JSON timing includes fixture setup, codec calls, validation and destruction,
excluding process startup and control I/O. Per-operation means are batch
averages, not tail latencies. REST latency spans request send through full
response read; validation follows the timestamp. Throughput includes client
work between requests and is a loopback result, not server-only capacity.
Create echoes validated input without growing a store. The real REST example
is measured for compilation. Handler errors close connections; reconnects are
counted.

## Memory and resources

JSON/REST use six samples at 50 ms intervals spanning at least 250 ms, with RSS
spread within max(64 KiB, 1%) and unchanged thread count. They also require
stable observed file/socket descriptor counts. REST clients close connections
before sampling; the listener remains. Descriptor counts are best-effort
concurrent snapshots. Every raw sample is retained. RSS captures allocator and
thread-pool retention; its growth alone neither proves nor disproves a leak.

Workloads enable ZORE_CHECK_LEAKS=1 and require clean exit after joining/draining.
That check covers tracked text buffers, channels, mutexes and signal
subscriptions, not all heap allocations. The existing suspended-task sampler's
[readiness/stability limitations](../suspended-tasks/README.md) still apply.

Frame ABI sizes exclude owned payload allocations, callable environments, task
blocks, allocator overhead and queues. Parent/child/spawn-wrapper frames are
reported separately. Imported types may be emitted without ever being allocated.
Whole-process RSS is not a per-task/frame size.

## Correctness and historical evidence

    python3 -m unittest discover -s benchmarks/performance -p 'test_*.py'
    cargo test --locked --test native benchmark

CI checks real clang layout folding, warmed connection reuse, failure handling,
configuration, JSON success/error cleanup, REST responses/drain and suspended
borrowed payloads. It imposes no timing/RSS/throughput thresholds.

The [task sample](../suspended-tasks/sample-linux.json),
[frame comparisons](../frame-storage/README.md) and
[budget comparisons](../cooperative-budget/README.md) remain historical evidence
on their recorded hosts.
