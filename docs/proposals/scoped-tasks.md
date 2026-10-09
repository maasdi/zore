# Q10 proposal: opt-in scoped tasks

Status: Design accepted for future specification work; not locked or implemented.
Every `taskScope` example below is proposed syntax and must be rejected by the
current compiler. The locked rules in specification §§16.4, 18.4–18.6, and
18.11 remain authoritative:
ordinary `go` cannot borrow another task's local storage, dropping a `Task`
detaches it, and the process abandons unfinished work when the initial task
completes. Q09b already makes the current model memory-safe.

## Recommendation and decision table

The accepted design choice is an opt-in lexical `taskScope { ... }` block. Its
task creation, borrowing, and completion rules apply only to `go` expressions lexically inside
that block or its nested ordinary blocks. They do not propagate into a called
function or a closure body. Every task registered with the scope finishes
before the scope releases any storage that task might borrow. Ordinary `go`
elsewhere keeps its current independent-input and detach behavior.

| Choice | Safety proof | Simplicity and runtime cost | Migration | Recommendation |
| --- | --- | --- | --- | --- |
| Opt-in lexical scope | Compiler bounds scoped loans by a mandatory join on every exit; handles cannot escape | New region and cleanup machinery; each scope tracks and joins its children | Existing programs keep their meaning | Accepted design direction; specification pending |
| Make all tasks scoped by default | Could prove borrows after changing all handle and process-exit paths | Simple default spelling, but dropping a handle could block or hang; background work needs a new detach operation | Breaks §§18.5–18.6 and 18.11 and programs relying on background tasks | Reject |
| Keep current conservative rejection | Already safe through independent task inputs | No new compiler or runtime work; no local borrowing across tasks | None | Retain until the opt-in design is specified and implemented |

The recommendation is not a request to revise default `Task` semantics. A
callback-only library API is also less suitable as the core facility: plain
callbacks would block when called from `async func`, while async closure
literals and suspension-safe callback cleanup are not currently defined.

## Proposed source and boundary

The spelling and all examples in this section are proposals, not valid Zore
programs today. `taskScope` creates a lexical region and a registry. A `go`
expression in it returns a scoped handle, written conceptually as
`ScopedTask<R1, ..., Rn>` below. This is a compiler-internal description, not a
proposed user-written type form. The handle is Move and can be rebound to a
local in the same scope. It cannot be returned, stored in a field or
collection, sent on a channel, captured by a task, or passed to another
function. There is no conversion to ordinary `Task` or detach operation for
it. A discarded scoped handle leaves its task in the registry; dropping the
handle never shortens a loan or detaches the task.

```ore
// Proposed, rejected today.
func inspect(values Array<int>) int { return values[0] }

func sum(values own Array<int>) int {
    taskScope {
        let first = go inspect(values) // proposed scoped shared borrow
        let second = go inspect(values)
        return first.wait() + second.wait()
    }
}
```

The `return` above waits for both scoped tasks before transferring its value
and before dropping `values`; explicit retrieval has already completed them.
If a handle is discarded, the scope still waits at its boundary and discards
its result. This includes an `error` result, as discarding an ordinary task
handle currently acknowledges its result. To use or propagate an error, the
program must explicitly retrieve the scoped handle with `.wait()` in a plain
function or `await` in an async function and apply the existing `?` rule.

Only a `go` written in the lexical scope registers there. A `go` inside a
called helper or inside a closure body keeps the existing ordinary-task rules,
even if the helper is invoked while a scope is open. This prevents an implicit
scope capability from crossing a call boundary. A nested `taskScope` has its
own registry. Its children finish before it closes; a child of the outer scope
that opens an inner scope cannot finish until its own inner scope closes.

### Loans, captures, and results

- A scoped task may borrow a parent local or borrowed parameter if its backing
  storage lasts until the mandatory join. Copy and `own` inputs keep their
  existing meanings. A shared loan permits concurrent readers; a `mut` loan
  is exclusive until that child completes. The parent cannot read through an
  active exclusive loan, write through an active shared loan, move the backing
  value, or start a conflicting child. An explicit successful wait ends the
  child's input loans except any borrow carried by its retrieved result.
  Discarding the handle does not.
