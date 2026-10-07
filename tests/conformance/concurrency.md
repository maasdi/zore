# Task, async, and channel runtime conformance cases

The async call contract, task typing, retrieval, panic, process-exit, and spawn
input rows are covered by executable tests in `tests/typecheck/check.rs` and
`tests/codegen/native.rs`; the channel rows for close repetition, blocked operations, the zero value, and
buffered-value cleanup are covered by executable tests in the same files; the
message escape proof is covered by the element-type rule, and mutex poisoning
is covered by `conformance/mutex.md`.

Authority: spec §6.3, §7.6, §11.7, §15.2, §15.4, §17.8, §18.4,
§18.8–18.11, §19.4, §19.8–19.13,
§20.2, §36.2, §41.4. These are pending type-checking, ownership, and runtime
cases; they are not executable tests or passing coverage. Runtime cases need
bounded execution and explicit synchronization, not timing sleeps
(`tests/README.md`).

## Async call contract (§17.8)

| Scenario | Expected result |
| --- | --- |
| `await fetchUser(1)` inside an `async func` | Valid |
| `go fetchUser(1)` in a synchronous or async function | Valid; yields `Task<User, error>` |
| `fetchUser(1)` as a bare statement | Reject: async call neither awaited nor spawned |
| `let f = fetchUser(1)` | Reject: async call neither awaited nor spawned |
| `process(fetchUser(1))` | Reject: async call passed as an argument without `await` |
| `await fetchUser(1)` inside a synchronous function | Reject: `await` outside an async body |
| `await` inside a non-async closure defined in an async function | Reject: the closure body is not an async body |

## Waiting in async and plain functions (§17.3, §18.3)

Pending until the state-machine slices land; the same programs also run with
fibers today.

| Scenario | Expected result |
| --- | --- |
| `ch.receive()` inside an `async func`, without `await` | Valid; suspends the task |
| `ch.receive()` inside a plain function | Valid; blocks the calling thread |
| `ch.receive()` inside a closure written in an `async func` | Valid; the closure body is not async, so it blocks the thread |
| `await ch.receive()` | Reject: `await` needs a call to an `async func` or a `Task` |
| Many `async func` tasks each waiting on a channel | All finish; no task needs a thread of its own |
| An async function calls a plain function that waits, in many tasks at once | Other tasks keep making progress (§18.9) |
| `go plainFn(x)` | Valid; yields a `Task`; the function runs to completion on a runtime thread |

## Task typing and classification (§18.8)

| Scenario | Expected result |
| --- | --- |
| `go compute()` where `compute() int` | Type `Task<int>` |
| `go load()` where `load() (User, error)` | Type `Task<User, error>` |
| `go log()` where `log()` has no results | Type plain `Task` |
| `let t Task<User, error> = go load()` | Valid annotation |
| `let t Task<int> = go load()` | Reject: type arguments do not match the result list |
| A written `Task<error, User>` | Reject: violates the §7.2 error-position rule |
| `let b = a` where `a` is a `Task<int>` | Move: `a` is unusable afterward |
| `func collect(tasks own Array<Task<int>>) int` retrieving each element | Valid; each result typed `int` |
| Sending a `Task<int>` over a `channel<Task<int>>` | Valid; ownership transfers |

## Retrieving task results (§18.9)

| Scenario | Expected result |
| --- | --- |
| `let v = task.wait()` in a synchronous function | Valid; blocks until completion |
| `task.wait()` written directly inside an `async func` | Reject: use `await task` |
| `let v = await task` inside an `async func` | Valid; suspends until completion |
| `await task` inside a synchronous function | Reject: `await` outside an async body |
| `task.wait()` then `task.wait()` on the same binding | Reject: use after move |
| `await task` then `await task` on the same binding | Reject: use after move |
| `let v, err = task.wait()` where the task yields `(T, error)` and `err` is never used | Reject: never-used error binding (§15.6) |
| `nil` task: `var t Task<int> = nil; t.wait()` | Runtime panic |
| Synchronous helper that calls `.wait()`, invoked from an async function, on a runtime with one worker | Other ready tasks still run; the program does not starve |
| Many async tasks each calling a synchronous helper that blocks in `.wait()` | Completes; the runtime may use extra OS threads |
| A task retrieving its own handle (sent to itself through a channel) | Program-level deadlock; not prevented by the progress guarantee |

## Panics in tasks (§18.10)

| Scenario | Expected result |
| --- | --- |
| Spawned task panics while owning a resource | That task unwinds and drops the resource; other tasks keep running |
| Spawned task panics; its handle is later retrieved with `.wait()` | The panic is raised again in the retriever at the `.wait()` |
| Spawned task panics; its handle is retrieved with `await task` | The panic is raised again at the `await` |
| Detached task panics | Panic reported on stderr; the process continues |
| Any panic | Reported on stderr with message and originating task when it occurs |
| Initial task panics while spawned tasks are running | Initial task unwinds; the process terminates; other tasks are abandoned without drops |
| Retrieving a panicked task's `(T, error)` result | No `error` value is produced; the retriever panics instead |
| Task panics while holding a mutex's lock | Lock released during unwind; mutex marked poisoned; a later locker is told (mechanism belongs to the mutex API, Q05) |

