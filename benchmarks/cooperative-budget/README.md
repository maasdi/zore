# Cooperative budget overhead

Build release compilers from `main` before #54 and the candidate branch, then
run from the repository root:

```sh
python3 benchmarks/cooperative-budget/run.py \
  --baseline /path/to/base/zore --candidate /path/to/candidate/zore \
  --repetitions 5
```

The runner compiles the same two workloads with each compiler before timing
fresh processes. It alternates execution order and verifies output. The compute
fixture runs a two-million-trip async loop with occasional `time.Millis()` calls
to keep the loop observable. The I/O fixture awaits 300 one-millisecond sleeps.

On this Linux x86-64 host, comparing `main` at `3b01b58` with the candidate
branch, median process times over five repetitions were:

| Workload | Without budgets | With budgets |
| --- | ---: | ---: |
| Computation | 3.9 ms | 60.2 ms |
| I/O-heavy | 339.2 ms | 343.7 ms |

The arithmetic loop is deliberately tiny per trip. Budget checks make those
trips observable, so LLVM can no longer eliminate most of the loop as in the
baseline. This is a substantial cost for such highly optimizable loops, not a
portable multiplier for general programs. The I/O-heavy difference is smaller
than the run-to-run spread. Neither measurement is a test threshold.

The internal 128-visit budget limits scheduling latency to roughly that many
poll entries or loop trips before the task rejoins the queue. It does not bound
the duration of one trip, a synchronous helper, or a native call. The value is
small enough for the one-worker progress regression and large enough to avoid
requeueing on every loop trip; it can be tuned independently of source semantics.
