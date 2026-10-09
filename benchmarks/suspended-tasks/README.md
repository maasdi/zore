# Suspended async task benchmark

This opt-in Linux benchmark records process resident memory and OS thread count
while async Zore tasks wait on one channel. It establishes a reproducible
baseline; it does not enforce a performance threshold or optimize the runtime.

## Prerequisites and command

Use Linux with `/proc`, Python 3.9 or newer, the repository's pinned Rust
toolchain, and clang with LLVM 15 or newer. `ZORE_RUSTC`, `ZORE_CC`, and
`ZORE_CACHE_DIR` have the same meanings as in the repository README.

From the repository root:

```sh
python3 benchmarks/suspended-tasks/run.py \
  --counts 100 1000 5000 \
  --repetitions 3 \
  --output /tmp/zore-suspended-tasks.json
```

The runner builds the release compiler once and builds `workload.ore` once in a
temporary directory. Compilation finishes before any measured process starts.
Each task count and repetition gets a fresh child process. Pass
`--skip-compiler-build` to reuse an existing `target/release/zore`. A build,
protocol, timeout, or child-process failure makes the runner exit nonzero.
Non-Linux hosts receive an unsupported-host error.

## Workload and measurement

Each async worker sends one readiness message, then waits to receive from a
shared unbuffered release channel. The main function retains every task handle.
After the runner finishes measuring, main releases every worker, joins every
handle, and prints a completion count checked by the runner.

The runner starts sampling only after main has received all readiness messages
and printed `READY`. A readiness send immediately before the worker's receive
does not prove that the worker has returned `Pending`. To account for that last
scheduling window, the runner samples `/proc/<pid>/status` until:

- the thread count is unchanged across the effective stability window;
- RSS spread is at most the larger of 64 KiB or 1% of the window median; and
- the window spans at least 250 ms.

The default interval is 50 ms and the requested window is six samples. The
effective window grows automatically if needed to cover 250 ms. This is a
documented stabilization criterion, not proof that the kernel scheduled every
worker at a particular instant.

The JSON includes the commit, dirty-tree flag, Rust/clang/Zore versions,
platform, CPU, logical core count, configuration, every raw waiting-phase
sample, the maximum observed RSS and thread count, and the settled window
median RSS and thread count. “Sampled peak” means the maximum of these periodic
observations; it is not the kernel's exact high-water mark. RSS is the whole
process: async frames, worker stacks, allocator state, handles, queues, code,
and shared runtime data are all included. The benchmark does not report a
hardware-independent per-task size or subtract a process baseline.

`sample-linux.json` is one local one-repetition run at 100 and 1,000 tasks. It
is evidence that the command and output schema work on the recorded host, not a
portable performance expectation.

For frame-storage comparisons, `../frame-storage/workload.ore` creates a 2 KiB fixed-array
temporary before its first channel wait. The array is dead while the task is
suspended. Run the same fixture with compilers built from the base and candidate
commits, for example:

```sh
python3 benchmarks/suspended-tasks/run.py \
  --source benchmarks/frame-storage/workload.ore \
  --compiler /path/to/base/zore --compiler-revision BASE_SHA \
  --counts 1000 5000 --repetitions 3 \
  --output /tmp/zore-frame-base.json
python3 benchmarks/suspended-tasks/run.py \
  --source benchmarks/frame-storage/workload.ore \
  --compiler /path/to/candidate/zore --compiler-revision CANDIDATE_SHA \
  --counts 1000 5000 --repetitions 3 \
  --output /tmp/zore-frame-candidate.json
```

`--compiler` uses the specified executable without rebuilding it. The
`commit` field identifies the benchmark runner's checkout; `compiler_revision`
identifies the external compiler only when supplied by the caller. These
process-RSS readings include runtime overhead and allocator behavior; they are
evidence for a particular host, not a portable test threshold.
