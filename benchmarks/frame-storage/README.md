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
