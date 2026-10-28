# Windows native build measurements

Issue [#205](https://github.com/maasdi/zore/issues/205) measures current-main
native build stages and the tradeoffs of overlapping runtime preparation with
clang object generation on Windows x64 MSVC. The
[historical Linux report](performance-build-scheduling.md) remains unchanged
as evidence for its recorded source and host.

The [capture harness](../benchmarks/performance/README.md#windows-build-capture)
builds an exact source revision and a matching serial control in isolated
worktrees. The control changes only the scheduling block; it is not a new
production compiler option. Release compiler/profiler identities, the control
patch, source manifests, cache evidence, LLVM/object hashes and all command
intervals are preserved. Separate linker timing is nested within rustc linking.

An initial profile covers hello, JSON, the REST fixture, the multi-file REST
sample, a local dependency and a suspended-payload control. A subsequent paired
comparison uses the same six workloads and three alternating repetitions, with
separate warm caches and fresh cold caches. Semantic/ownership checking and
code-generation optimization remain unchanged.

## Capture identity — 2026-10-28

[Workflow 40643702712](https://github.com/maasdi/zore/actions/runs/40643702712)
passed. Current-main compiler/runtime sources are
`113bcb81dd92accbecc41bbd2f857e0d3c1856bc`. The serial control is generated
commit `b025e129b0ad5f5fae86b5b817810d7cd0f0f85b`; its source tree is
reproducible from the public base and committed patch below. Its tracked-file
manifest differs from current main only in `compiler/src/driver/build.rs`.
Both worktrees were clean after their release builds. The capture harness is
`c43b54b96b9adf30156556fe4a5ae38964bfeb9d`, the PR merge tree for code head
`e5654ec985f4e1f76438b1b58f42a264a77c5f7a`; both report metadata snapshots
record a clean runner checkout.

The hosted Windows build was 10.0.26100.0, image `20261021.278.1`, with four
logical CPUs, process affinity mask 15, and 17,174,360,064 physical-memory bytes.
The processor identifier was Intel64 Family 6 Model 173 Stepping 1. Physical
host scheduling was uncontrolled. Tools were Rust 1.98.1 targeting x64 MSVC,
clang 20.1.8, MSVC tools 14.51.36231, Windows SDK 10.0.26100.0 and Python 3.13.16.
Absolute rustc/clang/linker paths and executable hashes are recorded, so scratch
directories cannot silently select another rustup default.

| Identity | Serial control | Current-main overlap |
| --- | --- | --- |
| Release compiler SHA-256 | `594a0b26e2e246511a23ce7a2692405ef789fe2f2c57c620035f51072afa5473` | `0cab2e3978b7acd3871da43662848095d498673071bfa75bf5177bf867120d23` |
| Stage profiler SHA-256 | `7c68b2d3881ceabd5c50974c35ac4bf0041064c7e902eb6c1f1b3cdcd99ee3fb` | `4a968a761cc244ec095dbfa1cb2109b65f62d74fd92930dc28f5ff48915fa9b3` |

The initial report contains six observations. The comparison contains 36 rows:
six workloads, three repetitions, two variants. Order is serial/overlap,
overlap/serial, serial/overlap within each workload. Warmups and each measured
variant hold the same host-local lease; each variant owns a private warmed cache,
and every cold build starts with a new runtime-artifact cache. This is not a
filesystem/page-cache flush. All 84 measured warm/cold profiles passed cache
validation, and all 378 recorded native tool commands exited successfully.

Every workload has one matching LLVM-input hash across initial and paired
warm/cold builds. Object sizes match within each workload, but object hashes
vary between builds. Their variation was not isolated; byte-identical objects
or binaries are not claimed. The source control changes scheduling only and
retains the same generated LLVM and optimization flags.

## Initial current-main profile

Durations are seconds, with one observation per workload. Entry/link includes
the separately recorded nested MSVC link; those columns must not be added.

| Workload | Check | Warm build | Cold build | Warm clang object | Cold runtime | Warm entry/link | Nested MSVC link |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.007 | 0.355 | 3.753 | 0.025 | 3.331 | 0.202 | 0.078 |
| JSON | 0.133 | 1.196 | 4.095 | 0.681 | 3.475 | 0.197 | 0.076 |
| REST fixture | 0.414 | 5.830 | 7.653 | 4.931 | 3.769 | 0.200 | 0.080 |
| Multi-file REST sample | 0.381 | 7.958 | 9.915 | 7.110 | 3.743 | 0.195 | 0.080 |
| Local dependency | 0.091 | 0.539 | 3.590 | 0.128 | 3.066 | 0.179 | 0.069 |
| Suspended payload | 0.024 | 0.559 | 3.552 | 0.226 | 3.114 | 0.181 | 0.070 |

The separate profiler runs the actual shared driver stages; it is outside the
timed build. Loading, checking/ownership, MIR/drop/async lowering, full emission
and pruning remain separate observations.

| Workload | Load | Check/ownership | Lower | Emit | Prune | Emitted / native IR bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Hello | 0.0009 | 0.0004 | 0.00002 | 0.0001 | 0.00003 | 3,566 / 3,566 |
| JSON | 0.0045 | 0.1218 | 0.0063 | 0.0292 | 0.0042 | 3,132,951 / 1,196,668 |
| REST fixture | 0.0094 | 0.3766 | 0.0216 | 0.1016 | 0.0153 | 12,211,503 / 6,161,513 |
| Multi-file REST sample | 0.0088 | 0.3662 | 0.0196 | 0.0976 | 0.0154 | 11,894,441 / 6,624,519 |
| Local dependency | 0.0044 | 0.0973 | 0.0044 | 0.0187 | 0.0022 | 1,734,589 / 286,240 |
| Suspended payload | 0.0014 | 0.0163 | 0.0024 | 0.0110 | 0.0014 | 1,307,572 / 315,142 |

## Alternating comparison and tradeoffs

Each entry is the median of three observations, in seconds. Cold reduction is
the ratio of the two medians, not the median of paired percentages.

| Workload | Warm serial / overlap | Cold serial / overlap | Cold reduction | Object/runtime overlap serial / overlap |
| --- | ---: | ---: | ---: | ---: |
| Hello | 0.382 / 0.315 | 3.462 / 3.405 | 1.6% | 0 / 0 |
| JSON | 1.109 / 1.057 | 4.219 / 3.796 | 10.0% | 0 / 0.600 |
| REST fixture | 5.576 / 5.340 | 9.590 / 7.495 | 21.8% | 0 / 3.530 |
| Multi-file REST sample | 8.406 / 8.138 | 11.700 / 10.114 | 13.6% | 0 / 3.665 |
| Local dependency | 0.592 / 0.534 | 3.699 / 3.491 | 5.6% | 0 / 0.052 |
| Suspended payload | 0.624 / 0.555 | 3.853 / 3.475 | 9.8% | 0 / 0.174 |

Every cold REST pair improved: 17.7–22.8% for the fixture and 11.9–20.5% for
the full sample. Their cold medians fell about 2.10 and 1.59 seconds. This is
smaller than their recorded overlap, while cold stage durations also changed;
elapsed stage times cannot be added as sequential work. Cold clang-object
medians increased from 4.675 to 6.667 seconds for REST and from 7.347 to 9.249
seconds for the full sample. Runtime medians were 3.772 / 3.530 and
3.311 / 3.665 seconds respectively. The capture does not isolate CPU, storage
or hosted scheduling as the cause of individual stage changes.

All six warm medians decreased, but individual warm pairs were mixed: hello
had a 10.0% regression, REST 2.8%, and the full sample 3.1%; JSON had a 0.1%
regression. One cold hello pair regressed 1.3%, and its native object/runtime
commands did not overlap in any repetition. Neither a universal warm speedup
nor a native-compilation overlap benefit for hello is established.

Build wall time includes launcher/Python startup, LLVM capture and command
instrumentation. Both variants use the same instrumentation, but its cost can
interact with scheduling, particularly for small or warm builds. Three repeats
on one hosted runner support these bounded observations, not statistical
confidence, uninstrumented latency guarantees or cross-host parity. This issue
adds measurements; any further optimization needs a separate scoped change.

## Reproduction and raw evidence

The opt-in workflow runs the following with a native MSVC environment, explicit
LLVM clang and recorded host JSON:

```powershell
python benchmarks/performance/windows_capture.py `
  --source-revision 113bcb81dd92accbecc41bbd2f857e0d3c1856bc `
  --host-description C:\capture\host.json `
  --directory C:\capture\new-run
```

It builds both release compilers and matching profilers before measurement.
The provenance file preserves exact commands, source manifests, the serial
control's generated commit identity, native tool paths/hashes and overlap
verification. Initial and paired reports preserve every row, compiler/profiler
and launcher hash, runner-source hash, command interval and runtime-cache snapshot.
Binary hashes are identities recorded during the run; the tool/compiler binaries
are not preserved in these archives for independent rehashing.
The original JSON bytes from artifact `12796289036` are gzip-compressed below;
SHA-256 values refer to decompressed JSON.

| Evidence | SHA-256 |
| --- | --- |
| [Capture provenance](../benchmarks/performance/build-windows-capture-2026-10-28.json.gz) | `12122790cd0eb35539587bda8e385527edc88c19779e63c0ae867fc58acbff90` |
| [Initial current-main profile](../benchmarks/performance/build-windows-initial-2026-10-28.json.gz) | `1cbbcef9c523615fb60cb61ba55d0921752967f436481123edd924e73b900c94` |
| [Alternating comparison](../benchmarks/performance/build-windows-paired-2026-10-28.json.gz) | `95b4a10af666602295bbea1564f5a3d4175766224dabd93858ecc47b50de87fa` |
| [Serial control patch](../benchmarks/performance/build-windows-serial-control-2026-10-28.patch), original bytes | `2bd5dcd458e873dd7e4aa86aeebef79f71d145d5451d4bbb9282a3727e71b159` |

## Validation

The measurement workflow passed its Windows harness checks and the complete
initial/paired capture. Native launcher tests cover shell-free argument and exit
forwarding; capture profiles verify real MSVC linking. Portable tests cover
cache evidence, variant order, input drift, compiler-control isolation, concurrent
command records, host metadata, leases and timeout child-tree cleanup.
Integration correctness CI and review are tracked in
[PR #207](https://github.com/maasdi/zore/pull/207). The first Linux CI run exposed
a test-only race when a terminated child's `/proc` entry vanished during an
assertion; the assertion now accepts an already-removed process. That correction
does not change the measured harness or compiler. Local native build/Clippy/tests
remain blocked by the absent MSVC linker; local Python, imports and formatting
checks passed. No timing or RSS limits were added to correctness CI.
