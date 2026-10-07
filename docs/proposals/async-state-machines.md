# Q32 proposal: async functions as state machines, without fibers

Status: ACCEPTED and incorporated into spec §17.3, §17.7, §18.3, §20.2, §35.1,
and §37.3, with a small edit to §18.9. The specification is authoritative; this
document preserves the accepted proposal. The work lands in the slices at the
end, one pull request each; the runtime keeps using fibers until slice 5.

## Decisions this proposal carries out

The maintainer made two decisions:

1. **Pure state machines; fibers are removed.** The spec already points that way:
   §35.1 says async functions lower to compiler-generated state machines, and
   §17.7 says they are expected to. Today's code uses stackful fibers instead
   (`runtime/src/fiber.rs`), which `docs/architecture.md` and
   `compiler-structure.md` §17 list as an approved deviation. This proposal ends
   that deviation: the guide's `async_lowering/` stage gets built and fibers go.
2. **The source language does not get harder.** The spec says the source language
   "should remain relatively simple even when the compiler performs sophisticated
   … async … analysis". So the state machines stay inside the compiler. No new
   keyword, no new rule about where a call may appear, and no rewrite of existing
   programs is required. The alternatives that put more on the programmer are
   listed under "Alternatives considered".

## What a Zore programmer sees

Almost nothing changes in how programs are written.

1. **An `async func` becomes a state machine.** It owns a heap frame holding its
   locals, and it can pause and resume without a stack of its own.
2. **Waiting operations pause an async function without `await`.** Channel send and
   receive, `select`, `Mutex.withLock`, and the `zore/time`, `zore/io`,
   `zore/os`, `zore/net`, and `zore/cancel` functions are still ordinary calls
   (§19, §37.3). Inside an `async func` such a call pauses the task and frees its
   worker thread. `await` stays for calls to `async func`s and for `Task` values,
   as it is now.
3. **Inside a plain `func`, the same calls block the calling thread.** This is
   what `task.wait()` already does, and what `main` already relies on (§37.3:
   "in the initial task a wait blocks the entry point's thread"). One spelling
   works in both kinds of function.
4. **`go` still accepts any declared function.** `go asyncFn(x)` creates a polled
   task. `go plainFn(x)` runs the function on a pool thread, and the pool adds a
   thread whenever one of its threads blocks, so a blocked plain function cannot
   starve the others.
5. **Existing programs keep compiling.** The one visible difference is cost: a
   plain function used as a task takes about a thread while it waits, where today
   it takes a small fiber stack. A program with tens of thousands of tasks should
   mark its task functions `async`; the calls inside them stay as they are.

Spec text this changes: §17.7 and §35.1 (from "expected" to the locked design),
§18.3 and §18.8 (what `go` runs), §19 and §37.3 (the sentence that a waiting
call "suspends only the calling task" becomes: in an async function; in a plain
function it blocks the thread), and §36.1 (the scheduler is a poller).

## Execution model

- A task is a heap block holding its frame, a `poll` function, its result, and
  scheduling state. A pool of worker threads (at least two, one per core) pops
  runnable tasks and calls `poll`. `poll` returns `Ready` or `Pending`.
- `Pending` means the task registered a waker with whatever it waits on: a
  channel queue, a mutex queue, the reactor (timer or descriptor), the helper
  thread pool, or another task. The waker puts the task back on the run queue.
  A scheduling flag (idle, running, notified) closes the window between "poll
  returned" and "wake arrived".
- A thread that blocks inside a plain function, parked in `Slot`, tells the pool
  so it can start a replacement worker. This keeps the pool's capacity constant
  even when plain functions wait.
- The blocked counter behind deadlock detection counts a suspended task as blocked
  from the moment it returns `Pending` on an internal wait until its waker fires,
  exactly as it counts a parked fiber today. Timers, descriptors, and helper
  threads still do not count.
- Panic state belongs to the task and is installed on whichever thread polls it,
  replacing the per-fiber swap.

## Compiler design

A new stage `async_lowering/` (`state_machine.rs`, `suspension.rs`, `lower.rs`)
as the structure guide plans, running on MIR after drop insertion and before
code generation.

- **Frame.** Every MIR local of an async function, including temporaries and drop
  flags, lives in a heap frame that never moves. A pointer to a local stays valid
  across a pause. The first version does not overlap or shrink frames by
  liveness; that is a later optimization. Because the frame is pinned, the
  conservative borrow rule of §17.6 is already met for locals, and the ownership
  checker needs no new rule for them.
- **Suspension points.** MIR gets an explicit flag on a call that may pause: an
  awaited async call, `await task`, or a waiting operation in an async body.
  Ownership, region, and drop analysis treat it like any other call, so the
  existing checks, drop elaboration, and unwind edges apply unchanged.
