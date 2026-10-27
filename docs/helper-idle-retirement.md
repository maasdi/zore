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

Local runtime checks and strict Clippy passed in normal and test-hook builds.
The Rust probe typechecks, and Python protocol/invariant/error-cleanup checks
passed. Native probe execution is skipped locally because MSVC `link.exe` is
absent. Hosted correctness CI and paired three-host lifecycle capture are
pending; this report does not yet claim an observed retirement improvement.
