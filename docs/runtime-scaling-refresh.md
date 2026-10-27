# Runtime population recapture — 2026-10-26

The [#177](https://github.com/maasdi/zore/issues/177) recapture is complete. The
[labeled PR run](https://github.com/maasdi/zore/actions/runs/40278626042)
captured Linux, macOS and Windows after the harness maintenance. Each host
completed all eight builds, 96 fresh-process measurements and 192
create/release cycles. Every after-idle checkpoint returned to 4,136 live ABI
bytes with no live tasks, sockets, timers or queued work. Windows retained idle
helpers at the six-second checkpoint; the measurements are detailed below.
The [2026-10-24 report](runtime-scaling.md) and its original raw files remain
available as historical evidence.

## Identity and reproduction

The capture used the opt-in [measurement workflow](https://github.com/maasdi/zore/actions/runs/40278626042)
on PR #199, head `8966659c46260adc69fb15347dcac1ad32bc2c12`, based on main
`2eb792c3cfa76e65899d7d49446e7b4663f9b1ce`. Each runner built its release
compiler from the same clean merge checkout. The measured source/compiler
revision is `d24f0ae6868e8835a0a67fef78af5c357ce805fc`; all reports record a clean
source tree and matching runtime/harness manifests. Report publication changes
documentation and adds raw data after the capture.

| Identity | Linux | macOS | Windows |
| --- | --- | --- | --- |
| OS | Linux 7.0.0-1017-azure, glibc 2.43 | macOS 26.6.2 | Windows Server 2025, build 10.0.26100 |
| Architecture / reported CPUs | x86_64 / 4 | arm64 / 3 | AMD64 / 4 |
| Reported physical memory, bytes | 16,758,403,072 | 7,516,192,768 | 17,174,360,064 |
| Open-file soft / hard limit | 65,536 / 65,536 | 10,240 / 9,223,372,036,854,775,807 | Not reported |
| Process soft / hard limit | 58,109 / 58,109 | 1,333 / 2,000 | Not reported |
| Rust host | x86_64-unknown-linux-gnu | aarch64-apple-darwin | x86_64-pc-windows-msvc |
| clang | Ubuntu 21.1.8 (6ubuntu1) | Apple 21.0.0 | LLVM 20.1.8 |
| Compiler executable SHA-256 | `b895b4de8232d6432ae453cbfbb0a9abcba84c1c7cf85980cba229ad932f12ef` | `6cabfb00c0a3c68614540e13fd25b6c7c88f91294d83ff6f5b439ec2057bb2d2` | `1ff6b9651df66a4f62348d1f6a9f536a531f1f4c0b9c783187dfb4ccd4822b1b` |

All hosts used Rust 1.98.1, commit
`48a229ceaefd4985c50990b14116b6d856af0985`, LLVM 22.1.8, Python 3.13.16 and
psutil 7.2.2. Full identities and per-variant workload, IR and executable hashes
are preserved in the raw reports. The Windows processor string identifies an
AMD64 Family 26 Model 2 processor; no retail CPU model is inferred.

The current harness builds one snapshotted runtime library per invocation,
uses POSIX-style runtime source keys, and no longer imports the performance
harness. The performance stage probe also shares the driver's actual native-IR
pipeline, including reachability pruning; its separate regression tests remain
part of correctness CI.

Build the recorded checkout with `cargo build --locked --release`, install
`psutil==7.2.2`, and run the defaults from the
[harness guide](../benchmarks/runtime-scaling/README.md):

```sh
python benchmarks/runtime-scaling/run.py \
  --compiler "$PWD/target/release/zore" \
  --compiler-revision "$(git rev-parse HEAD)" \
  --output /tmp/runtime-scaling.json
```

Defaults cover tasks, timers, successful socket reads and expired socket reads;
populations 32/128/512/640; payloads zero and 4,096 ints; three fresh processes
per combination; and two create/release cycles per process. Every cycle records
baseline, active, completed and six-second idle-cleanup checkpoints. OS sampling
targets 300 ms in nominal 50 ms steps. Each process uses two scheduler workers;
the measurement deadline is 120 seconds. Socket deadlines are 120 seconds for
successful reads and 30 seconds for expiry. The initial Windows attempt used a
five-second expiry and could time out while admitting 512/640 tasks; the longer
harness deadline lets the active checkpoint settle. Context timers are
explicitly cancelled before their one-hour deadlines.

Each report contains exactly the expected 96 distinct mode/payload/population/
repetition combinations and eight ordered checkpoints per process. Every build
and process succeeded, with exit code zero, empty stderr and no harness-forced
termination. All recorded source hashes match the measured revision. Linux
checkpoints contain seven OS samples spanning 300.732–305.931 ms; macOS has
three to seven spanning 300.024–509.207 ms; Windows has seven spanning
302.924–319.872 ms. Sampling cost and scheduling affect those observed spans;
they are not timing guarantees.

## Populations and pinned storage

All four populations completed in every mode and payload variant on all three hosts.
No experimental failure point was reached at 640. This is not a service-capacity
promise for arbitrary workloads.

The following padded LLVM ABI sizes and active counts at population 640 match
on all hosts and in both payload variants. Allocation increments use the
settled control allocation of 4,136 bytes, not transient baseline scanner work.

| Mode | `main.wait` frame, bytes | Active tasks at 640 | Active ABI bytes above control at 640, zero payload |
| --- | ---: | ---: | ---: |
| Tasks | 312 | 640 | 371,712 |
| Timers | 864 | 1,920 | 2,174,080 |
| Sockets | 528 | 640 | 740,992 |
| Socket deadline | 544 | 640 | 751,232 |

The timer worker frame was 696 bytes in the historical report; the current
capture measures 864. The current active timer allocation also includes its
context watchers and synchronization state. The two reports measure different
implementations and are not a paired experiment attributing that change.

The 4,096-int variant adds exactly `population * 4,096 * 8` live ABI bytes at
every paired active checkpoint: 1,048,576 / 4,194,304 / 16,777,216 / 20,971,520
bytes at the four populations. Dynamic payloads do not enlarge the frame layout.
Separate watcher, task, spawn-wrapper and control-array storage is included in
the allocation counter rather than attributed to one worker frame.

All hosts retain two scheduler workers. At population 640, Linux and macOS
register 640 cancellable deadlines and 640 socket reads with the shared reactor.
Windows starts 640 timer threads for the timer mode. Its socket modes reach the
512-helper cap with 215–362 queued helper jobs, showing queued progress at this
population rather than 640 simultaneous helper calls. Timer active snapshots can
include runnable context-watcher work: queued-task counts range from 0–196 on
Linux, 19–597 on macOS and 373–427 on Windows in the zero-payload population-640
captures. These transient task queues are empty at every after-idle checkpoint.

## Cleanup and RSS

All 192 after-idle checkpoints per host return to 4,136 live ABI bytes, with
zero tasks, queued tasks/helpers, blocked workers, socket handles, readiness
registrations, live timers, stale timer records, timer threads, channels or
mutexes. They retain two scheduler workers and one control-scanner text buffer.
Linux and macOS return to one helper and five OS threads. Windows timer
checkpoints return to one helper and seven OS threads. Windows socket and
socket-deadline checkpoints retain 237–489 idle helpers at population 640; all
queued work is empty and every task and socket has completed. The control
scanner also uses helper work, and snapshots are not atomic observations of the
whole process.

Every process has exactly zero settled cycle-two minus cycle-one live ABI bytes.
The retained Windows helpers are evidence of bounded idle-pool retention at a
six-second checkpoint, not evidence of an unbounded leak. Longer idle checkpoints
and more reuse cycles remain necessary to locate the retirement tail.

The table shows RSS increments above the same cycle's baseline at population
640 with a 4,096-int payload, in KiB. Each cell is min / median / max of six
checkpoint medians, preserving three processes and both cycles. These descriptive
statistics are not confidence intervals or performance thresholds.

| Host / mode | Active RSS increment, KiB | After-idle RSS increment, KiB |
| --- | ---: | ---: |
| Linux tasks | 14,248 / 21,306 / 22,796 | -324 / 1,836 / 8,692 |
| Linux timers | 15,144 / 23,546 / 27,640 | -20 / 5,150 / 12,448 |
| Linux sockets | 2,872 / 13,010 / 22,780 | 12 / 9,546 / 20,360 |
| Linux socket deadline | 3,872 / 15,002 / 22,540 | 36 / 7,784 / 19,296 |
| macOS tasks | 16 / 20,040 / 40,256 | 16 / 20,096 / 40,272 |
| macOS timers | 384 / 20,960 / 39,424 | 400 / 21,752 / 40,912 |
| macOS sockets | 32 / 11,320 / 22,928 | 32 / 11,336 / 22,944 |
| macOS socket deadline | 64 / 11,400 / 23,840 | 64 / 11,424 / 23,872 |
| Windows tasks | 21,748 / 22,854 / 23,392 | -324 / 1,154 / 1,980 |
| Windows timers | 36,400 / 37,494 / 38,616 | 412 / 1,336 / 2,252 |
| Windows sockets | 22,664 / 28,236 / 32,816 | 852 / 5,712 / 10,336 |
| Windows socket deadline | 23,592 / 28,204 / 32,468 | -1,540 / 5,008 / 9,100 |

## Interpretation and remaining work

Live ABI bytes include allocations through the Zore allocation interface,
including generated frames and dynamic arrays. They exclude Rust runtime
containers, thread stacks and allocator bookkeeping. Frame ABI sizes and dynamic
payload requests are separate quantities. Runtime snapshots and OS samples are
sequential observations, so transient counts need not describe one atomic state.

RSS can remain elevated after live allocations and owned resources return to
control counts. Two cycles do not establish a whole-process leak bound or isolate
allocator and stack caching. Host, toolchain, runtime and harness revisions
differ from the historical capture; these results are not a paired optimization
comparison or a portable host ranking. Correctness CI has no performance limits.

Issue #177's all-host recapture is complete. The Windows evidence unblocks the
measurement-informed runtime work in
[#169](https://github.com/maasdi/zore/issues/169),
[#170](https://github.com/maasdi/zore/issues/170) and
[#171](https://github.com/maasdi/zore/issues/171). The measurement workflow
defaults to Linux/macOS on manual dispatch; the `run-scaling-benchmarks` PR
label or `include_windows` manual input runs all three hosts. Ordinary Windows
correctness CI remains enabled.

## Windows timer follow-up (#169) — 2026-10-26

The [labeled after-run](https://github.com/maasdi/zore/actions/runs/40294003728)
measured PR #200's shared timer service on Windows, Linux and macOS. Each host
completed all eight builds, 96 fresh-process measurements and 192 create/release
cycles. All reports record clean source trees and matching runtime/harness
manifests. The Windows host was Server 2025 build 10.0.26100 (AMD64 Family 25
Model 17), Rust 1.98.1 and LLVM clang 20.1.8. The measured source revision is
`573da4b85ddeecbc7b72cdfd60c980d325c7a217`, from PR head
`013b2c1c5e3bd80b0a15b93ac4dec12cee607daa`.

At 640 simultaneous deadlines, the before run registered 640 timer threads and
647 OS threads. The after run registered 640 timers on one shared service
thread, with eight OS threads. After six seconds idle, all timer registrations
and task work were gone; the shared timer service remained as one idle thread.
The timer service adds one idle OS thread compared with the seven-thread before
checkpoint. Both runs returned live ABI allocations to 4,136 bytes. These
counts matched for zero payload and 4,096-int payload runs.

| State | Live timers, before / after | Timer threads, before / after | OS threads, before / after | Live ABI bytes, before / after |
| --- | ---: | ---: | ---: | ---: |
| Active at 640 | 640 / 640 | 640 / 1 | 647 / 8 | 2,178,216 / 2,178,216 (zero payload); 23,149,736 / 23,149,736 (4,096 ints) |
| Six-second idle | 0 / 0 | 0 / 1 | 7 / 8 | 4,136 / 4,136 |

The table below gives RSS increments over each cycle's baseline in KiB. Each
cell is min / median / max of the six per-cycle baseline-subtracted medians.
Thread stacks and allocator caching make RSS a host-specific observation.

| Payload | Active RSS before | Active RSS after | Idle RSS before | Idle RSS after |
| --- | ---: | ---: | ---: | ---: |
| Zero | 15,208 / 15,576 / 16,196 | 3,636 / 4,206 / 4,664 | 136 / 692 / 1,160 | -28 / 542 / 1,108 |
| 4,096 ints | 36,400 / 37,494 / 38,616 | 24,864 / 26,104 / 26,832 | 412 / 1,336 / 2,252 | -104 / 1,066 / 1,876 |

The runs matched workloads and configuration but used distinct runtime/compiler
revisions and hosted runner allocations. These observations provide resource
evidence; they are not portable performance guarantees. The exact before and
after reports preserve all process samples, checkpoints, compiler identities
and source manifests.

## Windows socket readiness follow-up (#170) — 2026-10-27

The implementation replaces helper-held accept/read/write waits on Windows
with one shared `WSAPoll` service. The compiler uses the same pollable socket
state machine on Windows, Linux and macOS. Windows registrations and in-flight
poll snapshots retain the socket owner, and cancellation removes registrations
without allowing descriptor reuse during polling. Files, connects, DNS and
standard input still use helpers; helper retirement remains separate in #171.

The harness counts `readiness_threads` separately from `timer_threads` and
requires exactly one idle readiness service in Windows socket workloads, with
zero live tasks, socket handles, timers and readiness registrations. Correctness
tests cover a saturated one-helper pool, more than 64 simultaneous sockets,
readiness/deadline races, registration cancellation and partial writes.

The [three-host capture](https://github.com/maasdi/zore/actions/runs/40433590332)
completed eight builds, 96 fresh-process measurements and 192 create/release
cycles on each host. All source trees were clean. Runtime and harness manifest
hashes match recorded source/compiler revision
`334ef49efe16cbfbbbbcba7384e2a4bd96879246`, the GitHub merge tree for PR #203
at head `d550fdb0aa39948813743d7e7b535837e60cb9b8`. That tree matches the
implementation head. The Windows host was Server 2025 build 10.0.26100, AMD64
Family 25 Model 1, with Rust 1.98.1 and LLVM clang 20.1.8.

The before run is the Windows after-#169 archive from workflow `40294003728`.
Both runs use populations 32/128/512/640, zero and 4,096-int payloads, three
repetitions, two cycles and six-second idle checkpoints. At 640 sockets, helper
threads fell from 512 busy helpers to one idle helper; pending socket work uses
one readiness thread instead. Queued helper work fell from 170–385 jobs in
ordinary socket mode and 192–366 in deadline mode to zero. Socket handles were
640 in both runs. Each after-run registers 640 readiness waits and 640 socket
deadlines; the helper-based before run did not expose those waits as reactor
registrations or timers.

| Mode / state at 640 | Helpers, before / after | OS threads, before / after | Live ABI bytes, before / after (zero payload) | Live ABI bytes, before / after (4,096 ints) |
| --- | ---: | ---: | ---: | ---: |
| Sockets, active | 512 / 1 | 518 / 8 | 760,488 / 745,128 | 21,732,008 / 21,716,648 |
| Socket deadline, active | 512 / 1 | 515–518 / 5–8 | 770,728 / 755,368 | 21,742,248 / 21,726,888 |
| Sockets, six-second idle | 364–494 / 1 | 370–500 / 8 | 4,136 / 4,136 | 4,136 / 4,136 |
| Socket deadline, six-second idle | 494–510 / 1 | 497–513 / 5 | 4,136 / 4,136 | 4,136 / 4,136 |

The after-run helper is idle at these runtime snapshots. The separate readiness
service remains as one thread and its loopback wake connection after socket work
finishes. Every host's after-idle checkpoints returned to 4,136 live ABI bytes,
with zero tasks, queued work, socket handles, readiness registrations and live
timers. Socket modes did not start the separate Windows timer service. OS thread
samples and runtime counters are sequential observations; their transient
combinations are not one atomic scheduler snapshot.

RSS increments below are KiB over each cycle's baseline. Each cell is min /
median / max of the six per-cycle baseline-subtracted sample medians at 640.

| Mode / payload | Active RSS before | Active RSS after | Idle RSS before | Idle RSS after |
| --- | ---: | ---: | ---: | ---: |
| Sockets / zero | 1,636 / 7,222 / 12,004 | 1,312 / 1,902 / 2,400 | -1,324 / 5,134 / 10,328 | -76 / 646 / 1,184 |
| Sockets / 4,096 ints | 22,056 / 27,604 / 32,620 | 21,936 / 22,460 / 23,160 | -612 / 5,344 / 11,096 | 312 / 1,008 / 1,488 |
| Socket deadline / zero | 1,112 / 6,496 / 11,852 | 1,324 / 1,976 / 2,508 | 200 / 5,612 / 10,828 | 228 / 808 / 1,360 |
| Socket deadline / 4,096 ints | 21,648 / 27,198 / 32,496 | 21,864 / 22,432 / 23,096 | -200 / 5,642 / 11,012 | 412 / 1,084 / 1,880 |

The runs have matching workload configuration but different source revisions
and hosted allocations; the before Windows CPU was Family 25 Model 17.
These are resource observations, not throughput guarantees, isolated allocator
effects or a portable Windows/Unix comparison. The helper pool still serves
other blocking operations, so these socket results do not complete #171.

[Correctness CI](https://github.com/maasdi/zore/actions/runs/40433590322) passed
on Linux, macOS and Windows, including 98 Windows runtime tests. Linux's first
attempt hit an unrelated crypto-fixture compiler launch with `Text file busy`;
the Linux job passed on rerun without a code change. The Windows regressions
include writable readiness, exact resumed-write bytes, saturated-helper
independence and owner release before wake. Local runtime type/lint and test-hook
checks passed; executable validation used hosted toolchains because local MSVC
`link.exe` and clang are absent. PR #203 remains pending merge.

## Raw evidence

These gzip archives preserve the original JSON bytes from their workflow runs,
including every sample, cycle, result, source manifest and frame layout. SHA-256
values below refer to decompressed JSON.

| Report | Workflow artifact ID | Decompressed SHA-256 |
| --- | --- | --- |
| [Linux](../benchmarks/runtime-scaling/linux-2026-10-26.json.gz) | `12633131837` | `42a9f4c246972306cca789f120be04f5b775d309b2fa9b18ebfa964806664bcb` |
| [macOS](../benchmarks/runtime-scaling/macos-2026-10-26.json.gz) | `12632797264` | `7e0f0543ecb4ae83f998b07fce99bf7ae1971149e8d0490f50bbd926369236bb` |
| [Windows](../benchmarks/runtime-scaling/windows-2026-10-26.json.gz) | `12633084222` | `ae683b2a3a16e70fba8f33b9a27d371d9194f6f2fa721e5639bbbf0409a0fa0` |

The Windows timer after-run reports are from workflow run `40294003728`:

| Report | Workflow artifact ID | Decompressed SHA-256 |
| --- | --- | --- |
| [Linux after #169](../benchmarks/runtime-scaling/linux-2026-10-26-after-169.json.gz) | `12639956998` | `515e05d20073d94ad027c4c71c41e3cd70a6e2b1e86abb6dc63229cc1abb174c` |
| [macOS after #169](../benchmarks/runtime-scaling/macos-2026-10-26-after-169.json.gz) | `12640151882` | `2ac2b852bd903aae8270b0ba55a93f0947882bbff17a9b0c98468b8be70976bd` |
| [Windows after #169](../benchmarks/runtime-scaling/windows-2026-10-26-after-169.json.gz) | `12639224441` | `6a158ac8bcc9753f7d12f14fd188cd20469660910e49f8d9c2c79376c412445b` |

The Windows socket-readiness after-run reports are from workflow `40433590332`:

| Report | Workflow artifact ID | Decompressed SHA-256 |
| --- | --- | --- |
| [Linux after #170](../benchmarks/runtime-scaling/linux-2026-10-27-after-170.json.gz) | `12705215817` | `b5cc88cf2dc2d64418191cf313aad9bf1b7cc2609dca7ae7f76101e09f3e960c` |
| [macOS after #170](../benchmarks/runtime-scaling/macos-2026-10-27-after-170.json.gz) | `12705345587` | `0e53fd50d2ac61d1746c2256bea8f361bb6e0593aec5d0bca1d5c641189b1c57` |
| [Windows after #170](../benchmarks/runtime-scaling/windows-2026-10-27-after-170.json.gz) | `12704962555` | `05a9bf95d8ee8421c30e03dc848bd003496df90a451aafd7c54a90121073ce9e` |

For example, `gzip -dc benchmarks/runtime-scaling/linux-2026-10-26.json.gz`
recovers the original Linux JSON bytes from this capture. The historical
three-host report retains its original Windows archive.