## Process exit (§18.11)

| Scenario | Expected result |
| --- | --- |
| `main` returns while a detached task is mid-loop | Process terminates immediately; the task's pending drops do not run |
| `main` returns with values still in a channel buffer | Process terminates; buffered values are not dropped |
| `main` keeps a worker's handle and calls `.wait()` before returning | Worker completes, including its cleanup, before exit |
| Detached infinite background loop | Does not prevent the process from exiting when `main` returns |

## Channel close repetition and blocked operations (§19.11)

| Scenario | Expected result |
| --- | --- |
| `ch.close(); ch.close()` | Runtime panic on the second close |
| Two tasks sharing a handle each call `close()` | The second close to run panics |
| Sender blocked on a full buffered channel when it is closed | That send panics; its Move value is dropped during the sender's unwind |
| Sender blocked on an unbuffered channel with no receiver when it is closed | That send panics |
| Receiver blocked on an empty channel when it is closed | Wakes and returns zero value, `false` |
| Close while buffered values remain, then receive | Buffered values first, then zero value, `false` (§19.8) |

## Channel zero value (§19.12)

| Scenario | Expected result |
| --- | --- |
| Receive from a zero-value channel | Returns zero value, `false` immediately |
| Send on a zero-value channel | Runtime panic |
| Close a zero-value channel | Runtime panic (already closed) |
| `var ch channel<int> = nil` | Reject: channels have no `nil` state |
| `ch == nil`, `ch1 == ch2` | Reject: channels are not comparable |

## Channel lifetime and buffered values (§19.13)

| Scenario | Expected result |
| --- | --- |
| Last handle to a channel with buffered Move values goes out of scope | Each buffered value is dropped exactly once; order unspecified |
| Handle copied into a struct field that outlives the original binding | Channel stays alive; buffered values are not dropped yet |
| Handle captured by a running task after the spawner's copy is gone | Channel stays alive until the task's copy is gone |
| Channel whose buffer holds a handle to itself, all outside handles gone | Channel and its buffered values are never freed while the process runs (stated leak); no crash or undefined behavior |
| Two channels whose buffers hold handles to each other | Same stated leak |
| Reply-channel pattern: a request carrying a `channel<Reply>` handle | Valid; no cycle, so ordinary cleanup applies |

These cases do not define the full `go` expression grammar (Q02), the
entry-point signature or exit status (Q05), or scoped tasks (Q10).

## Spawn lifetime proof on every exit (§18.4)

| Scenario | Expected result |
| --- | --- |
| Spawn a shared borrow of a parent-local owned array, immediately followed by wait | Reject; normal-path retrieval is not an unwind guarantee |
| Spawn a mutable borrow of parent-local storage, immediately followed by await task | Reject for the same reason |
| Spawn a borrow of a parent's borrowed parameter or temporary | Reject; parent borrowing does not supply independent storage |
| Spawned borrow would escape through a parent's `?` or panic | Reject at spawn, before runtime |
| Parent transfers or drops a handle while child would still borrow its locals | Reject; handle movement/detachment cannot extend backing lifetime |
| Spawn using a Copy integer, string, or channel handle into a shared parameter | Valid; task owns its copied argument storage before parent can exit |
| Spawn using a Move array into an own parameter | Valid; task owns the transferred allocation |
| Spawn using a Move array into an ordinary shared parameter | Reject; no implicit move or clone into the task |
| Spawn using a mut parameter with a Copy caller-local argument | Reject; copying would silently change the mutation contract |
| Spawn using a Copy wrapper containing a view into parent-local storage | Reject recursively; Copy does not make backing independent |
| Spawn closure capturing such a view, including nested in a Move container | Reject recursively, independent of capture grammar |
| Parent returns through `?` or panics after a valid owned-input spawn | Child inputs remain independently valid; handle cleanup detaches |
| Task result would borrow its task-owned argument or local storage | Reject; retrieval cannot extend destroyed task storage |
| Unknown or unproven package-storage lifetime used for a spawned borrow | Reject; Q05 supplies no immortal-storage assumption |
| Direct await of an async call borrowing caller storage | Allowed when §17.6 proof succeeds; this is not detached work |

## Channel message escape proof (§19.4)

| Scenario | Expected result |
| --- | --- |
| Send owned array of independent elements | Valid; ownership transfers normally |
| Send string or channel handle | Valid; independent Copy value |
| Send view into sender-local array | Reject; receiver may outlive sender |
| Same view nested in a Copy struct, owned container, or closure | Reject recursively |
| Send local-backed view over an unbuffered channel | Reject; completed send is not completed receiver use |
| Sender waits for a later acknowledgment after sending local-backed view | Reject; normal-path protocol does not prove safety across unwind |
| Message contains borrowed storage with unknown external lifetime | Reject rather than assume independent lifetime |

## Deadlock (runtime)

| Scenario | Expected result |
| --- | --- |
| The initial task receives or sends on a channel no other task uses | Exit status 2 and "all tasks are asleep" on standard error |
| Two tasks each wait for the other through channels | Same |
| The initial task waits on a task that waits on a channel nobody serves | Same |
| The last running task finishes while the others wait on channels | Same |
| A task waits on a timer, a file read, a pending accept, or a busy task | Not a deadlock; the program continues |
