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
fixture prints the exact sum from a two-million-trip async arithmetic loop.
The I/O fixture awaits 300 one-millisecond sleeps.

On this Linux x86-64 host, comparing the optimized branch against each
reference in separate 11-repetition runs gave these median process times:

| Reference | Compute reference | Compute optimized | I/O reference | I/O optimized |
| --- | ---: | ---: | ---: | ---: |
| Before budgets (`3b01b58`) | 3.3 ms | 15.2 ms | 341.8 ms | 341.6 ms |
| Initial budgets (`865a23a`) | 45.6 ms | 17.7 ms | 337.1 ms | 336.1 ms |

The initial implementation treated every edge to a lower-numbered MIR block
as a loop backedge, adding four budget checks per arithmetic trip. Checking
dominance identifies only the actual loop header and cuts this benchmark's
compute time by about 61%. The arithmetic loop is deliberately tiny per trip;
the remaining cost is substantial for such highly optimizable work, not a
portable multiplier for general programs. The I/O-heavy differences are
smaller than the run-to-run spread. Neither measurement is a test threshold.

The internal 128-visit budget limits scheduling latency to roughly that many
poll entries or loop trips before the task rejoins the queue. It does not bound
the duration of one trip, a synchronous helper, or a native call. The value is
small enough for the one-worker progress regression and large enough to avoid
requeueing on every loop trip; it can be tuned independently of source semantics.
