# Helper pool lifecycle measurements

This opt-in component harness measures the real runtime helper pool directly.
It does not measure compiler-generated frames, source-level socket waits, or
throughput. Windows sockets use shared readiness after #170; files, standard
input, connects and DNS still use helpers. The synthetic workload isolates the
remaining helper lifecycle without attributing old socket behavior to current
programs.

The probe uses ordinary Rust standard input on its main thread. It does not keep
a helper occupied reading its control stream. Two scheduler workers are installed
for consistent runtime snapshots. Snapshot initialization starts the existing
Unix reactor; that service belongs to the process baseline. The Windows timer
and socket services remain unused. There are no Zore payload allocations, so
tracked live ABI bytes are zero; Rust containers and thread stacks remain part
of OS memory observations.

```sh
python -m pip install psutil==7.2.2
python benchmarks/helper-lifecycle/run.py \
  --baseline-revision 153dfc7d24b03be844c174f55b5a272209248b5c \
  --after-revision HEAD --output /tmp/helper-lifecycle.json
```

Both revisions must contain the same instrumentation API. The baseline commit
adds counters and controlled burst hooks while preserving the old relative
five-second wait policy. The after revision changes idle-deadline handling.
Each runtime is extracted from its exact Git commit, hashed and compiled with
`zore_test_hooks`; both executables use the same probe source and Rust toolchain.
No Zore compiler or clang is involved. Windows requires the x64 MSVC Rust linker
environment; Linux and macOS use their native Rust toolchains.

The default natural-burst matrix has populations 32, 512 and 640, two repetitions,
and four cycles per fresh process. Each cycle admits gated jobs, captures the
active state, releases them, and performs another burst after a short gap to
observe reuse. Every job checks a nested `blocking::run` result after release,
including at the 512-thread capacity. Population 640 exercises queued admission.
Active jobs and the queue must settle at the expected counts before capture.

After the reuse burst, snapshots occur at elapsed offsets 1, 6, 15, 30 and 60
seconds. These offsets share one completion origin; they are not cumulative
sleeps. The report records requested and observed offsets. Before/after order
alternates between cases on the same host. Each side starts with a fresh process.

A separate 32-job control uses two cycles and deliberately notifies idle helpers
every half-second for ten seconds. These notifications model allowed empty
wakeups; they do not establish the cause of the historical Windows socket tail.
The control distinguishes relative rearming from a fixed idle deadline without
mixing it into the natural-burst observations.

The default capture uses 14 fresh processes, 52 cycles and 104 gated bursts per
host, and takes roughly 50–60 minutes. The labeled
`run-helper-lifecycle-benchmarks` pull request workflow or manual dispatch runs
Linux, macOS and Windows separately from correctness CI. Failed experiments
retain their raw artifacts and remain failures. Ordinary correctness CI runs
small deterministic lifecycle tests and a two-job probe smoke test, without
retirement-duration or RSS thresholds.

`helper_active_jobs` counts closures popped but not returned, including their
nested inline work. Starting and transitioning workers are not active jobs.
`helper_starts - helper_retirements == helper_threads`; retirement records the
logical decision slightly before the OS thread exits. `helper_empty_wakes`
counts notified/spurious returns with an empty queue, without claiming to
distinguish stolen notifications from spurious wakes. Scheduler workers, queued
tasks, timers, readiness services, handles and tracked ABI bytes remain separate.

Runtime counters precede the OS sample window, so they are not atomic with RSS,
thread or descriptor/handle counts. A retained idle helper is an observation,
not automatically a leak. The harness verifies workload completion and logical
counter invariants, but does not turn checkpoint retirement counts or RSS into
pass/fail performance limits. Raw timing, lifecycle and memory data preserve
retirement tails, recreation, reuse and allocator/stack retention separately.