- For a scoped `go` on a closure literal, the proposed exception to §16.4's
  owning-spawn rule permits ordinary inferred borrowing captures. Read-only
  captures are shared, mutations use an exclusive capture, and consuming a
  captured Move value transfers that value to the task as an owning call-once
  capture. The task's closure environment and borrowed referents stay pinned
  until completion. A capture containing a view keeps its backing loan alive.
- A borrow of a local declared directly in the `taskScope` block is eligible
  when initialized before spawning. A local declared in a shorter nested
  block or loop iteration is initially rejected as a scoped-task input, even
  if a normal path appears to wait. Its storage could be dropped before the
  scope boundary through another exit. A later extension could prove an
  unconditional completion before that shorter lifetime ends.
- A scoped task result may contain a view only when ordinary output-provenance
  checking proves it points to borrowed parent storage that remains valid
  through retrieval. A view into the child's own locals is rejected. During
  the initial extension, a view-bearing scoped result cannot leave the scope,
  even if its backing belongs to an outer scope. This is conservative; a
  future region proof could allow an outer-backed view to escape. Ordinary
  `Task` results retain the current ban on views.

These restrictions are part of the proposed safety proof, not a hidden change
to ordinary closure or task typing. A scoped loan belongs to the scope's task
registry, so moving or dropping the handle cannot end it prematurely.

## Exit-path proof

Scope closure has two ordered phases. First stop registration and wait until
every registered child has finished, including its own cleanup. Only then run
ordinary local drops in reverse order and continue the pending control
transfer. Values in an enclosing scope that were borrowed remain protected by
their loans until the join phase completes. A child that has already been
explicitly retrieved is complete and needs no second wait.

| Exit or event | Join and result of transfer | Why borrowed storage stays valid |
| --- | --- | --- |
| Fallthrough | Join all children, then drop scope locals | No referent is dropped before its last child finishes |
| `return` from the enclosing function | Evaluate return values once, join, drop locals, then return | A return cannot carry a scoped handle or a scope-bound view |
| `break` or `continue` crossing the scope boundary | Join before the loop transfer and its remaining cleanup | The current iteration's referents remain live through the join |
| `break` or `continue` staying within the scope | No scope-wide join yet; ordinary inner-block cleanup still applies | A child cannot borrow a shorter-lived inner-block local under the initial rule |
| `?` with a non-nil error | Save the error, join, drop, then propagate it | Error propagation cannot bypass the join; an unobserved child panic can supersede the return |
| Parent task panics | Preserve the parent panic, join children, then unwind and drop | The task does not release a referent during the wait |
| Child panics | Child cleans up and becomes complete; the scope joins every other child before a boundary re-raise | Failure does not remove the failed child's completion obligation |
| A local destructor panics during scope cleanup | Children have already joined; apply existing panic or double-panic rules | No scoped child still refers to the destroyed local |
| `main` completes through an active scope | Join precedes completion of the initial task | §18.11's immediate exit begins only after `main` actually completes |
| Process abort, external termination, or a fatal deadlock | The process ends without promising drops | No task can access released parent memory after process termination |

An exceptional exit from a nested block must not silently drop a borrowed
referent before joining its child. The initial restriction on shorter-lived
referents makes that proof local to the scope boundary. The compiler must also
reject explicitly dropping or replacing a borrowed parent value before the
child completes.

## Failures, nontermination, and scheduling

Panic is not converted into `error` or made catchable. Every child panic is
reported by the runtime at its origin under §18.10. If an explicit `.wait()`
or `await` retrieves a panicked child, that panic is re-raised there as today;
scope cleanup joins the other children while the parent unwinds. If the parent
is already panicking, unobserved child panics are reported at their origins
but not re-raised in the parent, avoiding a second unwind. On a boundary exit
without a parent panic, join every child and re-raise the earliest spawned
unobserved child panic. All later failures have already been reported. This
choice gives deterministic priority independent of completion order. It
supersedes a pending `return` or `?` result, as a panic during cleanup can.
The exact failure-priority rule still requires a locked specification.

