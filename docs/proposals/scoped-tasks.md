# Q10: opt-in scoped tasks

Status: Adopted for plain and async bodies under specification §§18.12–18.13.
The [implemented contract and examples](../scoped-tasks.md) replace the earlier
proposal; Git history preserves its alternatives and exit-path analysis.

| Choice | Decision |
| --- | --- |
| Opt-in lexical scope block | Adopted: mandatory join before parent cleanup on every exit |
| Make every task scoped by default | Rejected: changes ordinary handle detach and process-exit behavior |
| Implicit scope propagation across helper/callback calls | Rejected: borrowing must follow an explicit lexical boundary |
| Cancel children instead of joining | Rejected: cancellation requests do not prove completion or lifetime safety |
| Direct async scopes | Adopted under §18.13: persistent state, pollable joins and resumable unwind cleanup |

The adopted spelling is `scope`; lexical `go` selects the innermost scope.
Unretrieved results/errors are discarded after joining. Parent panic wins over
unobserved children; otherwise earliest spawn wins after all children finish.
Handles/views cannot escape, and shorter-lived referents are rejected initially.
These decisions are locked in the specification rather than inferred from
experimental behavior. Ordinary tasks remain safe without this optional feature.