- **Generated code.** For each async function: a frame type, a `poll(frame)`
  function whose first instruction dispatches on a stored state number to the
  resume point, and a constructor that moves the arguments into a new frame. At a
  suspension point the code starts the operation (or the callee's frame), polls it,
  and on `Pending` stores the state number and returns `Pending`; on resume it
  jumps back to the poll. `Ready` continues, with panics leaving through the
  call's existing unwind edge.
- **Calls.** `await f(x)` allocates `f`'s frame, polls it in a loop as above, reads
  its result, and frees the frame. Recursion works because each call has its own
  frame.
- **`go`.** `go asyncFn(x)` builds the frame and a task block around it and
  enqueues it. `go plainFn(x)` keeps today's owning-closure path and runs it as
  a pool job.

## Runtime design

Every primitive that waits today parks a `Slot`. Each gets a poll form beside its
blocking form, and both share the same queues:

- `channel.rs`: the waiting entry holds a waiter that is either a `Slot` or a task
  waker. Send, receive, and `select` gain `start` and `poll` functions that keep
  the in-progress entry in the frame. A leftover entry is removed on completion,
  as `select` does now.
- `mutex.rs`, `task.rs` (join), `reactor.rs` (timers, descriptor readiness),
  `blocking.rs` (helper completion): the same waiter swap.
- Natives behind `zore/*` get poll variants where they wait; the bodyless shim
  generator learns the shapes it needs.

`fiber.rs` and the stack-switching assembly are deleted at the end.

## Slices

Each slice is its own PR, keeps every test passing, and leaves the language
usable.

1. **Spec and docs.** Update the sections above and record Q32. No code.
2. **Scheduler and wakers.** A poll-based task type and run queue beside fibers;
   the waiter abstraction in the runtime primitives; worker compensation for
   blocked threads. Nothing uses it yet.
3. **State machines for simple async functions.** `async_lowering/`, frames,
   `poll`, `await` of async calls and tasks, `go` of async functions. Fibers
   still run everything else.
4. **Waiting operations in async functions**, in this order: channels and
   `select`, time and the reactor, `Mutex`, then I/O and the helper pool. One PR
   each, with tests that run the same program as plain and as async code.
5. **Plain-function tasks on pool threads; remove fibers.** `go plainFn` moves to
   the pool, `fiber.rs` goes, tests that rely on tens of thousands of plain-function
   tasks mark those functions `async`, and the thread-per-task fallback is retired.
6. **Docs and guide.** Remove the approved deviation, update `architecture.md`,
   and add `async_lowering/` to the layout.

## Alternatives considered

- **An `await` on every wait.** Writing `await ch.receive()` or `await select
  { ... }` inside async functions would make every pause visible in the source,
  at the cost of a second spelling for each waiting operation, a new rule that a
  plain function may not use the awaited form, and a rewrite of existing task
  code. Rejected for simplicity; it can be added later without changing the
  runtime design.
- **`go` only for `async func`.** Stricter, with no hidden threads, but it breaks
  every existing task function and removes a way to start background work.
  Rejected for the same reason.
- **The compiler works out which functions can pause.** Plain functions that wait
  would also become state machines, so no thread is ever tied up and no `async`
  marker is needed on waiting code. It is the simplest to use and the hardest to
  build, especially for function values and recursion. This design does not
  prevent it: it is the natural upgrade once slices 3 and 4 are done.

## Risks and open questions

- **Scaling of plain-function tasks.** Today a plain-function task is a small
  fiber stack, so tens of thousands of them fit. After fibers go, a plain-function task
  that waits holds a thread. The fix is to mark such functions `async`, a
  one-word change per function. Tests that create tens of thousands of tasks
  need it, and `docs/roadmap.md` and the tests guide must say so.
- **Frame size.** All locals in one frame can be large for functions with big
  fixed arrays. Liveness-based layout is the planned follow-up.
- **Abandoned frames.** A task that never finishes (a deadlock, or exit while
  tasks are suspended) leaks its frame, because dropping a suspended frame needs
  per-pause initialization information. This matches today, where abandoned
  fibers are never unwound. Cancellation (§18) would need that information and
  is out of scope.
- **Blocking inside async code.** Calling a plain function that waits from an
  async function blocks a worker thread. Compensation prevents starvation; a
  lint could flag it later.
- **Size of the change.** Slices 3 and 4 touch the code generator, the MIR, and
  every waiting primitive in the runtime. Slice 2 exists to keep slice 3 small.

## Defaults chosen here, for the maintainer to veto

- Frames are not shrunk by liveness in the first version.
- Suspended frames are not destroyed when a program ends.
- A pool thread that blocks in plain code is replaced so the pool keeps its
  capacity.
