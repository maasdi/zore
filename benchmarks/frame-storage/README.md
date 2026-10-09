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

## Slot reuse analysis scaling

`scaling/main.rs` is a Cargo example that builds an async function with `n`
integer locals, all live across one channel send, and times only
`async_lowering::lower`. Checking, MIR lowering, and drop insertion run before the
clock starts. It prints the MIR size, the deterministic `frame_reuse_work` count,
and the median time over the requested repetitions:

```sh
cargo run --release --locked --example async_frame_scaling -- 5 100 200 400 800
```

On Linux 6.18 x86-64 (Intel Xeon at 2.80 GHz, rustc 1.98.1, release build), the
median `lower` time was:

| Declared locals | MIR locals / blocks | `main` at `faa99bb` | Bitset analysis | Work count |
| ---: | ---: | ---: | ---: | ---: |
| 100 | 203 / 104 | 49.2 ms | 0.3 ms | 44,821 |
| 200 | 403 / 204 | 412.1 ms | 0.7 ms | 174,436 |
| 400 | 803 / 404 | 3,999.0 ms | 2.4 ms | 688,066 |
| 800 | 1,603 / 804 | 79,417.4 ms | 9.1 ms | 2,739,331 |

The `faa99bb` medians use three repetitions, except 800 locals, which was timed
once. The new timings use five repetitions. The previous analysis marked every
pair of live locals at every program point and rescanned group members for each
candidate, roughly `O(n³)`. The bitset analysis pairs each instruction's
mentioned locals with the live set, and keeps per-local group conflicts, so its
work count grows about four times per doubling. Every local in this fixture is
live at once, so the interference graph itself has about `n²` edges and the
analysis cannot be cheaper than that here.

Both analyses produce the same slot assignment: a unit test in
`compiler/src/async_lowering/storage.rs` compares them on every example and
benchmark program plus targeted loop, branch, view, and unreachable-code cases.
`tests/async/lowering.rs` checks the work count's growth from 100 to 800 locals
instead of a wall-clock threshold. These timings are host-specific observations,
not performance thresholds.
