# Q32 proposal: async functions as state machines, without fibers

Status: PROPOSED. Nothing here is authorized for implementation until the
maintainer accepts it. On acceptance the spec sections named below are updated
first (§53), then the work lands in the slices at the end.

## Decision this proposal carries out

The maintainer chose pure state machines and the removal of fibers. The spec
already points that way: §35.1 says async functions lower to compiler-generated
state machines, and §17.7 says they are expected to. Today's code uses stackful
fibers instead (`runtime/src/fiber.rs`), which `docs/architecture.md` and
`compiler-structure.md` §17 list as an approved deviation. This proposal ends
that deviation: the guide's `async_lowering/` stage gets built and fibers go.

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
   starve the others. A program with tens of thousands of tasks should write them
   as `async func`; plain-function tasks cost about a thread each.
5. **Existing programs keep compiling.** Task functions that are plain `func`s
   still work and run on pool threads. Rewriting them as `async func` is how to
   get the small-task scaling.

Spec text this changes: §17.7 and §35.1 (from "expected" to the locked design),
§18.3 and §18.8 (what `go` runs), §19 and §37.3 (the sentence that a waiting
call "suspends only the calling task" becomes "in an async function; in a plain
function it blocks the thread"), and §36.1 (the scheduler is a poller).

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
  awaited async call, `await task`, or a waiting operation (above) in an async
  body. Ownership, region, and drop analysis treat it like any other call, so
  the existing checks, drop elaboration, and unwind edges apply unchanged.
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
   the pool, `fiber.rs` goes, tests that rely on tens of thousands of tasks become
   async, and the fallback thread-per-task path is retired.
6. **Docs and guide.** Remove the approved deviation, update `architecture.md`,
   and add `async_lowering/` to the layout.

## Risks and open questions

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
- **Targets without the pool.** The pool needs only threads, so the Windows
  fallback no longer needs a separate thread-per-task design.

## Defaults chosen here, for the maintainer to veto

- Waiting operations pause without `await` (smallest language change, keeps
  §37.3 almost as written).
- `go` keeps accepting plain functions.
- Frames are not shrunk by liveness in the first version.
- Suspended frames are not destroyed when a program ends.
