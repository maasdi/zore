# Native build scheduling — 2026-10-25

Issue [#195](https://github.com/maasdi/zore/issues/195) remeasures current main
and overlaps runtime preparation with native object generation. The
[2026-10-23 profiling/pruning report](performance-builds.md) is preserved at its
original revisions. Compare variants within this new capture; changes since
that historical report are not attributed to this scheduling change.

## Identity and method

Baseline compiler/runtime: `46e66e076c5a0e57d984d2304ddcd48d96c8175b`.
Candidate compiler/runtime and paired runner: `e9ee2cad12f3505c7d0abeaf2040d31dd91ccc20`.
Both compilers and matching `compile_profile` probes were built in release mode.
The initial baseline and paired runs both began with clean runner checkouts;
the later report commit changes documentation/data only. Raw metadata records
compiler/probe SHA-256 hashes, toolchain versions, source hashes, cache contents,
commands and timings.

Host: Linux-6.18.55-x86_64-with-glibc2.41, AMD EPYC 9V74 80-Core Processor;
three CPUs in affinity, two-CPU cgroup quota, 8 GiB memory limit.
Toolchain: rustc 1.98.1 (48a229cea 2026-09-01), Debian clang version 19.1.7 (3+b1),
Python 3.12.14. No Windows/macOS performance run is claimed.
Separate [Windows measurements](performance-build-windows.md) for
[#205](https://github.com/maasdi/zore/issues/205) use a later current-main snapshot
and an isolated serial control; they do not extend this capture's host claims.
The Linux implementation and measurement scope in #195 is complete. Existing
correctness CI still covers Linux, macOS and Windows.

The initial baseline has one fresh check, warm build and cold runtime-cache
build per workload. The final alternating comparison contains 36 rows: six
workloads, three repetitions, two variants. Each row measures a separate check,
warm build, staged probe and fresh-runtime-cache build. Variant order is
baseline/candidate, candidate/baseline, baseline/candidate. Each variant has a
private warmed cache; each cold build gets a fresh cache. Every warm snapshot
verifies unchanged rlib hashes/sizes/mtimes and absence of runtime compilation;
every cold snapshot verifies compilation and retained cache publication.
Warm means a valid runtime-library cache; clang still recompiles the program.

The shared host lock serializes each warmup and measured variant, including its
check, probe and cold build. Native tools have the existing 300-second command
bound after acquiring a lease. Physical host scheduling is not controlled.
The probe runs the actual `driver::build::native_ir` stages in a separate
process; its phase durations are observations outside the timed build.

## Initial clean-main profile

All durations below are seconds; the initial profile has one observation per
workload. The raw report also retains clang/linker command lines and sizes.

| Workload | Check | Warm build | Cold build | Warm clang | Cold runtime | Warm rustc entry/link |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.002 | 0.241 | 3.068 | 0.028 | 2.789 | 0.116 |
| JSON | 0.124 | 1.261 | 4.289 | 0.899 | 2.943 | 0.114 |
| REST fixture | 0.420 | 7.703 | 10.654 | 6.960 | 2.864 | 0.109 |
| Multi-file REST sample | 0.422 | 10.387 | 13.060 | 9.654 | 2.936 | 0.102 |
| Local dependency | 0.104 | 0.496 | 3.381 | 0.184 | 2.820 | 0.108 |
| Suspended payload | 0.017 | 0.562 | 3.340 | 0.329 | 2.764 | 0.112 |

The entry/link duration includes the nested native linker; do not add their
elapsed durations. All underlying native-link timings are in the raw report.
The staged phase attribution is:

| Workload | Load | Check/ownership | MIR/drop/async | Emit | Prune | Emitted/native IR bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.0002 | 0.0001 | 0.0000 | 0.0000 | 0.0000 | 3,566 / 3,566 |
| JSON | 0.0042 | 0.1366 | 0.0045 | 0.0233 | 0.0041 | 3,133,063 / 1,196,780 |
| REST fixture | 0.0077 | 0.4255 | 0.0150 | 0.0744 | 0.0157 | 12,211,563 / 6,161,573 |
| Multi-file REST sample | 0.0082 | 0.4127 | 0.0123 | 0.0756 | 0.0197 | 11,894,506 / 6,624,584 |
| Local dependency | 0.0031 | 0.0855 | 0.0030 | 0.0105 | 0.0018 | 1,734,594 / 286,245 |
| Suspended payload | 0.0041 | 0.0165 | 0.0015 | 0.0076 | 0.0013 | 1,307,709 / 315,279 |

Clang dominates the larger warm builds. Cold builds additionally spend about
three seconds compiling the independent runtime library in sequence.

## Focused change

After starting clang, the driver prepares the runtime library on one scoped
worker while collecting clang output on the main thread. Both stages finish
before linking or reporting a compiler error. A clang launch failure starts no
runtime worker; when both compilers fail, clang diagnostics retain precedence.
The temporary build directory remains alive until both stages finish.

Runtime source/toolchain keys, atomic cache publication, private fallback builds
and native adapters keep their existing behavior. Complete semantic and ownership
checking, MIR/drop/async lowering and emission still precede native tools.
Clang keeps `-O2` and host object flags; runtime/link compilation keeps
`-Copt-level=2 -Cpanic=abort`. No code-generation or runtime source change is made.

## Alternating warm/cold comparison

Entries are medians of three observations. Reduction is the ratio of cold-build
medians, not the median of paired percentages.

| Workload | Warm baseline/candidate | Cold baseline/candidate | Cold reduction |
| --- | ---: | ---: | ---: |
| Hello | 0.227 / 0.189 | 3.095 / 2.938 | 5.1% |
| JSON | 1.267 / 1.179 | 4.228 / 3.370 | 20.3% |
| REST fixture | 7.509 / 7.442 | 10.437 / 9.167 | 12.2% |
| Multi-file REST sample | 10.012 / 9.877 | 13.096 / 12.026 | 8.2% |
| Local dependency | 0.504 / 0.553 | 3.788 / 3.394 | 10.4% |
| Suspended payload | 0.549 / 0.544 | 3.565 / 3.078 | 13.6% |

Every cold REST pair improves: 11.5–15.4% for the fixture and 5.6–8.2% for the
multi-file sample. Their cold medians fall about 1.27 and 1.07 seconds
respectively. The candidate's median runtime/object command overlap is about
3.35 seconds for REST and 3.26 for the full sample; baseline overlap is zero.
Overlap is not an equal end-to-end saving: concurrent compilers compete for CPU
and memory, and individual compiler commands can take longer on this two-CPU
quota. Their elapsed durations must not be summed as sequential stages.

Warm results do not establish a universal improvement. The dependency control's
warm median regresses about 9.8% (49 ms), and one cold dependency pair is 7.7%
slower. The full sample has a 12.077-second candidate warm outlier, compared with
9.813–10.128 seconds for baseline warm builds. Every observation is retained.
Three repetitions support these bounded observations, not statistical confidence
or portable latency/capacity guarantees. CI has no timing/RSS thresholds.

All repetitions/variants have byte-identical LLVM input and equal object sizes
for each workload. The [IR identity manifest](../benchmarks/performance/build-scheduling-ir-identities-linux-2026-10-25.json)
records each hash/size. This scheduling change targets build critical paths;
it does not weaken native optimization or claim a runtime throughput change.

## Reproduction and raw evidence

Build each recorded source revision in a separate checkout:

```sh
cargo build --release --locked --bin zore --example compile_profile
```

Run from the candidate checkout with the matching binaries/probes:

```sh
python3 benchmarks/performance/run.py --suites compile --repetitions 3 \
  --compiler /path/to/candidate/zore \
  --compiler-revision e9ee2cad12f3505c7d0abeaf2040d31dd91ccc20 \
  --stage-profiler /path/to/candidate/compile_profile \
  --baseline-compiler /path/to/baseline/zore \
  --baseline-revision 46e66e076c5a0e57d984d2304ddcd48d96c8175b \
  --baseline-stage-profiler /path/to/baseline/compile_profile \
  --compile-workloads hello json rest rest_sample dependency suspension \
  --compile-lock-file /path/to/shared.lock \
  --work-directory /tmp/zore-build-scheduling-paired \
  --output /tmp/zore-build-scheduling-paired.json
```

The work directory must be new. The initial profile uses only the baseline
compiler/probe, omits the `--baseline-*` arguments and sets repetitions to one.
No `--warm-builds-only` flag was used: both captures contain cold builds.
Exact capture paths, commands and binary/source identities are in the raw JSON.

- [build-scheduling-baseline-linux-2026-10-25.json.gz](../benchmarks/performance/build-scheduling-baseline-linux-2026-10-25.json.gz); decompressed SHA-256:
  `74f7bb2a46af333cf8b437618c302f09db009f9177c264c0c3d4f8e6f12182a0`.
- [build-scheduling-paired-linux-2026-10-25.json.gz](../benchmarks/performance/build-scheduling-paired-linux-2026-10-25.json.gz); decompressed SHA-256:
  `fb6efdbc04d4063b6b7373a760f87c78e3f3b27c69065495efa44b40a50bda55`.

## Verification and limits

The real-tool regression requires both compiler stages to start before either
continues, then checks that cold/warm binaries execute and a warmed runtime is
reused. It fails against the original serial driver. Separate forced object,
runtime and combined failures verify that both stages finish and diagnostics
retain their priority. Existing missing-toolchain, frontend independence,
unusable-cache fallback and concurrent cache-publication checks pass.

Local verification passed formatting, strict all-target Clippy, locked build,
six runtime-cache units, eight selected native tests (including real HTTP), one
static-native-library integration test and all twelve performance-harness tests.
The optional frontend-control test was run using the already-built compiler
through its expected `target/debug/zore` path. Full all-target and host results
belong to PR CI; they are not inferred from Linux measurements.
