# Runtime population measurements — 2026-10-24

This is the historical three-host capture. See the
[2026-10-26 all-host recapture](runtime-scaling-refresh.md) for current harness
evidence. The original Windows archive remains here as historical evidence.

Issue [#157](https://github.com/maasdi/zore/issues/157) adds an opt-in
[cross-host harness](../benchmarks/runtime-scaling/README.md) and deterministic
small-capacity helper/deadline tests. All three hosts completed all eight
builds and 96 fresh-process measurements. The results identify Windows timer
thread cost, socket helper admission and delayed helper retirement for linked
implementation follow-ups.

## Identity and reproduction

The Linux and macOS reports come from hosted CI runners of one pull request
checkout, and the Windows report from a later run of the same runtime and
harness sources. All three were produced from a clean checkout and record
identical runtime source hashes. This is a comparison of host implementations,
with no optimization before/after claim. Each report records the compiler
executable SHA-256, source revision, toolchain identities and every runtime and
harness source hash; the raw reports below are the stable evidence.

The reports were produced by earlier revisions of the harness and runtime
sources. Their embedded source manifests identify those historical versions;
they do not identify the current tree. The initial post-capture
`run.py`, `cc.cmd` and `README.md` edits change build order (the runtime
library is compiled once instead of once per variant), portability (POSIX-style
source keys, the Python interpreter running the harness, an optional
`--compiler-revision`) and error handling, not the workload, the
instrumentation or what is measured; `test_harness.py` gained tests. The
`reactor.rs` difference was its module documentation line only. Subsequent
runtime and harness changes are covered by the separate recapture above; these
original report bytes remain unchanged.

Windows host: Windows Server 2025, build 10.0.26100, AMD64, four reported CPUs,
processor `AMD64 Family 25 Model 1 Stepping 1, AuthenticAMD`, and
17,174,360,064 reported physical-memory bytes. The hosted runner image was
`windows-2025-vs2026`, version `20261021.278.1`. Rust targets
`x86_64-pc-windows-msvc`; clang is 20.1.8 with the same target, using MSVC Build
Tools 14.51.36231 and Windows SDK 10.0.26100.0. The compiler executable SHA-256 is
`64422db55d33964519f08b0c14bafc731921f411d361a2a53f20e95c40b35f6d`.

| Identity | Linux | macOS |
| --- | --- | --- |
| OS | Linux 6.17.0-1022-azure, glibc 2.39 | macOS 26.6.2 |
| Architecture / reported CPUs | x86_64 / 4 | arm64 / 3 |
| Reported physical memory, bytes | 16,770,797,568 | 7,516,192,768 |
| Open-file soft / hard limit | 65,536 / 65,536 | 10,240 / 9,223,372,036,854,775,807 |
| Process soft / hard limit | 63,872 / 63,872 | 1,333 / 2,000 |
| Rust host | x86_64-unknown-linux-gnu | aarch64-apple-darwin |
| clang | Ubuntu 18.1.3 (1ubuntu1) | Apple 21.0.0 (clang-2100.1.1.101) |
| Compiler executable SHA-256 | `57e71c71425034520f15d5dabdafdb8d84b59788a3d25e42afabb07c463dcc2f` | `2387afaeb227dbbfcd8b365d9afe40465aeac33eb156adb4966aca45ee401846` |

All three hosts used Rust 1.98.1, commit
`48a229ceaefd4985c50990b14116b6d856af0985`, LLVM 22.1.8, Python 3.13.16 and
psutil 7.2.2. Full toolchain strings, platform identities, executable/workload/IR
hashes and every runtime/harness source hash remain in the raw reports. The
Linux processor string identifies only `x86_64`; no CPU model is inferred.

Build the recorded source with `cargo build --release --locked`, install
`psutil==7.2.2`, and use the command in the harness guide with the matching
compiler revision. Defaults reproduce the experiment: four modes (`tasks`,
`timers`, `sockets`, `socket-deadline`), populations 32/128/512/640, payloads
0/4,096 ints, three fresh processes per combination and two create/release
cycles in each process. Each cycle samples baseline, active, completed and
after six seconds idle; OS samples span at least 300 ms at 50 ms intervals.
The process deadline is 120 seconds, socket completion deadlines are 120
seconds or five seconds in the expiry case, and context deadlines are one hour
before explicit parent cancellation. Every process uses two scheduler workers.

The workload checks task results, payload contents, one-byte reads, and exact
timeout errors with zero bytes. Population admission waits for registered Unix
operations or admitted/queued Windows helper work. The timer mode includes two
context watchers per user task; its active snapshot can still contain runnable
watcher work. Snapshots and OS samples are sequential observations.

## Unix populations and pinned storage

All four populations completed in all modes, both payload variants and every
repetition on both Unix hosts: 96/96 processes and 192 complete cycles per host.
No experimental failure point was reached at 640. This does not promise service
capacity beyond, or even at, that population for arbitrary workloads.

Worker frame sizes are padded LLVM ABI bytes, identical on these two hosts and
for both payload variants. Separate watcher, task and spawn-wrapper storage is
included in allocation counters, rather than attributed to this one frame.

| Mode | `main.wait` frame, bytes | Active tasks at population 640 | Active ABI bytes above settled control at 640, zero payload |
| --- | ---: | ---: | ---: |
| Tasks | 312 | 640 | 371,712 |
| Timers | 696 | 1,920 | 1,569,856 |
| Sockets | 528 | 640 | 740,992 |
| Socket deadline | 544 | 640 | 751,232 |

The table's allocation increments are exact in every repetition and cycle on
both hosts, using the settled control allocation of 4,136 bytes. The active
control includes its scanner/buffers and a population-dependent Task array;
its capacity grows in steps, so division by task count is not a frame size.
Baseline control scanning can temporarily hold another 16 bytes, which is why
active-minus-baseline raw deltas can be 16 bytes smaller than the table.

The 4,096-int variant adds exactly `population * 4,096 * 8` live ABI bytes to
the paired zero-payload active checkpoint, for every mode, repetition and cycle:
1,048,576 / 4,194,304 / 16,777,216 / 20,971,520 bytes at the four populations.
The heap payload is read after suspension and kept separate from pinned frames.
The frame layouts themselves do not grow with the dynamic payload.

Both Unix hosts retained two scheduler workers at every active checkpoint.
Linux had one idle helper and five OS threads; macOS had one or two idle helpers
and five or six OS threads. Timer mode registered one cancellable
timer per user task, and both socket modes registered one read and deadline per
connection. None used per-deadline timer threads or queued socket helpers.

## Cleanup, delayed resources and RSS

At all 192 after-idle checkpoints on each Unix host, the runtime reported:

- 4,136 live ABI bytes, two scheduler workers and one idle helper;
- zero tasks, queued tasks/helpers, blocked workers, socket handles, readiness
  registrations, live timers, stale timer records, channels or mutexes;
- one text buffer for the control scanner, five OS threads and six descriptors.

For every one of the 96 processes per host, every settled runtime counter was
identical in cycle two and cycle one. The settled ABI-byte difference was
exactly zero, including payload cases. Some macOS completed checkpoints still
had a second idle helper; after the six-second idle interval every checkpoint
had one. This is bounded helper retirement in these observations.

Live ABI bytes cover allocations through the Zore allocation ABI. They exclude
Rust scheduler/helper/reactor containers, thread stacks, allocator bookkeeping
and other process memory. Flat counters over two cycles do not establish a
whole-process leak bound or attribute every retained RSS byte.

The following are RSS increments above that cycle's baseline at population
640 with the 4,096-int payload, in KiB. Each entry is min / median / max of six
checkpoint medians (three processes, two cycles), retaining first-cycle and
reuse-cycle variation. These are descriptive observations, not paired-process
confidence intervals or performance thresholds.

| Host / mode | Active RSS increment, KiB | After-idle RSS increment, KiB |
| --- | ---: | ---: |
| Linux tasks | 16,396 / 20,812 / 22,836 | 32 / 4,602 / 6,716 |
| Linux timers | 12,256 / 22,140 / 26,352 | -640 / 5,068 / 13,972 |
| Linux sockets | 804 / 12,208 / 22,280 | 8 / 9,966 / 21,344 |
| Linux socket deadline | 1,272 / 12,596 / 22,696 | -136 / 9,576 / 20,952 |
| macOS tasks | 96 / 20,024 / 40,224 | 96 / 20,088 / 40,336 |
| macOS timers | 240 / 21,408 / 41,344 | 240 / 21,528 / 41,488 |
| macOS sockets | 16 / 12,472 / 22,768 | 16 / 12,496 / 22,816 |
| macOS socket deadline | 48 / 11,584 / 22,720 | 48 / 11,608 / 22,720 |

RSS can remain elevated when measured live allocations and resources return to
their control counts. Allocator/container retention and stack caching are
possible contributors, not isolated causes. Reusing memory in the second cycle
also reduces some active-minus-baseline increments; negative settled increments
mean the sampled process used less resident memory than that cycle's baseline.
The experiment does not establish an unbounded leak or a portable host ranking.

## Windows populations, cleanup and implementation follow-ups

Windows completed all 96 processes and 192 cycles, including population 640 in
both payload variants of every mode. No failed experimental scale point was
reached.

The following active counts at population 640 cover both payload variants,
three processes each and both cycles. Runtime counts are snapshot ranges; OS
threads span every raw sample at those checkpoints.

| Mode | Scheduler tasks | Helpers | Queued helper jobs | Timer threads | OS threads |
| --- | ---: | ---: | ---: | ---: | ---: |
| Tasks | 640 | 1 idle | 0 | 0 | 7 |
| Timers | 1,920 | 1 idle | 0 | 640 | 647 |
| Sockets | 640 | 512 | 140–339 | 0 | 518 |
| Socket deadline | 640 | 512 | 131–324 | 0 | 518 |

Context deadlines therefore have a measured per-deadline thread cost. Socket
deadlines are handled by the retrying socket helpers, rather than separate
timer threads. The helper cap is 512 and its queue is unbounded; population
640 demonstrates admitted and queued progress, rather than 640 simultaneous
helper calls. Queue counts describe submitted jobs at a transient checkpoint,
not a one-to-one census of user reads or a precise `population - capacity`
formula. No service-capacity or Windows/Unix performance-parity promise follows.

Windows has the same worker ABI frame sizes and exact paired dynamic-payload
increments as Unix. Task and timer active ABI totals also match. At population
640, each zero-payload socket variant has another 15,360 live ABI bytes versus
its Unix counterpart (24 per connection); this experiment does not isolate the
owner of that extra allocation component. Rust helper/timer state, stacks and
allocator overhead remain outside the ABI counter.

Every Windows completed and after-idle checkpoint returned to 4,136 live ABI
bytes, zero tasks, queued tasks/helpers, blocked workers, socket handles,
readiness registrations, live timers, channels and mutexes, with
two scheduler workers and one control text buffer. Every process's settled
cycle-two minus cycle-one live ABI allocation difference was exactly zero.
Task and context-timer cases returned to one idle helper, seven OS threads and
49 process handles after idle; every after-idle timer-thread count was zero.
Completed snapshots recorded zero or one remaining timer thread. Timer
completion can briefly precede OS thread exit: a zero-payload population-640
completed sample still saw 332 threads,
although its runtime timer-thread counter was already zero; after idle it had
seven. These counters and OS observations are not atomic.

Socket helper retirement was incomplete after the six-second idle interval,
despite the nominal five-second idle policy. At population 640, successful-read
cases retained 448–505 idle helpers and deadline cases 479–509. Their queues,
tasks and owned socket tables were empty. Across those socket cases the raw OS
samples retained 454–515 threads and 960–1,082 process handles. Windows handles
and Unix descriptors are different units, and process handle types are not
identified here. Across all Windows cases, settled cycle-two minus cycle-one
idle-helper differences ranged from -43 to +53 (median zero). This is measured
delayed retirement requiring longer tail observation and diagnosis; two cycles
do not establish its cause, eventual retirement time or unbounded growth. The
observed helper population stayed within the production cap of 512.

The same population-640 payload RSS summary convention as the Unix table gives:

| Mode | Active RSS increment, KiB | After-idle RSS increment, KiB |
| --- | ---: | ---: |
| Tasks | 21,452 / 22,778 / 23,572 | -488 / 1,050 / 2,292 |
| Timers | 35,216 / 36,316 / 37,684 | -20 / 1,264 / 2,704 |
| Sockets | 21,892 / 27,438 / 32,416 | 340 / 5,622 / 10,932 |
| Socket deadline | 21,640 / 27,038 / 32,356 | -32 / 5,596 / 11,088 |

The selected implementation follow-ups are:

- [#169: shared Windows cancellable timing](https://github.com/maasdi/zore/issues/169),
  addressing the measured per-deadline timer-thread cost;
- [#170: shared Windows socket readiness and queued deadlines](https://github.com/maasdi/zore/issues/170),
  addressing helper admission and queued progress;
- [#171: helper idle retirement and longer tail measurements](https://github.com/maasdi/zore/issues/171),
  diagnosing retirement independently for all blocking-helper users.

They must preserve cooperative scheduling and current results/errors,
partial-write counts, deadlines, cancellation and deterministic
normal/error/panic cleanup. Frame shrinking and forced preemption remain
separate work; the frame/payload accounting does not select either change.

Correctness checks use a private helper capacity of one, gated work, two queued
jobs, an expired real socket-read deadline, nested helper submission and idle
cleanup. They do not require hundreds of threads or assert RSS/latency limits.
Native correctness CI remains separate from this opt-in measurement workflow.
These linked measured issues satisfy #157's split-follow-up option; this report
does not itself implement shared Windows infrastructure.

## Raw evidence

These gzip files preserve the original JSON bytes, including every repetition,
cycle, runtime snapshot, OS sample, frame layout, build result and identity.
SHA-256 values refer to decompressed JSON.

| Report | Decompressed SHA-256 |
| --- | --- |
| [Linux](../benchmarks/runtime-scaling/linux-2026-10-24.json.gz) | `97981f5b05d654f1f6c0020c4dc09856dae385742d016cdfcfa9a2651caf2656` |
| [macOS](../benchmarks/runtime-scaling/macos-2026-10-24.json.gz) | `ed79f2bed19e826cbf601b70a89405a5e8b5607ac7c6ac91f2674c5ce267d5f4` |
| [Windows](../benchmarks/runtime-scaling/windows-2026-10-24.json.gz) | `04c883eb94a7b612addde1267bffdc679eeb2b4f29e830a1a8b6de0509949186` |

For example, `gzip -dc benchmarks/runtime-scaling/linux-2026-10-24.json.gz`
recovers the unchanged Linux report. Earlier Linux performance, frame-storage
and REST-retention baselines remain available in their original reports.
