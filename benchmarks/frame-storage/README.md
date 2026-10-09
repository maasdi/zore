# Poll-local frame storage comparison

`workload.ore` keeps 1,000 or 5,000 async tasks waiting on a channel after
using a 256-element integer array. The array is dead at suspension. The
measurement runner and its stabilization rules are documented in
`../suspended-tasks/README.md`.

On Linux 6.18 x86-64 with clang 19, using the same workload, Rust toolchain,
and three repetitions per count, the settled process RSS was:

| Waiting tasks | `main` at `5d8afad` | Poll-local storage at `7237ec5` |
| ---: | ---: | ---: |
| 1,000 | 6,440 KiB | 2,580 KiB |
| 5,000 | 27,012 KiB | 7,364 KiB |

The numbers are medians of the three settled-RSS readings. Each process had
four threads at measurement time. The 256-element temporary and its MIR copy
remove about 4 KiB from each waiting frame; allocator and process overhead
also contribute to the observed RSS difference. These are host-specific
observations, not performance thresholds or memory-use guarantees.

A separate IR layout regression uses a 512-element array and checks that the
frame contains two such array fields instead of the previous four. On this
64-bit target, that frame is 8,328 bytes rather than 16,536 bytes, an 8,208-byte
reduction. The two retained arrays are still needed after suspension; no slot
reuse is applied.

To reproduce, build release compilers from `main` at `5d8afad` and this branch,
then run `../suspended-tasks/run.py` with `--source` set to this workload,
`--compiler` pointing to each build, `--counts 1000 5000`, and `--repetitions 3`.

## Cooperative loop resume storage

`loop/workload.ore` creates the same 256-element array on each loop trip before
waiting on a channel. Its array is reinitialized after every cooperative loop
budget resume and is dead before the next one. Against `main` at `de823db`,
budget-point liveness removes both array fields from the `wait` frame. The
optimized LLVM IR allocates 240 bytes for that frame instead of 4,368 bytes.

On Linux 6.18 x86-64 with clang 19, three settled-RSS readings per count gave:

| Waiting tasks | `de823db` | Budget-point liveness |
| ---: | ---: | ---: |
| 1,000 | 6,488 KiB | 2,612 KiB |
| 5,000 | 27,088 KiB | 7,716 KiB |

All runs had four threads at measurement time. RSS includes allocator and
runtime overhead, so it is not a per-frame size measurement. To reproduce,
build release compilers from `de823db` and the candidate branch, then run
`../suspended-tasks/run.py` with `--source` set to
`benchmarks/frame-storage/loop/workload.ore`, each compiler supplied in turn
with `--compiler`, `--counts 1000 5000`, and `--repetitions 3`.

## Non-overlapping pinned frame slots

`reuse/workload.ore` keeps two 512-element integer arrays live through separate
waits. Their lifetimes do not overlap, but both require persistent storage.
The `wait` frame has three array fields in `main` at `93caea1` and two after
same-type slot reuse. On this 64-bit host, its LLVM field layout is 12,584
versus 8,464 bytes, a 4,120-byte reduction including other reused scalar
fields and alignment.

On Linux 6.18.44 x86-64 (AMD EPYC 9V74, clang 19, rustc 1.98.1), three
settled-RSS runs with 4,000 waiting tasks and five threads gave:

| Compiler | Settled RSS, KiB | Median, KiB |
| --- | --- | ---: |
| `main` at `93caea1` | 54,084; 54,112; 54,088 | 54,088 |
| Slot reuse branch | 38,064; 38,056; 38,084 | 38,064 |

These runs used debug-built compilers to generate the same native workload.
The Linux sampler reads `/proc` after all tasks report ready and before they
are released; it requires stable RSS and thread counts. RSS includes allocator,
runtime, and process overhead and is not a per-frame size or correctness test.
To reproduce, build each compiler from its revision and run
`../suspended-tasks/run.py` with `--source` set to
`benchmarks/frame-storage/reuse/workload.ore`, `--compiler` set to the binary,
`--counts 4000`, and `--repetitions 3`.
