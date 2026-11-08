# Environment and subprocess control

Locked contract: specification §37.3. Native coverage lives in
`tests/codegen/subprocess.rs`; pure runtime validation is in `runtime/src/exec.rs`.
External-package ownership and async checks live in `tests/packages/exec.rs`.

| Case | Required result |
| --- | --- |
| `Getenv` of a missing, present empty, and present nonempty variable | Presence boolean distinguishes missing from empty |
| `Getenv` of empty, NUL, or equals-containing keys | Empty value and false |
| `Getenv` used as a single value | Rejected: the call returns two values |
| Modify an Environ snapshot | Parent environment and later child configuration unchanged |
| Overlay child variables, with `SetEnv` and `Env` directly, and repeat a key | Parent variables inherited; last override wins |
| `SetEnv` with an empty key or one containing `=` or NUL | Panics with `exec: invalid environment key` |
| A `Stream` other than `Null`, `Inherit` or `Pipe` | `exec: invalid stream` before spawn; `Pid` of the failed handle is 0 |
| Clear child environment and set an empty value | Only explicit entries exist; empty remains present |
| Invalid child environment entry | Error before spawn; returned Process is closed |
| Start with piped input/output/error | Owned endpoints satisfy std/io; raw bytes roundtrip |
| `Inherit` stdout and stderr in `RunWith` | The child's output appears in the parent's own stdout and stderr |
| `Pid` of a running child | A positive host identifier |
| Take a requested endpoint twice | Second take fails |
| Close child stdin | Child sees EOF |
| Drain both output streams concurrently | Child completes without pipe-capacity deadlock |
| Wait repeatedly after successful/nonzero exit | Cached Status and error; exit facts preserved |
| Kill an already exited child | Success without altering cached exit result |
| Start with a pre-canceled context | Context error and no child side effects |
| Cancel a child gated on stdin | Wait returns context canceled; child reaped |
| Deadline while child is gated | Wait returns context deadline exceeded; child reaped |
| Natural exit then context cancellation | Cached natural result wins |
| Close/drop a running child | Direct child killed and reaped; monitors terminate |
| Zero/default handles | Operations fail; no stale handle aliasing |
| Capture an infinite output producer with limits 0, 1, 8192, 9000, 65536 and 70000 | Exact retained prefix, limit error, child killed and reaped |
| `OutputWith` with `SetEnv`, and with stdout `Inherit` and stderr `Inherit` | The child sees the variable; stdout is captured anyway and stderr reaches the parent |
| `RunWith` and `CombinedOutputWith` past a context deadline | `context deadline exceeded`; child reaped; nothing captured |
| `RunWith` with any `Pipe` stream; capture with `Pipe` stdin or (for `OutputWith`) stderr | Error naming `Start`, before spawn |
| `CombinedOutputWith` with `Pipe` stderr | Allowed: both streams are captured |
| Combined output at and past exact limit | Shared-pipe write order retained; overflow prefix returned |
| Negative or greater-than-1-GiB `OutputLimit` | `exec: invalid output limit` before spawn |
| Async Wait and explicit Close | Channel suspension keeps other tasks progressing; exit is noticed by nonblocking checks at most 50 ms apart |
| Eight gated children and waiters with helper capacity two | Waiting children reserve no helpers; every canceled waiter receives the context error |
| Both helper workers blocked in pipe reads | Start, Kill, cancellation, Close, drop and reaping still progress independently of the helper queue |
| Copy or use a moved Process/pipe; construct private fields | Rejected by ordinary ownership and visibility rules |

Native subprocess tests build a Rust helper and use bounded parent-process
execution. Ready bytes gate cancellation and cleanup instead of sleep-based
lifecycle assumptions. A controlled runtime sets helper capacity to two and gates
on actual active helper counts before exercising saturated-pool controls.
Descendant process-tree cleanup and descendant pipe EOF
are deliberately outside this direct-child contract.
