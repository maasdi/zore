# Performance follow-ups — 2026-10-14

Issues [#127](https://github.com/maasdi/zore/issues/127) and
[#128](https://github.com/maasdi/zore/issues/128) now have measured fixes.
Internal LLVM linkage reduces native compilation work while keeping frontend
checking complete. Cancelable context deadline registrations and direct HTTP
shutdown notification remove the completed-request timer backlog observed in
the [original baseline](performance.md).

## Identity and reproduction

Baseline compiler/runtime: c50f2362d9183c18694282f6c29d2bb12049c75c.
Candidate compiler/runtime and runner: e1f3f4db579fa0dbc5465b70d5cde8f686f6d130.
All five reports began with a clean runner checkout. Both compilers were built
in release mode; their binary hashes are recorded in the raw reports.
Host: Linux 6.18.44 x86-64, AMD EPYC 9V74, three CPUs in process affinity,
two-CPU cgroup quota, 8 GiB memory limit. Toolchain: rustc 1.98.1,
clang 19.1.7, Python 3.12.14.

Build each compiler from its revision in separate checkouts using
`cargo build --release --locked`. Run the candidate checkout's measurement
scripts with the matching binaries:

    python3 benchmarks/performance/run.py --suites compile \
      --baseline-compiler /path/to/baseline/zore \
      --baseline-revision c50f2362d9183c18694282f6c29d2bb12049c75c \
      --compiler /path/to/candidate/zore \
      --compiler-revision e1f3f4db579fa0dbc5465b70d5cde8f686f6d130 \
      --warm-builds-only --repetitions 3 --output /tmp/compile-comparison.json

For each matching compiler/runtime revision, run both `get` and `invalid_json`
in fresh servers:

    python3 benchmarks/performance/retention.py \
      --compiler /path/to/compiler/zore --compiler-revision SAME_SHA \
      --runtime-revision SAME_SHA --case get \
      --rounds 20 --requests 500 --concurrency 1 \
      --idle-seconds 35 --idle-interval 1 --output /tmp/retention-get.json

The measurements ran serially: compiler comparison, baseline GET, candidate GET,
candidate invalid JSON, baseline invalid JSON. They impose no CI thresholds.
See the [measurement guide](../benchmarks/performance/README.md) for sampling,
cache and attribution limits.

## Native compilation

Each workload used three fresh check/build processes per variant with separate
warmed runtime-artifact caches. Repetition order alternated baseline/candidate,
candidate/baseline, baseline/candidate. Entries below are medians; raw reports
retain all repetitions, compiler identities, commands and source hashes.
Clang covers LLVM-to-object compilation, excluding final linking. Warm build
includes checking, emission, object compilation, linking and capture overhead.
Cold runtime-cache builds were deliberately omitted from this focused comparison.

| Workload | Check baseline/candidate, s | Warm build baseline/candidate, s | Clang baseline/candidate, s | Object baseline/candidate, bytes | Input IR baseline/candidate, bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.0013 / 0.0013 | 0.154 / 0.150 | 0.026 / 0.025 | 1,384 / 1,200 | 3,205 / 3,214 |
| JSON fixture | 0.1213 / 0.1230 | 3.056 / 1.529 | 2.770 / 1.255 | 423,672 / 155,736 | 3,781,750 / 3,784,207 |
| REST fixture | 0.3215 / 0.3224 | 34.858 / 20.203 | 34.316 / 19.641 | 1,907,480 / 1,204,248 | 12,323,909 / 12,282,409 |
| Multi-file REST sample | 0.3115 / 0.3168 | 36.143 / 32.446 | 35.604 / 31.915 | 1,892,296 / 1,505,824 | 12,010,546 / 11,969,048 |
| Suspended payload | 0.0168 / 0.0188 | 0.842 / 0.593 | 0.683 / 0.403 | 127,800 / 40,592 | 1,481,313 / 1,482,312 |

Warm REST fixture build time fell about 42%; the multi-file sample fell about
10%. The effect depends on reachable code, and these figures are observations
on this host. The comparison includes both follow-ups; the small REST input-IR
change also reflects the request-context rewrite. A native test independently
checks LLVM elimination of unused internal definitions.

Ordinary generated functions and bundled native wrappers now have internal
linkage. The external `zore_entry` shim and runtime ABI remain available.
Referenced closure, method, callback, interface, generic, initializer and async
helpers remain reachable through their ordinary LLVM references. All bodies
still undergo frontend checking, including unused bodies. There is no custom
reachability pass or change to scheduler/language rules.

## Request retention

Each server handled 20 batches of 500 measured requests, plus one warmup request
per batch: **10,020 total requests, 10,000 measured**. The GET case returns a user;
the invalid-JSON case rejects a late unknown field with HTTP 400. Each batch
closes clients before settled process sampling; the listener remains open.
The final checkpoint follows another 35 seconds idle, longer than the original
30-second request deadline. Every run drained and quiesced before clean,
tracked-resource-checked exit.

| Case / variant | RSS before, KiB | RSS after load, KiB | RSS after idle, KiB | Tasks / live timers after load | Tasks / live timers after idle | Channels after load / idle |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| GET baseline | 2,220 | 12,360 | 12,376 | 10,021 / 10,021 | 1 / 1 | 10,020 / 0 |
| GET candidate | 1,832 | 2,324 | 2,324 | 1 / 1 | 1 / 1 | 1 / 1 |
| Invalid JSON baseline | 2,184 | 12,408 | 12,424 | 10,021 / 10,021 | 1 / 1 | 10,020 / 0 |
| Invalid JSON candidate | 1,852 | 2,640 | 2,640 | 1 / 1 | 1 / 1 | 1 / 1 |

In the baseline, every completed request left a detached deadline timer task,
its channel and reactor registration until expiry. Counts grew with the exact
request total, then returned to the accept wait after the original deadlines.
RSS stayed near its high-water mark even after those objects disappeared,
consistent with allocator/container retention. This establishes a temporary
live-object backlog; it does not establish an unbounded allocation leak or
attribute every retained RSS byte.

The candidate stayed at one task and one live timer at every settled load
checkpoint: the server's accept wait. Its one retained channel is the new
shutdown notification. Mutex/text-buffer counts stayed at two/four. Both
variants retained seven open descriptors and one socket at settled checkpoints;
thread counts ranged from six to seven. Runtime worker snapshots stayed at
three for the candidate and rose from three to four during baseline load,
returning to three after idle. The pool's configured capacity is two; blocking
control input and compensation mean observed workers can exceed that capacity.
No per-worker stack-byte or whole-heap attribution is claimed.

Request throughput is also retained in the raw files. Median batch rates were
5,219/4,324 requests/s for baseline/candidate GET and 2,323/2,642 requests/s for
invalid JSON. Thus the GET run was slower while the error run was faster. Each
case/variant used one fresh process; these component-profiling runs establish
neither a portable throughput gain nor a capacity regression. Client work,
worker compensation and read-only instrumentation affect those rates.

The profiler links captured LLVM against matching runtime sources with private
read-only test counters. Older baseline sources receive only snapshot/quiescence
hooks. Snapshots identify scheduler tasks, workers and queues; reactor live
registrations and timer records; tracked channel, mutex and text resources.
Source hashes record that instrumentation. Process samples and component
snapshots are sequential observations, not one atomic heap census. Resource
counts identify the retained owners and distinguish their eventual release from
RSS retention; they do not give precise task/frame/stack allocation sizes.

## Lifecycle and correctness

Context deadlines now register a private channel receive and an owned timer
with one watcher frame. When cancellation wins, completing the watcher removes
both registrations instead of leaving a detached `time.After` job. Timer-drop
removes its live operation; stale heap records are bounded by the existing
compaction policy. Cancellation still schedules cooperative watcher cleanup;
it does not synchronously join tasks. Ordinary detached task semantics and the
public `time.After` API are unchanged.

HTTP request-context watchers select directly on one server shutdown channel.
Forced shutdown closes it once; normal draining still lets in-flight requests
finish. This removes short poll-alarm tasks without changing accept/read/write
polling, handler errors or graceful drain behavior.

Deterministic tests check canceled long deadlines, expiry, parent propagation,
repeated cancellation, timer and receive-queue cleanup, cancellation races,
reference release and quiescence. Native linkage tests cover ABI visibility,
LLVM dead-body elimination, initialization, callbacks, generic/interface calls,
async captures and destructors, and unused-body diagnostics. Existing native
error/panic, signal, context and HTTP drain/cancellation tests remain required.
Harness tests check the generated control program with the real frontend and
rewrite both old and new hook linkage. CI asserts state and behavior, never RSS,
latency or throughput thresholds.

## Raw evidence

These gzip files preserve all original JSON bytes. Hashes below refer to the
decompressed JSON, including metadata and raw repetitions/samples/requests.

| Report | Decompressed SHA-256 |
| --- | --- |
| [Compiler comparison](../benchmarks/performance/followups-compile-linux-2026-10-14.json.gz) | `4e62de51c0d9cadb51b798655edd4f249c5cde7640b3ea12f86794551bd8813c` |
| [Baseline GET](../benchmarks/performance/followups-base-get-linux-2026-10-14.json.gz) | `11cdadcfa8067b49a72dbac90a363867bb7eb7ba4ce318d73a90576add325788` |
| [Candidate GET](../benchmarks/performance/followups-candidate-get-linux-2026-10-14.json.gz) | `9124d96d3ad65f28d2513bc78929e7c4f5980020bfc5c69c18d1a4a4282d6b05` |
| [Baseline invalid JSON](../benchmarks/performance/followups-base-invalid-linux-2026-10-14.json.gz) | `f6c9363a806da0f9c78c0457ceaa6d1a7ffd22dade5dfd262e57804462d78696` |
| [Candidate invalid JSON](../benchmarks/performance/followups-candidate-invalid-linux-2026-10-14.json.gz) | `f310c70d7520c7f60befdec4bb1e234b880a2602ac1b833c4e022066d7bead07` |
