# Runtime population measurements

This opt-in harness measures actual compiler-generated async frames on Windows,
Linux and macOS. It uses the same workload source and two scheduler workers on
all three hosts. It preserves older Linux suspended-task and REST-retention data.
No RSS, throughput or wall-clock performance threshold runs in correctness CI.
Correctness CI validates the harness frontend/protocol generation and exercises a
subprocess-isolated helper capacity of one: a gated active job, two queued jobs,
an already expired real socket read deadline, nested helper work and idle cleanup.
Only test builds expose that capacity/idle hook; production remains 512 helpers.

```sh
cargo build --locked --release
python -m pip install psutil==7.2.2
python benchmarks/runtime-scaling/run.py \
  --compiler "$PWD/target/release/zore" --compiler-revision "$(git rev-parse HEAD)" \
  --output /tmp/runtime-scaling.json
```

On Windows use `target/release/zore.exe` with the repository's x64 MSVC toolchain
and LLVM clang configuration. The harness uses Python reader threads for pipe
control, including Windows; it does not rely on Unix pipe selectors. The sampler
requires `psutil==7.2.2` on all hosts. The workload/runtime have no new dependency.

The separate `Runtime scaling measurements` workflow runs on manual dispatch or
pull requests labeled `run-scaling-benchmarks`. It builds that checkout's compiler,
records its hash and source revision, and uploads a separate raw JSON artifact
per host even when a measurement fails. Unlabeled pull requests skip the job.
A job timeout, build failure or child failure is evidence of a failed experiment,
never a successful scale point. The workflow has read-only repository permission.

Each mode runs at populations 32, 128, 512 and 640, payloads zero and 4,096 ints,
three fresh-process repetitions, and two create/release cycles per process:

- `tasks`: channel-suspended tasks, with a payload read after suspension.
- `timers`: one long context deadline per task, cancelled through its parent.
  Context parent/deadline watchers are included in the scheduler counts.
- `sockets`: one silent loopback connection per task; the external peer supplies
  exactly one byte after the active checkpoint. Task results check count/content.
- `socket-deadline`: silent peers stay open until each read reports an error with
  zero bytes, exercising read deadline completion and subsequent owned cleanup.

Each cycle records a baseline, active population, completed population and a
checkpoint after six seconds of idle cleanup (the helper idle policy is five
seconds). Baseline measurement has an explicit start handshake. All control lines use
fixed-width tokens to avoid attributing different scanner token sizes to the
population. Counts include the control scanner and its buffers. Active timer and
socket populations wait for runtime registration/admission before measurement;
capacity or deadline failures remain failures. Successful cleanup requires no
live tasks, queued tasks/helpers, blocked scheduler workers, socket handles,
readiness registrations or cancellable timers. Raw samples cover 300ms at 50ms
intervals; no allocator/RSS stability is assumed from that window. Change sample,
idle and timeout durations explicitly for longer observation.

The JSON records full rustc/clang identities, host/CPU/Python/sampler identities,
configuration, executable/workload/IR hashes and every runtime/harness source
hash. The runtime is copied once per invocation so all variants compile the
same snapshotted runtime bytes. Compiler provenance is the supplied revision plus the actual executable
hash; the harness does not claim that a caller-supplied binary matches its label.
The workflow builds the compiler from the recorded checkout. Dirty source status
is explicit. Results preserve each repetition and cycle without aggregation.

## Attribution and limits

`frames` contains LLVM field layouts, constructor names and padded ABI sizes
computed by clang from the captured compiler output. Multiply the relevant
`main.wait` frame size by the active task count to estimate that pinned frame
component. Context watchers have their own frames and counts. Dynamic int payload
storage is separate: requested live bytes are `population * payload * 8`, and
array capacity/bookkeeping may add storage. Zero/payload paired observations
isolate that contribution without treating whole-process RSS as a frame size.

`allocated_bytes` counts exact currently live bytes allocated through the Zore
allocation ABI, including frames, arrays and other runtime-managed storage.
It excludes Rust scheduler/helper/reactor allocations and thread stacks.
Scheduler tasks/queues/blocked workers, helper threads/idle workers/queued jobs,
registered timers, timer threads, socket handle-table entries, channels, mutexes
and text buffers are counted separately. On Linux/macOS `timer_records` counts
heap deadline records, including bounded stale records; on Windows it is zero
and `timer_threads` counts actual live timer workers. OS RSS and threads plus
Unix open descriptor or Windows process handle counts include all components.
Windows handles and Unix descriptors are different units.

Increasing live allocations/resources after each released cycle indicates a
retention candidate. Completed-but-not-yet-idle resources indicate delayed
cleanup. A flat allocation/resource count with retained RSS may reflect allocator
or thread-stack caching; RSS alone cannot establish a leak. The next baseline in
the same process helps distinguish one-time initialization from repeated growth.

The production helper cap remains 512; queued jobs are unbounded. Windows socket
waits occupy helpers and retry nonblocking operations; a population above 512
exercises queuing, rather than establishing 640 concurrent active helper calls.
Windows cancellable deadlines still use individual timer threads, with OS thread
creation as a possible limit. Unix controls use the shared readiness reactor.
The configured populations and deadlines are experimental limits, not supported
service capacity promises or Windows/Unix performance parity claims.

Current cross-host results and any measured optimization/follow-up remain pending
execution of the labeled pull request workflow. Do not infer scaling from native
correctness tests or mark issue #157 complete from this harness alone.
