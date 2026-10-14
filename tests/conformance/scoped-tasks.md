# Lexical task scopes (§§18.12–18.13)

Plain and direct async scopes are locked and implemented. Executable ownership
cases live in `tests/ownership/scoped_tasks.rs`, native lifecycle cases in
`tests/codegen/scoped_tasks.rs` and `tests/codegen/async_scopes.rs`, and record/result
ownership units in `runtime/src/task_scope.rs`. Lexer/formatter regressions and
self-hosting lexer/parser comparisons cover the syntax.

| Case | Required behavior |
| --- | --- |
| Shared children and borrowing closure | Concurrent reads join before referent/capture destruction |
| Exclusive child | Reject parent read/write/move/drop and overlapping child until explicit successful wait |
| Dropped/ignored/moved handle | Registry retains child and its loans; moving within scope does not shorten them |
| Handle escape | Reject return, storage, channel send, calls, closure capture and cross-scope rebinding |
| Short backing | Reject inner-block/loop local or temporary, also transitively through a view/callable |
| Retrieved view | Prove parent provenance; reject child backing and escape through aliases/composites |
| Fallthrough and transfers | Join before drop on return, crossing break/continue and non-nil ?; evaluate result once |
| Local loop transfers | No premature join on break/continue staying inside the task scope |
| Child panic | Join every child, choose earliest spawned unobserved panic, clean owned inputs/results once |
| Parent/explicit-wait panic | Preserve parent panic while joining remaining children before parent storage drops |
| Nested scopes | Close innermost first, no implicit scope propagation into helpers/closure bodies |
| Repeated spawn site | Each child has stable capture/argument storage, including inside loops |
| Child result cleanup | Result destructors finish while parent referents remain valid |
| Scheduling | Plain joins compensate a small worker pool; internal cycles fail with bounded deadlock diagnostic |
| Async | Accept direct scopes in async functions/literals; retrieval uses `await`, and `.wait()` remains plain-only |
| Async persistence | Pin registry/referents/drop flags and pending transfers; never reuse registry slots or repeat evaluated returns/errors |
| Async unwind | Poll joins while preserving the original panic across Pending and worker migration; close nested boundaries before parent drops |
| Completion races | Atomically register wakeups; latch wake-before-park; complete and destroy results exactly once |
| Async scheduling | Progress on one worker through suspending joins; internal cycles still produce bounded fatal deadlock diagnostics |
| Ordinary tasks | Existing independent-input rejection, detach and process-exit behavior unchanged |
| Frontend/tooling | Parse/format round trip, exact lowercase `scope` keyword without an alias, scoped diagnostics and unused-body checking |

The first-stage checker releases explicit-wait loans only for a uniquely
identified child. Ambiguous merged handles or multiple live children from a
repeated site retain possible loans until the boundary. Returned views keep
conservative input provenance; unproved callable backing is rejected.