No cancellation is required or implied. A cancellation request would not be
a lifetime proof: cleanup still must wait for task completion. A child that
never finishes keeps the scope open, including on a parent panic path.
Holding a mutex while joining a child that needs that mutex, waiting on a
child that needs the parent to produce a channel value, and cyclic nested
joins can all deadlock. Worker compensation preserves runnable-task progress
when a plain scope blocks a worker; it cannot resolve these program-level
cycles. Existing fatal deadlock detection may terminate the process when no
internal wait can make progress. External waits may leave a scope waiting
indefinitely. Neither path may unwind the parent and free borrowed storage
while a child remains live.

The scope registry costs at least one completion record per registered child
and a join on each exit. Scope entry and closure do not spawn a new thread.
Forcing a join on scope exit may delay a return or panic indefinitely; that is
an explicit cost of choosing this block, not a change to ordinary `go`.

### Plain and async functions

The first implementation stage should accept `taskScope` only in a plain
`func`. Its boundary blocks until all children complete, using the existing
worker-compensation guarantee if that plain function runs on a scheduler
worker. Both plain and async child functions can run in the scope. A plain
helper with a scope called from an `async func` still blocks its worker and
must receive compensation, as any plain helper that waits does today.

Direct use in `async func` is a later stage. It must keep the scope registry,
borrowed locals, and child results in the persistent async frame and suspend
the parent poll while joining. It also needs a pollable unwind/cleanup path:
an async parent panic cannot synchronously free its frame while scoped children
borrow it. Until that path exists and is tested for every exit in the matrix,
the compiler must reject a `taskScope` block directly inside `async func`.
When supported, explicit retrieval remains `await scopedTask`; direct
`.wait()` stays invalid in an async body. The parent task's one scheduler wait
must be registered and woken by child completion without lost wakeups or false
deadlock counts. Synchronous mutex callbacks remain plain and block with
compensation; no callback may implicitly borrow a scope from its caller.

## Proposed accepted and rejected examples

All examples in this section are proposed future conformance cases. None is
currently accepted by the compiler.

```ore
func read(values Array<int>) int { return values[0] }
func update(value mut int) { value += 1 }

func examples(values own Array<int>) {
    taskScope {
        let a = go read(values)    // proposed accepted: shared borrow
        let b = go read(values)    // proposed accepted: overlapping shared borrow
        let c = go func() int { return values[0] }() // proposed borrowed capture
        println(a.wait() + b.wait() + c.wait())
    }

    var count = 0
    taskScope {
        let t = go update(count)   // proposed accepted: exclusive borrow
        t.wait()
        println(count)             // proposed accepted: child is complete
    }
}
```

```ore
// Proposed rejections inside a taskScope block.
let t = go update(count)
println(count)                     // conflicting parent read before completion
let other = go update(count)       // second exclusive borrow overlaps
drop(t)
println(count)                     // dropping a scoped handle did not complete it
go func() { count += 1 }()         // rejected while another exclusive loan lives
return t                           // scoped handle cannot escape
```

```ore
// Proposed shorter-lifetime rejection, even if the scope continues.
taskScope {
    if enabled {
        let short = [int; 2]{1, 2}
        go read(short)             // short ends before the taskScope boundary
    }
}
```

```ore
// Proposed view-result rule. `takeFirst` returns a view of its input.
taskScope {
    let t = go takeFirst(values[:]) // allowed if output provenance is proved
    let first = t.wait()
    println(first[0])              // use inside scope allowed
    return first                   // rejected in the initial extension
}
```

Outside a `taskScope`, `go read(values)` remains rejected when `values` is
another task's local, even if followed immediately by `.wait()`. A plain
owning `go consume(values)` remains valid and independently lifetime-safe.

## Compiler and runtime work after acceptance

