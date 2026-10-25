# Native application builds — 2026-10-23

The later [2026-10-25 scheduling report](performance-build-scheduling.md)
remeasures current main and records overlapping runtime/object compilation.
The historical measurements below retain their original source identities.

Issue [#156](https://github.com/maasdi/zore/issues/156) profiles current main
before choosing a focused native-build improvement. These measurements follow
[#127/#128](performance-followups.md); their internal-linkage change is already
present in both compilers here.

## Current-main profile

The release compiler and embedded runtime are from
`7cad71391c40eeb6886451518bc90f011bbdedd1`. Its binary SHA-256 is
`190bb126a7b6a9b4743dac5a241abd2950ee1106a76efa069a28ad49c79a6328`.
The library probe uses that same compiler source with only the benchmark example
registered in Cargo. The runner checkout contains the new profiling harness;
raw metadata records its dirty state and preserves the exact runner sources.
The runtime implementation is unchanged by this issue.

Host: Linux 6.18.55 x86-64, Intel Xeon Platinum 8573C, three CPUs in process
affinity, two-CPU cgroup quota, 8 GiB memory limit. Toolchains: rustc 1.98.1
(48a229cea), Debian clang 19.1.7, Python 3.12.14. Heavy jobs share a host lock;
measurements never overlap other participating compiler builds or test jobs.
The host runs other tasks between measurement leases, so filesystem state and
allocator/CPU state are not controlled experiments.

Each workload has three fresh check, warm-build and cold-runtime-cache-build
processes. All tables report medians in seconds unless labeled bytes.
“Cold” means a fresh runtime artifact cache; page/filesystem caches stay warm.
Every warm cache is write-probed and its runtime rlib hashes, sizes and mtimes
must stay unchanged. Every cold cache starts empty and must record a runtime
compile and retain the result. No fallback rebuild is counted as warm reuse.

| Workload | Check | Warm build | Cold cache build | Clang object | IR bytes | Object bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.0024 | 0.274 | 4.544 | 0.031 | 3,566 | 1,200 |
| JSON | 0.1175 | 1.979 | 5.969 | 1.589 | 3,785,197 | 155,848 |
| REST | 0.3059 | 25.319 | 29.761 | 24.638 | 12,283,363 | 1,204,312 |
| Multi-file REST sample | 0.3014 | 42.013 | 46.025 | 41.361 | 11,969,997 | 1,505,840 |
| Multi-file local dependency | 0.0899 | 0.714 | 5.217 | 0.350 | 2,133,812 | 31,040 |
| Suspended payload | 0.0192 | 0.759 | 4.820 | 0.488 | 1,483,304 | 40,656 |

The dependency application has two application files and an explicit local
manifest dependency with two source files; the dependency encodes JSON using
`std/encoding/json`. Its entire input tree and hashes are recorded in metadata.

The separate library probe attributes frontend work without instrumenting the
compiler CLI or subtracting check wall time from build wall time:

| Workload | Load | Check/ownership | MIR/drop/async lowering | Full IR emission | Rust entry/link | Nested native link | Cold runtime compilation |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.0002 | 0.0001 | 0.0000 | 0.0000 | 0.117 | 0.052 | 4.196 |
| JSON | 0.0037 | 0.1075 | 0.0038 | 0.0263 | 0.121 | 0.054 | 3.977 |
| REST | 0.0079 | 0.3003 | 0.0125 | 0.0795 | 0.128 | 0.061 | 4.139 |
| Multi-file REST sample | 0.0075 | 0.2883 | 0.0123 | 0.0767 | 0.128 | 0.056 | 4.033 |
| Multi-file local dependency | 0.0036 | 0.0826 | 0.0018 | 0.0153 | 0.119 | 0.053 | 4.449 |
| Suspended payload | 0.0011 | 0.0150 | 0.0017 | 0.0098 | 0.121 | 0.053 | 4.027 |

Rust entry/link includes its nested native link; these columns must not be
added together. The probe runs separately from each timed build and reports
full IR emission, before the new native-input pruning. Command timestamps also
retain process/wrapper overhead and rustc host-version queries. Those overheads
explain why stage columns do not sum to the end-to-end build measurement.

Clang LLVM-to-object work dominates both large REST builds. The historical
20.2/32.4-second figures are from another host/revision; they are not a baseline
for this experiment. The measurements above justified removing provably
unreachable internal function bodies before clang sees them.

## Focused implementation

The measured candidate compiler source is
`0a2d238aa0abd6b26b066f30dc313eebf916c298`, based on the main revision above.
Its release binary SHA-256 is
`235146019e8a548be07ad21a7595fa7882617192f37399162ad1ae1b192b67b2`.
The runner checkout contains uncommitted harness/report edits; raw metadata
records that state and preserves its exact runner source contents. Supplemental
hashes of every compiler/runtime Rust source and Cargo input were verified
against the candidate source commit. The raw report also preserves the exact
candidate patch against the baseline, so it can be reconstructed independently
of the local source commit's publication history. Production compiler/runtime sources stayed
unchanged during measurement.

Native linking retains externally visible functions and every module-level
function reference, then traverses direct references from those roots. Global
initializers, aliases, method tables, function-valued callbacks and async helper
pointers remain conservative roots. Unreachable private/internal definitions,
including isolated recursive cycles, are removed from clang's input. Quoted,
unquoted and hexadecimal-escaped LLVM symbol names share a canonical byte key.
Unexpected definition structure or invalid/duplicate symbols leave the module
unchanged. Global data, type declarations and the external runtime ABI are kept.

All source bodies still undergo complete loading, checking, ownership analysis,
MIR/drop insertion, async lowering and full LLVM emission. The public full-IR
emitter is unchanged. This is a driver-side reduction of native compiler input,
not a source-language reachability rule or a general optimizer. It intentionally
keeps functions referenced by otherwise unused global tables.

Focused tests cover dead mutual recursion, canonical symbol names, escaped
callbacks, aliases/metadata roots, malformed-input fallback and idempotence.
A real native fixture exercises initialization, callbacks, generic/interface
calls, scoped tasks, suspending captured ownership and cleanup. Existing unused
source diagnostics, entry-shim visibility, native ABI and cleanup tests remain
required. Correctness CI has no host-sensitive timing assertions.

## Paired measurements

The alternating comparison contains all 36 rows: six workloads, three
repetitions and two variants. Each workload runs baseline/candidate,
candidate/baseline, then baseline/candidate, with separate stable warm caches.
It was captured on 2026-10-23 on an AMD EPYC 9V74 host with the same Linux,
toolchains, three-CPU affinity, two-CPU quota and 8 GiB memory limit. The initial
stage profile above used the Intel host. Compare variants within this paired
capture; differences between its baseline and the initial profile are not
optimization results. Neither capture controls physical-host scheduling.

| Workload | Baseline warm | Candidate warm | Median reduction | Baseline clang | Candidate clang | Native-input IR bytes, before → after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.218 | 0.206 | 5.3% | 0.026 | 0.025 | 3,566 → 3,566 |
| JSON | 1.952 | 1.731 | 11.3% | 1.529 | 1.331 | 3,785,197 → 1,507,360 |
| REST | 21.863 | 21.036 | 3.8% | 21.213 | 20.419 | 12,283,363 → 8,467,592 |
| Multi-file REST sample | 34.650 | 34.008 | 1.9% | 34.003 | 33.413 | 11,969,997 → 9,012,454 |
| Multi-file local dependency | 0.642 | 0.515 | 19.8% | 0.329 | 0.186 | 2,133,812 → 303,652 |
| Suspended payload | 0.638 | 0.569 | 10.9% | 0.426 | 0.354 | 1,483,304 → 333,491 |

JSON, the dependency fixture and suspended payload have lower candidate build
and clang times in all three pairs. JSON's paired build reductions range from
5.9% to 11.3%; the table reports reduction of medians, not the median of paired
percentages. Three repetitions support a bounded observation, not statistical
confidence or a universal speedup claim.

Hello's identical IR and object show that its 12 ms median difference is
measurement variation. REST and the REST sample are slightly slower in the
third candidate pair despite smaller IR; their small median changes do not
establish a reliable improvement. Most of their clang work remains. Global
function tables conservatively retain many bodies; pruning global data or
changing that retention policy is outside this change. Further REST backend
work remains a measured follow-up to #156, without a claimed solution here.

Object bytes are unchanged for hello (1,200), JSON (155,848), dependency
(31,040) and suspension (40,656). REST falls only from 1,204,312 to 1,203,648,
and the REST sample from 1,505,840 to 1,505,800. Clang already removes most
unreachable code from the resulting object; this change saves some of its
input-processing work. Full public IR bytes remain identical between variants
for all six workloads. All 36 warm cache snapshots are nonempty and unchanged,
with no runtime compilation commands. Candidate cold-cache savings were not
measured; the initial baseline separately attributes roughly four seconds to
runtime compilation.

## Reproduction and raw evidence

Build each compiler with `cargo build --release --locked`, and its matching
probe with `cargo build --release --locked --example compile_profile`. The
current-main probe requires only the `compile_profile` example registration
and `benchmarks/performance/stages.rs` from this change; no production compiler
source modification is needed.

Follow the [measurement guide](../benchmarks/performance/README.md). Select
`--suites compile --repetitions 3` and provide both compiler paths/revisions and
matching stage profilers. `--compile-lock-file /path/to/shared.lock` acquires a
host lease around each warmup or measured variant, releasing between variants;
other checks can run between leases. Omit `--warm-builds-only` to measure both
warm and fresh-runtime-cache builds. Wrapper capture overhead is included for
both variants.

The exact capture arguments, absolute executable paths, host limits, command
vectors and per-command monotonic timestamps are in each artifact's metadata.
The paired invocation is reproducible with the following arguments after
building the matching binaries (replace the paths with local equivalents):

```sh
python3 benchmarks/performance/run.py --suites compile --repetitions 3 \
  --compiler /workspace/scratch/issue-156-candidate-zore \
  --compiler-revision 0a2d238aa0abd6b26b066f30dc313eebf916c298 \
  --baseline-compiler /workspace/scratch/issue-156-baseline-zore \
  --baseline-revision 7cad71391c40eeb6886451518bc90f011bbdedd1 \
  --stage-profiler /workspace/scratch/issue-156-candidate-stages \
  --baseline-stage-profiler /workspace/scratch/issue-156-baseline-stages \
  --compile-workloads hello json rest rest_sample dependency suspension \
  --compile-lock-file /workspace/scratch/zore-heavy-check.lock \
  --work-directory /workspace/scratch/issue-156-paired-warm-profile \
  --warm-builds-only --timeout 60 \
  --output /workspace/scratch/issue-156-paired-warm-profile.json
```

The initial profile uses only the baseline compiler/probe, omits the
`--baseline-*` options and `--warm-builds-only`, and uses a fresh work directory.
The runtime-cache directory is private to each workload and variant; wrapper
commands record its exact paths. Compile subprocesses use the harness command helper's 300-second timeout after
a lease is acquired; lock acquisition has no timeout. `--timeout 60` configures
runtime-suite protocols and does not change the compile command limit. Observed
paired leases were short (the largest workload takes about 36 seconds per
variant); the lock is released between variants.

Compressed raw JSON artifacts include all rows, source hashes, exact runner
source contents and capture configuration:

- [Initial current-main profile](../benchmarks/performance/build-current-main-linux-2026-10-23.json.gz),
  18 rows; uncompressed SHA-256
  `5127dba57de4dc7c05d03478a6ad5f573d0361758bbc442965bfbf4f324b833b`.
- [Paired warm comparison](../benchmarks/performance/build-paired-linux-2026-10-23.json.gz),
  36 rows; uncompressed SHA-256
  `3c522a2a0ece0c1ad37fe35ed3dfd0d1b5da61a499cd9fd97c4e0a665b52e519`.

The paired artifact includes the exact candidate patch against main and hashes
for 148 compiler/runtime Rust and Cargo inputs, verified against the measured
source commit. The measured source commit remains an ancestor of the report
commit; later harness/report edits do not change the compiler/runtime inputs.
Raw runner snapshots may precede final harness fixes; their preserved source
contents, rather than the final report commit, identify the capture code.

Validation: five pruning unit tests, six focused native linkage/timer tests,
eleven Python harness tests, formatting, strict all-target clippy, locked build
and all-target test compilation passed on Linux. The full local test suite was
not run; three-host PR CI remains required. Performance is opt-in evidence and
adds no timing gates to correctness CI.
