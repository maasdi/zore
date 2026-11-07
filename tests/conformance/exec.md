# Environment and subprocess control

Locked contract: specification §37.3. Native coverage lives in
`tests/codegen/subprocess.rs`; pure runtime validation is in `runtime/src/exec.rs`.
External-package ownership and async checks live in `tests/packages/exec.rs`.

| Case | Required result |
| --- | --- |
| Lookup a missing, present empty, and present nonempty variable | Presence boolean distinguishes missing from empty |
| Lookup empty, NUL, or equals-containing keys | Empty value and false |
| Modify an Environ snapshot | Parent environment and later child configuration unchanged |
| Overlay child variables and repeat a key | Parent variables inherited; last override wins |
| Clear child environment and set an empty value | Only explicit entries exist; empty remains present |
| Invalid child environment entry | Error before spawn; returned Process is closed |
| Start with piped input/output/error | Owned endpoints satisfy std/io; raw bytes roundtrip |
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
| Capture an infinite output producer with limits 0, 1 and chunk boundaries | Exact retained prefix, limit error, child killed and reaped |
| Combined output at and past exact limit | Shared-pipe write order retained; overflow prefix returned |
| Negative or greater-than-1-GiB capture limit | Error before spawn |
| Async native wait/pipe/capture operations | Helper suspension classification keeps other tasks progressing |
| Copy or use a moved Process/pipe; construct private fields | Rejected by ordinary ownership and visibility rules |

Native subprocess tests build a Rust helper and use bounded parent-process
execution. Ready bytes gate cancellation and cleanup instead of sleep-based
lifecycle assumptions. Descendant process-tree cleanup and descendant pipe EOF
are deliberately outside this direct-child contract.
