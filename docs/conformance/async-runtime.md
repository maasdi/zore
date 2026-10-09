# Q32 async runtime coverage inventory

Audit baseline: `main` at `0e640b4154e80eded053c227c46ef1521dfb3582`.
Current status (issue #75): the limits below were updated after the baseline.
Frames now place poll-local values outside the pinned frame and share
same-type slots, and `go` accepts closures and function values. The baseline
text is kept as the record it was, with notes where the status changed.
Authority: specification §§17–20, 35–36, and 37.3–37.4; the accepted
[Q32 proposal](../proposals/async-state-machines.md) explains the implementation
choices. **Covered** means the linked assertions cover the named cases, not all
programs. **Partial** means a required aspect lacks a direct assertion.
**Unsupported** is an accepted but unimplemented form. **Unverified** means
the evidence found does not assert the behavior. Markdown scenario lists in
`tests/conformance/` are not executable tests.

The native tests' [`prints`](../../tests/codegen/native.rs#L58) and
[`panics`](../../tests/codegen/native.rs#L189) helpers assert exit status,
stdout, and stderr. The Q32 parity helpers run the same source as a plain
function and as an `async func`, compare exact output, and inspect generated
IR: [channel/select](../../tests/codegen/native.rs#L5154),
[timer](../../tests/codegen/native.rs#L5459),
[mutex](../../tests/codegen/native.rs#L5650), and
[I/O](../../tests/codegen/native.rs#L5785). Runtime unit tests use explicit
queue, count, or completion assertions. These links identify the assertions
behind the rows below.

## Waiting operations

| Operation and status | Spec | Plain-call execution path | Async execution path | Executable tests for both paths | Ownership and cleanup evidence | Wakeup, error, or panic evidence | Explicit gap |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Async call / `await` — covered | §§17.3, 17.5–17.8, 35.1–35.2 | N/A: a plain function cannot use `await`; it spawns work and joins with `task.wait()`. | An awaited call polls a child heap frame; `Pending` saves a resume state and preserves locals. | [Lowering asserts child call, unwind edge, and span](../../tests/async/lowering.rs#L18); [native calls and tasks return results](../../tests/codegen/native.rs#L3058); [IR has explicit states and no poll-local `alloca`](../../tests/codegen/native.rs#L4810). | [Views remain valid across a pending child](../../tests/codegen/native.rs#L4840); [partial moves drop once](../../tests/codegen/native.rs#L4900). | [Nested panic cleanup and join propagation](../../tests/codegen/native.rs#L4922); [recursive calls and nil task](../../tests/codegen/native.rs#L4981). | No gap in these identified call/await cases; frame shrinking remains an accepted limit below. |
| Task join — covered | §§18.2–18.3, 18.9–18.10 | [`task.wait()`](../../runtime/src/task.rs#L210) parks a slot and compensates a blocked pool worker. | [`await task` poll](../../runtime/src/task.rs#L132) registers a waker and returns `Pending`. | [Plain results and errors](../../tests/codegen/native.rs#L2938); [async await results](../../tests/codegen/native.rs#L3058); [plain task awaits a polled child](../../tests/codegen/native.rs#L5023). | [Move result transfers to caller](../../tests/codegen/native.rs#L2965); [async error propagation and Move return](../../tests/codegen/native.rs#L4950). | [Wake before pending through existing handle](../../runtime/src/task.rs#L320); [blocking join compensation](../../runtime/src/task.rs#L336); [task panic re-raised at wait](../../tests/codegen/native.rs#L2989). | No gap in the identified join paths; spawned-input restrictions are listed below. |
| Channel send / receive — covered | §§19.4–19.9, 17.3 | [`zore_channel_send` and `receive`](../../runtime/src/channel.rs#L300) park a slot when needed. | [`zore_channel_start` / `poll`](../../runtime/src/channel.rs#L628) retain a waker and suspend the task. | [Parity asserts exact plain/async loop output and poll calls](../../tests/codegen/native.rs#L5231); [mixed plain/async waiters](../../tests/codegen/native.rs#L5409). | [Move messages drop once](../../tests/codegen/native.rs#L3232); [partial moves and borrows survive a pending receive](../../tests/codegen/native.rs#L5326). | [Close wakes pending send and receive](../../runtime/src/channel.rs#L939); [closed send drops unsent value and unwinds](../../tests/codegen/native.rs#L5366). | No gap in these identified queue, close, and cleanup cases. |
| `select` — covered | §19.14 | [`zore_select`](../../runtime/src/channel.rs#L467) parks the caller on pending cases. | The channel start/poll path retains registrations until one case claims the result. | [Parity checks ready/default/zero cases](../../tests/codegen/native.rs#L5262); [pending duplicate cases evaluate once](../../tests/codegen/native.rs#L5292). | [Unchosen Move values drop in reverse order](../../tests/codegen/native.rs#L5388). | [Concurrent winners claim one waiter](../../runtime/src/channel.rs#L750); [pending select deadlocks are detected](../../tests/codegen/native.rs#L5352); [closed send panics and cleans up](../../tests/codegen/native.rs#L5366). | No gap in these identified cases; exhaustive fairness across every selection pattern is unverified. |
| Timer / `time.Sleep` — covered | §§17.3, 37.3 | [`reactor::sleep`](../../runtime/src/reactor.rs#L649) waits on a slot. | `start_sleep` registers a waker; [`zore_reactor_poll`](../../runtime/src/reactor.rs#L37) resumes the frame. | [Parity checks positive, zero, and negative durations, exact output, and poll IR](../../tests/codegen/native.rs#L5491); [many sleeping tasks and plain helpers make progress](../../tests/codegen/native.rs#L5599). | [Timer resumes a frame holding views and Move values](../../tests/codegen/native.rs#L5511); [panic after timer resumes drops once](../../tests/codegen/native.rs#L5580). | [Timer completion and nonpositive durations](../../runtime/src/reactor.rs#L714); [timer wait is external for deadlock detection](../../tests/codegen/native.rs#L4001). | No gap in these identified sleep cases; timer metadata growth is tracked separately below. |
| Mutex acquisition — covered | §§20.2, 17.3 | [`zore_mutex_lock`](../../runtime/src/mutex.rs#L209) parks a slot on contention. | [`zore_mutex_start` / `poll`](../../runtime/src/mutex.rs#L89) register a task waker. The callback remains a plain body and can block its worker. | [Parity checks result and poll IR](../../tests/codegen/native.rs#L5678); [repeated contention reaches 160 updates](../../tests/codegen/native.rs#L5707). | [Move callback result](../../tests/codegen/native.rs#L5678); [captured marker drops once with error result](../../tests/codegen/native.rs#L5761). | [FIFO grants across slot and waker waiters](../../runtime/src/mutex.rs#L274); [poison wakes queued polls](../../runtime/src/mutex.rs#L411); [callback panic and zero mutex](../../tests/codegen/native.rs#L5731). | No gap in these identified acquisition cases; callback blocking is intentional. |
| Socket accept / read / write — partial | §§37.3, 17.3, 36.1 | [`net` calls](../../runtime/src/net.rs#L144) wait on descriptor readiness and block the calling thread, with worker compensation. | [`net_poll` start/poll](../../runtime/src/net_poll.rs#L306) saves the operation and wakes on readiness or deadline. | [Parity echoes split characters through Accept/Read/Write](../../tests/codegen/native.rs#L5845); [parity checks Accept timeout and byte I/O](../../tests/codegen/native.rs#L5946). | [Dropping a connection closes it](../../tests/codegen/native.rs#L3809); [byte-result ownership in parity case](../../tests/codegen/native.rs#L5946). | [Readiness/deadline race wakes once](../../runtime/src/reactor.rs#L832); [timeout keeps connection usable](../../tests/codegen/native.rs#L4655). | No direct test asserts that a pending async socket operation frees the only pool worker for unrelated work. |
| File / stdin helper completion — covered | §§37.3, 17.3, 36.1 | [`blocking::run`](../../runtime/src/blocking.rs#L77) waits on a helper result, compensating a blocked worker. | [`zore_blocking_start` / `poll`](../../runtime/src/blocking.rs#L106) wake the task after the helper publishes results. | [Parity checks file and byte results](../../tests/codegen/native.rs#L5820); [stdin parity checks invalid UTF-8 and EOF](../../tests/codegen/native.rs#L5888). | [`?` cleans the frame once on file error](../../tests/codegen/native.rs#L5914); [helper completion followed by panic drops frame once](../../tests/codegen/native.rs#L6011). | [Helper frees the only worker, result published before wake](../../runtime/src/blocking.rs#L172); [completion before Pending is retained](../../runtime/src/blocking.rs#L204); [helper panic follows task](../../runtime/src/blocking.rs#L255). | No gap in these identified helper completion cases. |
| Cancellation-token waits — partial | §37.4, §§17.3, 19.14 | `Token.Sleep` and `Done().receive()` use the ordinary timer/channel waiting paths in bundled Zore source. | The same wrappers suspend async callers through internal poll frames; cancellation is cooperative. | [Plain token test checks repeat Cancel, child propagation, Done, and Sleep](../../tests/codegen/native.rs#L4608); [plain/async parity checks timeout makes Sleep false](../../tests/codegen/native.rs#L5994). | [1000 child tasks finish through async frames](../../tests/codegen/native.rs#L6036). | [Cancelled Sleep returns false; timeout marks token cancelled](../../tests/codegen/native.rs#L5994). | No direct plain/async parity assertion for `Done()` inside a pending `select`, followed by cancellation from another task. |

An ordinary waiting call in an `async func` is a suspension point without
`await`; only an async call or task value uses `await`. A plain user helper and
a closure callback retain their plain execution semantics even when called
from an async function. The [lowering test](../../tests/async/lowering.rs#L216)
finds two wrapper suspensions but keeps the nested plain helper ordinary;
the [mutex lowering test](../../tests/async/lowering.rs#L189) finds acquisition
but not the blocking callback as a suspension.

## Scheduler and lifecycle checks

| Concern and status | Executable assertion | Explicit gap or limit |
| --- | --- | --- |
| Wake before Pending / park — covered | [Scheduler coalesces ten early wakes per poll](../../runtime/src/scheduler.rs#L452); [slot latches concurrent wake versus park](../../runtime/src/slot.rs#L68); [helper completes before Pending without resubmission](../../runtime/src/blocking.rs#L204). | No gap in these identified races. |
| Concurrent wake coalescing — covered | [1000 tasks under racing wakes poll exactly twice each](../../runtime/src/scheduler.rs#L750); [one select winner removes duplicate registrations](../../runtime/src/channel.rs#L874). | No gap in these identified races. |
| Prevent concurrent polling — covered | [8000 wakes while a poll is held keep its count at one; it runs once more after release](../../runtime/src/scheduler.rs#L470). | No gap in this direct race. |
| Worker compensation — covered | [All base workers can block while a replacement releases them](../../runtime/src/scheduler.rs#L666); [nested guards compensate once](../../runtime/src/scheduler.rs#L733); [64 short blocks reuse one surplus worker](../../runtime/src/scheduler.rs#L701). | Uninstrumented foreign blocking calls cannot trigger compensation; see [architecture](../architecture.md). |
| Internal versus external deadlock accounting — covered | [Pending internal task counted; external task uncounted](../../runtime/src/scheduler.rs#L536); [internal-only deadlock exits 2](../../runtime/src/scheduler.rs#L787); [external wake prevents false deadlock](../../runtime/src/scheduler.rs#L813). | Deadlock in only a subset of tasks is not reported while another task can run. |
| Task-local panic state — covered | [Panic state survives Pending without leaking into another poll](../../runtime/src/scheduler.rs#L561); [it follows a task to a replacement worker](../../runtime/src/scheduler.rs#L591); [task panic is raised at join](../../tests/codegen/native.rs#L2989). | No gap in these identified transfer cases. |
| Detach — covered | [Dropping a waker leaves pending poll work retained](../../runtime/src/scheduler.rs#L503); [detached task panic leaves main successful](../../tests/codegen/native.rs#L3018); [async detached result drops after completion](../../tests/codegen/native.rs#L5104). | Dropping a handle does not cancel work. |
| Process-exit abandonment — partial | [An endless detached task does not keep the process alive](../../tests/codegen/native.rs#L3034). | No executable assertion checks that pending destructors and buffered-value cleanup are skipped at exit, as §18.11 specifies. |

## Accepted limits and resource-gap history

- **No preemption:** a running plain function or poll body runs until it yields
  or returns. Q32 adds cooperative suspension, not forced interruption.
- **Frame storage (current status):** values needed across a suspension stay in
  the pinned heap frame. Other locals and drop flags use poll-local storage, and
  same-type Copy locals without cleanup, views, or address-taking may share one
  frame field when liveness proves no overlap. Parameters, captures, results,
  drop flags, and hoisted scratch fields are never shared. Frames are not shrunk
  by general liveness. [Poll-local tests](../../tests/async/lowering.rs) and
  [IR tests](../../tests/codegen/native.rs#L4810) assert these rules.
- **Conservative task inputs:** borrowed or `mut` inputs whose backing may
  outlive the spawner are rejected. The [spawn-input test](../../tests/typecheck/check.rs#L3222)
  asserts these diagnostics; scoped tasks and a broader lifetime proof are
  outside current Q32 behavior.
- **`go` function values (current status):** `go` accepts a declared function or
  method, a closure literal, a function-typed local, and a function value
  received through an `own` argument, with owning-closure rules. A callee stored
  in a field, element, map value, or call result is still rejected, and a
  function value received through a parameter cannot be spawned.
  [Spawn tests](../../tests/typecheck/check.rs) cover these rules.
- **Process-exit abandonment:** §18.11 allows the process to stop with running
  tasks. Suspended frames are not destroyed at exit, as stated by the accepted
  [Q32 proposal](../proposals/async-state-machines.md); cancellation tokens do
  not implicitly stop tasks.
- **#45 is fixed on this baseline:** [PR #58](https://github.com/maasdi/zore/pull/58)
  added `State::compact_timers` with a `max(64, 2 × live waiters)` trigger; the
  [regression](../../runtime/src/reactor.rs#L539) holds earlier live deadlines
  while completing 1000 later registrations and asserts retained entries stay
  within 64, then checks live deadlines survive. This is a bound on metadata
  growth in that scenario, not proof of constant memory for every workload.
- **#46 is fixed on this baseline:** [PR #59](https://github.com/maasdi/zore/pull/59)
  added a 250 ms idle grace for surplus workers. The
  [burst test](../../runtime/src/scheduler.rs#L701) asserts two starts for 64
  blocking sections; the [retirement test](../../runtime/src/scheduler.rs#L666)
  checks eventual return to base capacity and queued-task progress. Spares
  temporarily consume threads by design.

## Validation

Run on 2026-10-08 from base commit
`0e640b4154e80eded053c227c46ef1521dfb3582`, with this document as the
only working-tree change. Native tests used the repository's pinned Rust
toolchain and clang with LLVM 15 or newer. Every command exited successfully:

| Command | Result |
| --- | --- |
| `cargo test --locked --test async_lowering` | 7 passed, 0 failed |
| `cargo test --locked -p zore-runtime` | 66 passed, 0 failed; 0 doc tests |
| `cargo test --locked --test native` | 209 passed, 0 failed |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo build --locked` | Passed |
| `cargo test --locked --all-targets` | Passed, including 7 async lowering, 209 native, and 66 runtime tests |

## Focused missing-test candidates

1. In `tests/codegen/native.rs`, hold an async socket `Accept` pending while a separate task runs; expect the other task to complete before any connection arrives.
2. In `tests/codegen/native.rs`, wait on `Token.Done()` in an async `select` and cancel from another task; expect exactly one case to run and no blocked task to remain.
3. In `tests/codegen/native.rs`, call a plain channel-waiting helper directly from an async function while all base workers do likewise; expect replacement workers to run queued senders and let every helper return.
4. In `runtime/src/scheduler.rs`, race detach, a final wake, and task completion; expect one cleanup and no extra poll.
