# Reproducible performance baselines

See the [recorded Linux baseline and follow-ups](../../docs/performance.md).

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

Compare compiler revisions in alternating order with separate warm caches:

    python3 benchmarks/performance/run.py --suites compile \
      --baseline-compiler /path/to/baseline/zore --baseline-revision BASE_SHA \
      --compiler /path/to/candidate/zore --compiler-revision CANDIDATE_SHA \
      --warm-builds-only --repetitions 3 --output /tmp/compile-comparison.json

Each row identifies its compiler variant/revision and includes the object size.
The default still measures cold runtime-artifact-cache builds; --warm-builds-only
omits those builds for a focused comparison of native compilation.

Profile repeated fixed-case REST requests, then observe the original deadlines:

    python3 benchmarks/performance/retention.py \
      --compiler /path/to/compiler --compiler-revision COMPILER_SHA \
      --runtime-revision SAME_SHA --case get --rounds 20 --requests 500 \
      --idle-seconds 35 --output /tmp/rest-retention.json

Repeat with each matching compiler/runtime revision. The profiler reads runtime
sources from Git, adds read-only test counters to older revisions when needed,
and links the captured LLVM against that instrumented runtime with a two-worker
pool. No production counters or public APIs are added. Source hashes identify
the instrumented runtime and shared control program. Checkpoints record tasks,
workers, queues, reactor registrations/timer records, channels, mutexes, text
buffers, and raw process RSS/thread/descriptor samples. These attribute retained
resources; they are not a complete heap-allocation census or server-only capacity.
The source control program drains with Background and waits for scheduler
quiescence before exiting, so its resource checks are not skipped by sleeping
deadline jobs. Idle duration is a measurement choice, not a CI assertion.

Every round warms one request per client before its measured batch. Checkpoints
count total requests including warmups and identify measured requests separately.
The actual response handlers match the normal fixture. Control/snapshot work
is outside each request batch.

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
subscriptions, not all heap allocations, and skips those checks while tasks
remain. HTTP request/deadline contexts can leave detached time.After jobs alive
until their timers expire even after connections drain; a clean REST exit alone
does not prove that those resource checks executed. This retention is tracked
separately in issue #128. The existing suspended-task sampler's
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
