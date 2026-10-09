# Cooperative budget overhead

Build release compilers for the two revisions to compare, then run from the
repository root:

```sh
python3 benchmarks/cooperative-budget/run.py \
  --baseline /path/to/base/zore --candidate /path/to/candidate/zore \
  --repetitions 11
```

The runner compiles each workload with each compiler before timing fresh
processes. It alternates execution order and verifies output. Times include
process start and exit.

- `compute` is a microbenchmark. One async task runs a two-million-trip
  arithmetic loop and prints the exact sum.
- `io` is one async task that awaits 300 one-millisecond sleeps.
- `mixed` is a small workload. Four of those compute tasks run alongside a fifth
  task that awaits 100 sleeps, so the pool has more runnable tasks than this
  host has workers.

None of these numbers is a test threshold.

## Current measurements (#78)

Environment: Linux 6.18 x86-64, 4 vCPUs (Intel Xeon, 2.80 GHz), clang 18.1.3,
rustc 1.98.1. Main is `80c44b0`. The candidate is main plus the scheduler change
described below. Each comparison was one 11-repetition run, and the table gives
medians with the standard deviation in parentheses.

| Compiler | compute | io | mixed |
| --- | ---: | ---: | ---: |
| Before budgets (`3b01b58`) | 5.9 ms (1.2) | 377.0 ms (6.5) | 132.0 ms (3.3) |
| Initial budgets (`865a23a`) | 1028.4 ms (22.5) | 374.4 ms (24.8) | 164.7 ms (25.2) |
| Main (`80c44b0`) | 341.1 ms (41.5) | 376.6 ms (8.5) | 125.4 ms (3.3) |
| Candidate | 9.6 to 10.1 ms (1.0 to 1.7) | 375.3 to 376.2 ms | 123.7 to 126.1 ms |

The candidate appears in all three runs, so its row gives the range of those
three medians. The `io` and `mixed` differences are within the run-to-run
spread. The `compute` change is far outside it.

The earlier figures in this file (3.3 ms before budgets, 15.2 ms with optimized
budgets) were measured on a different host. They do not reproduce here, so the
table above replaces them.

## What was slow on main

Every 128 loop trips the compute task used up its budget, woke itself, and
returned Pending. The worker put the task back on the queue and signalled the
pool's condition variable. That woke a sleeping worker, which found the queue
already empty because the first worker had taken the task back, and went to
sleep again. That is about 15,600 needless thread wake-ups for this loop.

`strace -f -c -e trace=futex` on the compute program:

| Build | futex calls | Time in futex |
| --- | ---: | ---: |
| Main | 60,009 | 9.8 s, summed across threads |
| Candidate | 7 | 0.015 s |

The fix is in the scheduler. When the worker that just polled a task puts it
back on the queue, it signals another worker only if the queue holds other work.
The polling worker always takes the queue's next task itself, so one requeued
task never needs a second worker. Fairness is unchanged. The task still goes
to the back of the queue, and any other queued task still wakes an idle worker.
Budgets, safe points, and the 128-visit limit are unchanged.

## Remaining cost

These are single 15-repetition medians of the compute workload on the same
host, built from IR captured through `ZORE_CC` and edited by hand:

| Variant | compute |
| --- | ---: |
| Candidate | 9.8 ms |
| Candidate with every budget check forced to succeed (never yields) | 8.4 ms |
| The same loop in a plain function | 4.7 ms |

- **About 1.4 ms is the checks and the 15,600 requeues.** Each loop header
  loads, tests, decrements, and stores the 16-bit budget in the poll context.
- **About 2.5 ms is frame residency.** A local that is live at a budget resume
  block must survive a yield, so it is stored in the pinned frame. In this loop
  that is `sum` and `i`. The poll function's `%frame` and `%context` parameters
  carry no aliasing information. LLVM therefore reloads and stores both locals
  around every budget store instead of keeping them in registers. Marking
  `%context` `noalias` by hand saved only about 0.5 ms, so most of the cost is
  the frame storage itself.

A separately scoped next step: keep such locals in poll-local storage, and copy
them into their frame slots only on the exhausted-budget path. On resume, load
them back. This has to exclude locals whose address can be held across the
yield, such as a borrowed local or one that a view points into. It also has to
keep the frame-slot reuse and destroy paths correct for the spilled values.
This is a compiler change, so it needs its own design and tests.

The 128-visit budget limits scheduling latency to roughly that many poll entries
or loop trips before the task rejoins the queue. It does not bound the duration
of one trip, a synchronous helper, or a native call.
