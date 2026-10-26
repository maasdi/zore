# Runtime population recapture — 2026-10-26

Issue [#177](https://github.com/maasdi/zore/issues/177) refreshes Linux and macOS
evidence after the harness maintenance. Windows recapture remains deferred until
a suitable machine is available. Both hosts completed all eight builds, 96
fresh-process measurements and 192 create/release cycles. Every after-idle
checkpoint returned to 4,136 live ABI bytes with no live tasks, sockets, timers
or queued work. The [2026-10-24 report](runtime-scaling.md) and
its original raw files remain available as historical evidence; its Windows
measurements are not a current cross-host comparison.

## Identity and reproduction

The captures use the opt-in
[measurement workflow run](https://github.com/maasdi/zore/actions/runs/40247170943).
The PR head is `ff0e68ed87958df9630c79f7bdd4077433cbb12d`, based on main
`a472e1d2830f83b47b7c7803efc8cf856d7f31c5`. The initial PR commit changes only
workflow host selection and the harness guide. Each runner builds its own
release compiler from the clean PR merge checkout; the report's source/compiler
revision identifies that checkout, rather than an externally supplied binary.
The measured merge revision is `dd6b6c6de806acdc22152bbdcf634e8edf3d4af2` on
both hosts; its tree matches that PR head. Both reports record a clean checkout,
and their runtime/harness manifests match each other and the recorded commit.
Later report publication changes documentation and adds raw data.

| Identity | Linux | macOS |
| --- | --- | --- |
| OS | Linux 6.17.0-1022-azure, glibc 2.39 | macOS 26.6.2 |
| Architecture / reported CPUs | x86_64 / 4 | arm64 / 3 |
| Reported physical memory, bytes | 16,766,414,848 | 7,516,192,768 |
| Open-file soft / hard limit | 65,536 / 65,536 | 10,240 / 9,223,372,036,854,775,807 |
| Process soft / hard limit | 63,838 / 63,838 | 1,333 / 2,000 |
| Rust host | x86_64-unknown-linux-gnu | aarch64-apple-darwin |
| clang | Ubuntu 18.1.3 (1ubuntu1) | Apple 21.0.0 (clang-2100.1.1.101) |
| Compiler executable SHA-256 | `23ad264dd1819515d1702a0050cde978b852e9c2e30cac8b376b3d6050f03d9d` | `6cabfb00c0a3c68614540e13fd25b6c7c88f91294d83ff6f5b439ec2057bb2d2` |

Both hosts used Rust 1.98.1, commit
`48a229ceaefd4985c50990b14116b6d856af0985`, LLVM 22.1.8, Python 3.13.16 and
psutil 7.2.2. Full identities and per-variant workload, IR and executable hashes
are preserved in the raw reports. The Linux processor string identifies only
`x86_64`; no CPU model is inferred.

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
its deadline is 120 seconds. Socket deadlines are 120 seconds for successful
reads and five seconds for expiry, while context timers are explicitly cancelled
before their one-hour deadlines.

Each report contains exactly the expected 96 distinct mode/payload/population/
repetition combinations and eight ordered checkpoints per process. Every build
and process succeeded, with exit code zero, empty stderr and no harness-forced
termination. All recorded source hashes were checked against the measured
commit. Linux checkpoints contain seven OS samples, spanning 301.306–305.641 ms;
macOS checkpoints contain three to seven, spanning 299.968–505.977 ms. Sampling
cost and scheduling affect those observed spans; they are not timing guarantees.

## Populations and pinned storage

All four populations completed in every mode and payload variant on both hosts.
No experimental failure point was reached at 640. This is not a service-capacity
promise for arbitrary workloads.

The following padded LLVM ABI sizes and active counts at population 640 match
on both hosts and in both payload variants. Allocation increments use the
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

Both hosts retain two scheduler workers. At population 640, timers register
640 cancellable deadlines and sockets register 640 reads with deadlines; none
uses per-deadline timer threads or queued socket helpers. Timer active snapshots
can still include runnable context-watcher work: the zero-payload population-640
queued-task counts range from 0–145 on Linux and 217–361 on macOS. These transient
queues are empty at every after-idle checkpoint.

## Cleanup and RSS

All 192 after-idle checkpoints per host have:

- 4,136 live ABI bytes, two scheduler workers and one helper thread;
- zero tasks, queued tasks/helpers, blocked workers, socket handles, readiness
  registrations, live timers, stale timer records, timer threads, channels or mutexes;
- one control-scanner text buffer, five OS threads and six descriptors.

Every process has exactly zero settled cycle-two minus cycle-one live ABI bytes.
Every settled runtime counter also matches between cycles except the idle-helper
count in one macOS socket process: it changes from one to zero while the total
helper count remains one. The control scanner uses helper work, and these
snapshots are not atomic observations of the whole process. Linux records one
idle helper at every settled checkpoint; macOS otherwise does too.

The table shows RSS increments above the same cycle's baseline at population
640 with a 4,096-int payload, in KiB. Each cell is min / median / max of six
checkpoint medians, preserving three processes and both cycles. These descriptive
statistics are not confidence intervals or performance thresholds.

| Host / mode | Active RSS increment, KiB | After-idle RSS increment, KiB |
| --- | ---: | ---: |
| Linux tasks | 16,744 / 20,722 / 23,072 | 1,092 / 5,484 / 6,716 |
| Linux timers | 19,824 / 23,980 / 27,416 | 1,768 / 5,142 / 7,176 |
| Linux sockets | 1,148 / 12,360 / 22,420 | -596 / 10,576 / 21,012 |
| Linux socket deadline | 2,676 / 14,186 / 22,492 | 16 / 8,338 / 19,608 |
| macOS tasks | 96 / 20,008 / 40,368 | 96 / 20,056 / 40,528 |
| macOS timers | 144 / 20,288 / 39,312 | 144 / 21,088 / 40,816 |
| macOS sockets | 32 / 11,360 / 23,056 | 32 / 11,384 / 23,104 |
| macOS socket deadline | 16 / 11,328 / 22,672 | 16 / 11,360 / 22,704 |

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

Issue #177 remains open for the deferred Windows recapture. Windows timing,
socket readiness and helper-retirement implementation work remains deferred in
[#169](https://github.com/maasdi/zore/issues/169),
[#170](https://github.com/maasdi/zore/issues/170) and
[#171](https://github.com/maasdi/zore/issues/171). The measurement workflow defaults
to Linux/macOS and offers an explicit `include_windows` manual input; ordinary
Windows correctness CI remains enabled.

## Raw evidence

These gzip archives preserve the original JSON bytes from workflow run
`40247170943`, including every sample, cycle, result, source manifest and frame
layout. Both downloaded ZIPs were checked against their workflow artifact
digests. SHA-256 values below refer to decompressed JSON.

| Report | Workflow artifact ID | Decompressed SHA-256 |
| --- | --- | --- |
| [Linux](../benchmarks/runtime-scaling/linux-2026-10-26.json.gz) | `12619604003` | `630bef5a1def27046453fa445f018e38e08ce93f32b3b713c58bdb6d4b886f3b` |
| [macOS](../benchmarks/runtime-scaling/macos-2026-10-26.json.gz) | `12619993223` | `d3184012cc32917d74b662f8f79e4407c5964086701867f8878d90ccbd3392d3` |

For example, `gzip -dc benchmarks/runtime-scaling/linux-2026-10-26.json.gz`
recovers the unchanged Linux report. The historical Windows archive remains
linked only from the historical report above.
