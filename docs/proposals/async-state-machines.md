# Q32 proposal: async functions as state machines, without fibers

Status: PROPOSED. Nothing here is authorized for implementation until the
maintainer accepts it. On acceptance the spec sections named below are updated
first (§53), then the work lands in the slices at the end.

## Decisions this proposal carries out

The maintainer made three decisions:

1. **Pure state machines; fibers are removed.** The spec already points that way:
   §35.1 says async functions lower to compiler-generated state machines, and
   §17.7 says they are expected to. Today's code uses stackful fibers instead
   (`runtime/src/fiber.rs`), which `docs/architecture.md` and
   `compiler-structure.md` §17 list as an approved deviation. This proposal ends
   that deviation: the guide's `async_lowering/` stage gets built and fibers go.
2. **Every pause is written with `await`.** Waiting operations are awaited inside
   an `async func`, so each place a task can pause is visible in the source.
3. **`go` is stricter.** It accepts only a call to an `async func`.

## What a Zore programmer sees

1. **An `async func` becomes a state machine.** It owns a heap frame holding its
   locals, and it can pause and resume without a stack of its own.
2. **Waiting operations are awaited inside an `async func`.** These are channel
   `send` and `receive`, `select`, `Mutex.withLock`, `Task` results, and the
   waiting functions of `zore/time`, `zore/io`, `zore/os`, `zore/net`, and
   `zore/cancel`. They are written `await ch.receive()`, `await ch.send(v)`,
   `await time.Sleep(10)`, `await m.withLock(f)`, and `await select { ... }`.
   Leaving out the `await` in an async function is a compile error, the same way
   `.wait()` is already rejected there.
3. **Inside a plain `func` (including `main`) the same operations block the
   thread and are written without `await`,** exactly as `.wait()` does today.
   `await` stays invalid in a plain function (§17). The operations that do not
   wait, such as `close`, `isPoisoned`, `Cancel`, and `Done()`, are written the
   same everywhere.
4. **`go` takes only a call to an `async func` or `async` method.** `go plainFn(x)`
   is a compile error that says to make the function `async`. There are no hidden
   threads: a plain function runs on its caller's thread. Background CPU work is
   written as an `async func` that never awaits. `go` still returns a `Task`, and
   `await task` or `task.wait()` still retrieves the result.
5. **Existing programs need rewriting.** Task functions become `async func`, and
   waiting calls inside them gain `await`. The bundled packages written in Zore
   change too: `time.After`, `cancel.WithTimeout`, and `Token.Child` start their
   helper work with `go` on `async` helper functions. The compiler's messages
   point at each place. Slice 5 converts every example, test, and bundled package.

Spec text this changes: §17.2 and §17.3 (what `await` accepts), §17.7 and §35.1
(from "expected" to the locked design), §18.3 and §18.8 (what `go` runs), §19
(channel operations, and the `select` statement's `await` form in §19.14), §20.2
(`withLock`), §37.3 (the sentence that a waiting call "suspends only the calling
task" becomes: awaited in an async function, blocking in a plain function), and
§36.1 (the scheduler is a poller).

## Execution model

- A task is a heap block holding its frame, a `poll` function, its result, and
  scheduling state. A pool of worker threads (at least two, one per core) pops
  runnable tasks and calls `poll`. `poll` returns `Ready` or `Pending`.
- `Pending` means the task registered a waker with whatever it waits on: a
  channel queue, a mutex queue, the reactor (timer or descriptor), the helper
  thread pool, or another task. The waker puts the task back on the run queue.
  A scheduling flag (idle, running, notified) closes the window between "poll
  returned" and "wake arrived".
- Blocking calls from plain code still park a `Slot`. When that happens on a pool
  worker (an async function called a plain function that waits), the pool starts a
  replacement worker so the other tasks keep running. This is a safety net; the
  awaited form is the way to wait in a task.
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
- **Suspension points.** Every pause is an `await`, so the rule is one line: MIR
  marks each awaited call (an async function, a `Task`, or a waiting operation)
  as a suspension point. Ownership, region, and drop analysis treat it like any
  other call, so the existing checks, drop elaboration, and unwind edges apply
  unchanged.
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
  enqueues it. The owning-closure spawn path used today is removed with fibers.
- **Checks.** The HIR checker already knows whether it is inside an `async func`.
  It adds two errors: a waiting operation without `await` inside an async
  function, and `go` on a plain function.

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
usable. While the new forms are being added, the old ones stay accepted so each
PR can be tested against the whole existing suite; slice 5 turns the new rules
into errors.

1. **Spec and docs.** Update the sections above and record Q32. No code.
2. **Scheduler and wakers.** A poll-based task type and run queue beside fibers;
   the waiter abstraction in the runtime primitives; worker compensation for
   blocked threads. Nothing uses it yet.
3. **State machines for async functions.** `async_lowering/`, frames, `poll`,
   `await` of async calls and tasks, and `go` of async functions as polled tasks.
   Fibers still run plain-function tasks.
4. **`await` on waiting operations**, in this order: channels and `select`
   (including the `await select` form), time and the reactor, `Mutex`, then I/O
   and the helper pool. One PR each, adding the syntax, the check, and the poll
   form together, with tests that run the same program in plain and async code.
   Un-awaited waiting inside an async function is still accepted for now.
5. **Strict rules and migration; remove fibers.** Rewrite the bundled packages,
   examples, and tests to the new rules, turn on the two errors (un-awaited wait
   in an async function; `go` on a plain function), delete `fiber.rs` and the
   thread-per-task fallback, and make the tests that run tens of thousands of
   tasks async.
6. **Docs and guide.** Remove the approved deviation, update `architecture.md`,
   and add `async_lowering/` to the layout.

## Risks and open questions

- **Migration size.** About 300 lines of the native tests and seven of the
  examples use tasks or waiting calls. Most edits are mechanical (`async` on the
  function, `await` on the call), and the compiler points at each one.
- **No background work for plain functions.** With the stricter `go`, a plain
  function cannot be started in the background. An `async func` that never awaits
  does the same job, and a blocking-task form can be added later if a real need
  appears.
- **`await select`.** `select` is a statement, so this adds a small grammar form,
  `await` followed by a `select` block. The alternative is to make an async
  `select` implicitly suspend, which this proposal rejects to keep every pause
  visible.
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