1. **Decision and syntax checkpoint.** The maintainer accepted the opt-in scope
   direction. Amend §§16.4, 17.8, 18.4–18.11 and the grammar under §53, with
   ownership, error, and async examples, then explicitly lock the exact rules
   and add conformance cases. Until that checkpoint, keep rejecting the syntax
   and borrowed spawns.
2. **Plain-scope semantic checkpoint.** Give each lexical scope a stable ID;
   infer capture and argument loans against that region; reject handle or
   view escape, shorter-lived referents, and conflicting accesses. Lower all
   normal and exceptional exits through a mandatory join before drop
   insertion. Preserve argument evaluation order and source spans.
3. **Plain-scope runtime checkpoint.** Keep a scope registry independent of
   handles, with completion, failure ordering, and an idempotent join. Reuse
   task wakers and worker compensation. Verify that a dropped handle stays
   registered and that every exit drains children before parent cleanup.
4. **Async-scope checkpoint.** Add persistent scope state and pollable cleanup
   and unwind, then prove the same exit matrix while other tasks make progress
   on a small worker pool. No direct async-scope syntax is accepted before
   this checkpoint passes.
5. **Optional precision checkpoint.** Consider safe handle movement inside
   scope-local collections, borrowing shorter-lived locals after a dominating
   wait, or allowing a proven outer-backed view result to escape. Each needs
   separate region and cleanup tests; none is needed for the initial facility.

Future tests belong in `tests/conformance/concurrency.md` as named pending
cases, then in `tests/typecheck/check.rs` for each accepted/rejected loan and
escape example and `tests/codegen/native.rs` for bounded exit, cleanup,
failure-order, and nested-scope behavior:

| Future case | Main assertion |
| --- | --- |
| `scope_shared_reads_join_before_drop` | Two shared children and one borrowing closure finish before backing storage drops |
| `scope_exclusive_loan_blocks_parent` | Parent read, second writer, and move are rejected until explicit completion |
| `scope_handle_drop_and_escape` | Handle drop retains the child; return, channel send, and owning capture of the handle are rejected |
| `scope_short_lived_referent` | A loop-iteration or inner-block local cannot be lent past its lifetime |
| `scope_view_result_provenance` | Parent-backed view works inside scope; child-local and escaping views are rejected |
| `scope_exit_paths` | Fallthrough, return, `break`, `continue`, and `?` all join before drops |
| `scope_panic_priority_and_cleanup` | Parent and multiple child panics follow the priority rule; owned values drop once |
| `scope_nested_and_deadlock` | Nested registries join transitively; a bounded internal deadlock reports without freeing borrowed storage |
| `scope_plain_and_async_workers` | Plain blocking joins compensate workers; later async joins suspend and wake exactly once |

Native tests should synchronize through channels rather than sleeps. Compiler
review must cover parser, resolution, ownership/regions, MIR drop insertion,
async lowering, and diagnostics independently of LLVM. Runtime review must
cover wake-before-park, worker compensation, deadlock counting, and panic
cleanup. No new dependency is justified by this proposal alone.

## Outstanding decisions

| Question | Proposed initial answer | Gate |
| --- | --- | --- |
| Exact block keyword and whether `go` inside it is always scoped | `taskScope`, always scoped in its lexical body | Lock in specification before grammar change |
| Can an unobserved child error be discarded after join? | Yes, like a dropped ordinary `Task`; explicit retrieval is required for propagation | Lock in specification before error rules change |
| Which panic wins if several children fail? | Earliest spawn among unobserved failures, after all children finish | Lock in specification before runtime change |
| Can short-lived inner-block locals be borrowed? | Reject initially | Optional later proof |
| Can scoped handles or view results escape? | Reject initially | Optional later proof |
| Can async functions contain direct scopes? | Reject until pollable unwind and join exist | Async-scope checkpoint |

Design approval does not itself lock Q10, alter ordinary task behavior, or
make scoped tasks necessary for current MVP safety. The specification and
conformance update is the next gate before implementation.
