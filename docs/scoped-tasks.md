# Lexical task scopes

`taskScope { ... }` gives a plain function an explicit join boundary. Every
`go` written in that block or its nested ordinary blocks finishes before the
boundary releases parent storage, including on return, `?`, loop transfer or
panic. The contract is locked in specification §18.12.

```ore
func read(values Array<int>) int { return values[0] }
func example(values Array<int>) int {
    taskScope {
        let first = go read(values)
        let second = go read(values)
        return first.wait() + second.wait()
    }
}
```

The children share the caller's array; no clone or ownership transfer is needed.
The boundary joins any child whose result has not already been retrieved.
Dropping or ignoring a handle retains that obligation and the child's loans.
See the runnable [example](../examples/scoped-tasks).

## Inputs and results

Copy inputs are copied into child-owned storage; `own` inputs transfer as usual.
Shared Move arguments borrow eligible parent storage. `mut` arguments and
mutating captures borrow exclusively. Parent reads/writes/moves and conflicting
children are rejected while an exclusive child loan remains; shared children
allow other readers. Function-value arguments require exclusive access as in
ordinary function calls.

A directly spawned literal infers borrowing captures for reads/mutations and
owning captures for consumed Move values. Its heap environment keeps each
instance stable through completion, including repeated spawn sites. The
callable itself still moves into the task.

Eligible backing includes parameters, enclosing storage and initialized locals
declared directly in the block. A shorter inner-block or loop-iteration local
cannot be lent until the outer task-scope boundary. Pass independent Copy/own
inputs, or put the task scope inside that shorter block instead.

Scoped handles have an inferred, distinct scope identity; omit a `Task<...>`
annotation. Handles can move between local bindings within the same task scope,
but cannot escape through returns, storage, channels, helper calls or captures.
There is no detach conversion. `.wait()` consumes the handle once.

A retrieved view must have proved parent backing and stay inside the originating
scope. Child-local/child-owned backing and escaping views are rejected. A
scoped handle or view cannot acquire a longer lifetime through aliases.
Retrieved errors follow ordinary explicit handling or `?`; unretrieved error
results are discarded after join.

## Cleanup and failure

Every crossing exit joins first, then destroys parent locals. Return values and
propagated errors are evaluated once before joining. A loop transfer that stays
inside the block does not trigger a scope-wide join. Nested scopes close from
inside out; helpers and closure bodies do not inherit a caller's scope.

Child panics are reported where they originate. An explicit wait re-raises its
child panic and still joins the others during unwind. An existing parent panic
has priority over unobserved child panics. Otherwise the earliest spawned
unobserved child panic wins after all children finish. Unretrieved result
cleanup runs in reverse spawn order while parent storage is still alive;
destructor failures follow ordinary panic/double-panic rules.

The boundary requests no cancellation. A nonterminating child can keep it open
indefinitely. Worker compensation preserves runnable progress for blocking
plain joins, but cannot solve a channel/mutex dependency cycle. Fatal deadlock,
process abort and external termination do not promise cleanup.

## First-stage limits

Direct scopes in async functions and async literals are rejected. Async child
functions and plain helpers containing their own scopes are supported; a plain
helper called from async code blocks with ordinary worker compensation.
Existing restrictions on `go` in generic bodies remain; this stage rejects
a direct task-scope block there too.

Loan release after an explicit wait requires an unambiguous registered child.
For handles merged across branches or several live children from one repeated
spawn site, potentially outstanding loans remain until the boundary. Returned
views conservatively retain the input provenance needed by the current region
analysis. Callable view results with unproved backing are rejected. These
restrictions favor safe rejection over a guessed lifetime; no additional syntax
or runtime cancellation is implied.

Ordinary `go` outside these blocks keeps independent-input, handle-detach and
process-exit behavior. Further self-hosting remains deferred; the existing
lexer/parser comparison stages include the adopted syntax.

## Evidence

[Conformance coverage](../tests/conformance/scoped-tasks.md) maps frontend,
ownership, runtime and native checks to the contract. Tests cover shared and
exclusive loans, handle discard/escape, view provenance, nested/repeated scopes,
all crossing exits, parent/child/destructor failure, one-worker compensation,
bounded internal deadlock and tracked cleanup after quiescence.

Timing and resource high-water marks are not correctness assertions.
