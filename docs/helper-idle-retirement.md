# Helper idle retirement

Issue [#171](https://github.com/maasdi/zore/issues/171) separates helper-pool
lifecycle from the shared timer and socket services delivered in #169 and #170.
The earlier Windows socket capture retained hundreds of idle helpers after six
seconds. That establishes retention at that checkpoint, not an unbounded leak
or an isolated explanation for RSS.

## Policy and regression coverage

The helper pool keeps its 512-thread limit and five-second idle policy. Each
worker now retains one monotonic idle deadline across wakeups that provide no
job. Completing actual work establishes a fresh grace period; a queued job is
always checked before expiry, so retirement cannot strand admitted work.
No source API, ownership rule or helper queue limit changes.

The previous relative wait restarted the full grace period after every empty
return from the condition variable. Rust documents both
[spurious returns and the need to adjust relative timeouts in a loop](https://doc.rust-lang.org/stable/std/sync/struct.Condvar.html#method.wait_timeout).
Windows also permits
[spurious and stolen wakeups](https://learn.microsoft.com/en-us/windows/win32/sync/condition-variables).
That makes indefinite rearming a concrete policy weakness, but does not prove
it caused the historical socket-retention observations.

Subprocess-isolated capacity-one tests advance a controlled monotonic clock
under the pool lock. Wait-generation checkpoints establish that each wake was
observed before the next advance. They cover empty wakes through the original
deadline, fresh grace after actual work, two jobs queued exactly at expiry,
nested blocking work, retirement and restart, and retained-payload cleanup.
These are state assertions with watchdogs, not CI performance thresholds.

Test-only counters distinguish running helper jobs, idle helpers, queued jobs,
successful thread starts, retirement decisions and empty wake returns. The
instrumentation baseline is commit `153dfc7d24b03be844c174f55b5a272209248b5c`,
which preserves the old relative wait policy. Production builds do not collect
these counters or expose the benchmark gates.

## Paired measurement design

The [component harness](../benchmarks/helper-lifecycle/README.md) compiles exact
before/after runtime revisions with identical instrumentation and one Rust probe.
Each fresh process submits gated real helper jobs, verifies nested inline work,
and performs short-gap reuse before longer idle checkpoints. Main-thread Rust
standard input avoids the permanently occupied control helper in the older
compiler-generated socket workload. There are no generated Zore frames or
payloads in this synthetic component test.

Natural bursts use populations 32, 512 and 640, two repetitions and four cycles.
Checkpoints occur at elapsed offsets 1, 6, 15, 30 and 60 seconds after completed
reuse. A separate 32-job control deliberately wakes idle workers for ten seconds;
it tests empty-wake resilience without claiming to reproduce Windows internals.
Paired ordering alternates on each host. Raw source manifests, executable hashes,
all observations and failed experiments remain recorded.

Logical retirement occurs slightly before an OS thread disappears. Runtime
counters precede OS sampling and are not atomic with RSS or thread counts.
Rust containers, allocator caching and stack retention are outside live Zore ABI
allocations. The five-second internal policy is not a guarantee that the OS
will finish destroying every helper thread at a particular wall-clock instant.

## Validation status

[Correctness CI](https://github.com/maasdi/zore/actions/runs/40620397025) passed on
Linux, macOS and Windows at head `1b4f154`, including the native two-job restart
probe, strict Clippy, formatting, builds, documentation and full test suites.
Local formatting, runtime/probe typechecking, seven Python harness checks and
imports passed; the local native smoke test was skipped. Local Cargo build,
Clippy and tests were blocked by the missing MSVC linker.
Native execution used hosted toolchains because local MSVC `link.exe` is absent.
The harness explicitly selects the native MSVC linker when the developer-tools
directory is present, avoiding Git Bash's unrelated executable of the same name.

## Paired capture — 2026-10-27

[Run 40462446032](https://github.com/maasdi/zore/actions/runs/40462446032) passed
on all three hosts. Each host completed two runtime/probe builds, 14 successful
fresh-process trials, 52 cycles and 104 gated bursts. Every child exited with
status zero and empty stderr, without harness termination. Source trees were
clean. All runtime and harness manifest hashes match their recorded commits.

The before runtime is `153dfc7d24b03be844c174f55b5a272209248b5c`; the after
runtime and harness are `e1c0b13145b02d53f2432ed99de965225c2bb948`, the PR #204
merge tree at head `60fd0cb74ec32616e7ff60b3855e374573377606`. Rust 1.98.1 and
psutil 7.2.2 were used on Linux x86_64, macOS arm64 and Windows x64 MSVC. The
Windows host was Server 2025 build 10.0.26100, AMD64 Family 25 Model 1, four CPUs.
These are paired component results on each host, not comparisons of application
throughput across hosts.

At 640 submitted jobs, both policies admitted 512 active helper jobs and kept
128 queued; every job completed its nested inline work. At every quiet
checkpoint, active jobs and queued jobs were zero. Tracked Zore ABI bytes,
tasks, socket handles, timers and readiness registrations were zero throughout
this synthetic workload. Lifecycle balances held at every checkpoint.

### Natural bursts

Linux and macOS recorded zero helpers at the requested six-second checkpoint
and all later checkpoints with both policies, at all three populations.
Windows exposed a longer tail with the relative-wait policy. Each range below
covers eight cycles: two fresh-process repetitions with four cycles each.

| Windows submitted jobs | Helpers at 6 s, before / after | At 15 s | At 30 s | At 60 s |
| --- | ---: | ---: | ---: | ---: |
| 32 | 17–27 / 0 | 6–19 / 0 | 0–5 / 0 | 0 / 0 |
| 512 | 512 / 0 | 280–512 / 0 | 70–300 / 0 | 0–36 / 0 |
| 640 | 368–512 / 0 | 145–512 / 0 | 29–323 / 0 | 0–107 / 0 |

The after policy recorded zero helpers at every six-second and later checkpoint
on every host. These are checkpoint observations, not an exact OS retirement
deadline. Offsets are measured from the parent's receipt of completion
acknowledgement. Windows observations sometimes ran late: the before 640-job
six-second checkpoints were observed at 6.000–12.692 seconds, whereas after
observations were at 6.000–6.001 seconds. The reports preserve every requested
offset, observed offset and OS-sample timestamp.

For 640-job Windows bursts, OS thread samples ranged from 302–518 before to
3–6 after at the requested six-second checkpoint, and 3–110 before to 3 after
at 60 seconds. The process baseline includes two scheduler workers and the main
thread; other OS activity and thread-exit lag are not identity-mapped to helpers.
Unix snapshots also initialize the existing reactor, producing a four-thread
idle baseline.

### Empty-notification control

This separate control issues twenty notifications over ten seconds in each
cycle. Both policies recorded 32 helpers at one second. At six and fifteen
seconds, every host's before run still recorded 32 helpers; every after run
recorded zero. Linux/macOS before runs reached zero by 30 seconds. Windows
before runs recorded 4–13 helpers at 30 seconds and zero at 60 seconds.

This directly exercises the relative-timeout rearming weakness with known empty
notifications. Natural Windows trials also recorded very large empty-wake
counts with both policies. Their underlying cause was not isolated, and this
experiment does not establish that the patch introduced a wake storm or that
one mechanism explains every historical socket observation.

### Reuse and sampling limits

Every Linux/macOS reuse burst started zero new helpers. Windows 32-job trials
also reused their helpers with both policies. High-capacity Windows observations
need a timing qualification: the harness sampled OS resources between completion
and the next request. Nominal 0.2-second sample windows sometimes had samples
about 4.4 seconds apart, consuming the five-second grace period.

Across the sixteen after cycles at 512/640 jobs, twelve recreated helpers and
had completion-to-reuse-response intervals of 5.030–5.096 seconds; the other four
started no helpers and had intervals of 0.599–4.698 seconds. Before runs reused
helpers across intervals of 4.470–12.547 seconds, consistent with prolonged
retention. Command-send times were not recorded in this capture, so these gaps
cannot be attributed exclusively to runtime response time. The configuration's
short reuse gap does not establish actual immediate reuse in those trials.

The deterministic clock test verifies that work admitted before expiry reuses
the worker and renews grace. A bounded follow-up now requests release and restart
inside the probe, with no OS sampling or parent round trip between them. It
records probe release, whole-burst drained, submission and all-active times,
plus parent send/receive times. Earlier workers can use part of their grace
while the rest of the burst drains; these are batch times, not individual
workers' idle origins.

[Follow-up run 40620433793](https://github.com/maasdi/zore/actions/runs/40620433793)
passed on Linux, macOS and Windows on 2026-10-28. Each host completed 12 fresh
processes, 48 restart observations and 60 bursts at populations 32, 512 and 640.
Both policies recorded zero new helper starts in every restart. All children
exited successfully with empty stderr, without harness termination; source trees
were clean and lifecycle/resource invariants held. This confirms immediate
batch reuse in these trials without making a universal contention guarantee.

The runtime baseline remains `153dfc7d24b03be844c174f55b5a272209248b5c`.
The after runtime and harness are `440e131995dabebf0e97a954482320a22a7b1061`,
the PR merge tree for head `1b4f1542c37c9e232a24f8d47601806b9c9e7695`.
Across both policies and all populations, release-to-drained intervals ranged
from 0.141 to 3.808 ms on Linux, 0.170 to 31.632 ms on macOS, and 0.143 to
9.595 ms on Windows. Drained-to-submission boundaries were at most 0.000208 ms;
submission-to-all-active intervals were at most 11.221 ms. No OS sampling
occurred within these restart intervals.

### RSS observations

Values below are KiB over each cycle's baseline, reported as min / median / max
of eight differences of OS-sample medians at 640 submitted jobs. A cycle can
begin with retained resources from the previous cycle, especially in the before
Windows run. These baseline differences do not isolate allocator or stack costs.

| Host / requested idle point | RSS increment before | RSS increment after |
| --- | ---: | ---: |
| Linux / 6 s | 4 / 28 / 1,328 | 4 / 26 / 1,324 |
| Linux / 60 s | 4 / 28 / 1,328 | 4 / 26 / 1,324 |
| macOS / 6 s | 0 / 24 / 1,392 | 0 / 16 / 1,408 |
| macOS / 60 s | 0 / 24 / 1,408 | 0 / 16 / 1,424 |
| Windows / 6 s | 6,124 / 8,286 / 9,368 | -184 / -8 / 444 |
| Windows / 60 s | -2,080 / 506 / 2,504 | -184 / -8 / 400 |

Flat or elevated RSS after jobs and helpers retire remains a process-level
retention observation. Four cycles and a 60-second tail do not establish a
whole-process leak bound or a portable memory/performance guarantee.

## Raw evidence

These gzip archives preserve the original artifact JSON bytes, including both
runtime variants and every checkpoint. SHA-256 values refer to decompressed JSON.

| Host | Artifact ID | Decompressed SHA-256 |
| --- | --- | --- |
| [Linux](../benchmarks/helper-lifecycle/linux-2026-10-27.json.gz) | `12717411007` | `ceb321bd200202d7e4a3a65da390667f924f336eff821ba37e80bfb5660dc86e` |
| [macOS](../benchmarks/helper-lifecycle/macos-2026-10-27.json.gz) | `12716734475` | `66f924e84b0cd75bf29ec91d7f385e8f1805447b42b38ee8fbe43588cf2ae5ba` |
| [Windows](../benchmarks/helper-lifecycle/windows-2026-10-27.json.gz) | `12717546368` | `a378faf9a99ec8c9f3c46198d8ced37205f23a9dc04c5bf41a21713f26ccd9cf` |

The bounded reuse follow-up preserves the original JSON bytes separately:

| Host | Artifact ID | Decompressed SHA-256 |
| --- | --- | --- |
| [Linux reuse](../benchmarks/helper-lifecycle/linux-reuse-2026-10-28.json.gz) | `12786977434` | `a0e04b03e9fd2593a002f75c3be00477d46db3a9a3531db5d1236fa2e29d417b` |
| [macOS reuse](../benchmarks/helper-lifecycle/macos-reuse-2026-10-28.json.gz) | `12787181338` | `d4dd1ff99acefda41cb26a235681fbc8bd4346d68c11ca65f27f4f80581a25c8` |
| [Windows reuse](../benchmarks/helper-lifecycle/windows-reuse-2026-10-28.json.gz) | `12786997774` | `d016e9e9df1bf40ced4cd02f13382a80b890ae2f2dcec8a4774c8eb1714ef32c` |

Implementation and measurement validation are complete. PR #204 awaits review
and merge; #171 remains open until merge.
