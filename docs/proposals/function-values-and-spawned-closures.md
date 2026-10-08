# Q33 proposal: declared function values and `go` on owning callables

Status: PROPOSED. Nothing here is locked, and no implementation may rely on it
until the maintainer accepts it and the decisions are written into the
specification and locked (§53). The specification stays authoritative. This
document is stage 1 of issue #52; stages 2 and 3 wait for acceptance.

Scope: a declared synchronous function's name used as a value; `go` applied to
a function value, a closure literal, or a closure variable; and how those
compose with the existing capture, task-input, and result rules. Out of scope:
scoped tasks (Q10), implicit joins, capture-marker syntax, source lifetime
annotations, and async callables beyond the boundary drawn in "Async callables".

## Baseline today

| Area | Current rule |
| --- | --- |
| Function names as values | §16.2: not locked, rejected as unsupported |
| Closures with tasks | §16.4: unsupported (Q02g) |
| `go` operand | §18.3, Q25(a): a call to a declared function or method; a call through a function value and a closure literal are rejected |
| Task inputs | §18.4, Q25(b): Copy copied, `own` moved, `mut` rejected, views and function values rejected, Move for a shared parameter rejected |
| Task results | Q25(c): no slice or function value |
| Implementation of `go f(args)` | The checker already builds an owning, call-once closure over the evaluated arguments; the runtime runs it once (`spawn_thunk`). A spawned closure is therefore the same shape the runtime already executes |

The existing machinery is enough. The proposal adds no task-safety model of its
own: a spawned callable must be an **owning** closure value, and §16.4 already
defines what an owning closure may capture.

## Decisions requested

1. A declared synchronous function's name is a function value (named-function
   conversion).
2. `go` may apply to any expression of function type, and a closure literal may
   be written as the callee.
3. The spawned callable is moved into the task and is consumed by its single
   run.
4. Spawning is a new owning position in §16.4's list.
5. Async callables are not part of this change.

## 1. Named-function conversion

Writing the name of a declared function where a value is expected yields a
closure value whose function type is the declaration's signature with names
removed.

```ore
func add(a int, b int) int { return a + b }
func scale(values mut []int, factor int) { /* ... */ }

let f = add                  // func(int, int) int
let g func(mut []int, int) = scale
println(f(1, 2))
```

Rules:

- The value has the same type a closure literal with that signature would have
  (§16.2 identity: parameter types, modes, and results, in order). There is no
  implicit conversion between modes and no subtyping.
- The value is a capture-free closure. It has no environment and nothing to
  destroy. Like every function-typed value it is **Move** (classification is by
  type, and the type does not know which closure it holds).
- Evaluating the name has no effect and happens once, where the name is written.
- Calling the value uses the ordinary call rules of §16.2: callee first, then
  arguments, exclusive use of the callee for the call.
- A package-qualified name (`pkg.F`) converts the same way for every function
  the package exports, whether it is written in Zore or implemented by the
  compiler: a caller cannot and should not tell the difference. The compiler
  supplies a forwarding wrapper for a native function. Built-in operations
  (`println`, `len`, `push`, `clone`, channel and mutex operations, and `error`
  construction) are not functions and stay rejected, with the existing
  diagnostic.
- A declared function is not a local, so converting it never borrows or moves
  anything.

Accepted:

```ore
let apply = func(op func(int, int) int, x int, y int) int { return op(x, y) }
println(apply(add, 2, 3))   // 5
```

Rejected:

```ore
let a = fetchUser            // async func: unsupported (see "Async callables")
let b = counter.increment    // method value: unsupported (see "Methods")
let c = println              // built-in
let d = add
let e = add
if d == e { }                // function values are not comparable (§16.2)
```

Alternatives considered:

| Alternative | Why not |
| --- | --- |
| Keep requiring `func(...) { f(...) }` | Works today, but hides the signature and cannot express a mode-preserving forward without restating parameters |
| Make a named-function value Copy | Classification is by type; two values of the same type cannot differ in Copy/Move without encoding the origin in the type |
| Give named functions a distinct type | Breaks identity (§16.2) and prevents assigning either form to a `func(...)` parameter |

## 2. Function-type identity

No change to §16.2. A function type is its parameter types, modes, and results.
A converted declaration and a literal with the same signature have identical
types. The spawning rules below use the callee's type only for the result list,
exactly as `go f(args)` does with `f`'s declared result list (§18.8). A
function-typed callee does not change how `Task<...>` is derived.

## 3. What `go` accepts

`go` takes a call. The callee position may be:

| Callee | Today | Proposed |
| --- | --- | --- |
| Declared function or method | accepted | unchanged |
| Closure literal written in place: `go func(x int) { ... }(1)` | rejected | accepted |
| Local of function type: `go job(1)` | rejected | accepted; the local is moved |
| Field, element, or map value of function type: `go s.job(1)` | rejected | rejected in this slice (see below) |
| Result of a call: `go build()(1)` | rejected | rejected; the callee must be a name or a literal |

Restricting the callee to a local, a literal, or a declared name keeps "the
callable is moved into the task" a rule about whole bindings. Moving a closure
out of a field or element would be a partial move of a container and needs its
own decision; it can be added later without changing anything below.

## 4. Ownership and invocation of the spawned callable

**The spawned callable is moved into the task and consumed by its run.**

- Evaluation: the callee operand is evaluated first, then the arguments left to
  right (§7.5), all in the spawner, each exactly once. Only then is the task
  created. Nothing of the body runs before `go` finishes evaluating.
- The task owns the closure value from creation. It calls the closure once and
  then destroys it, so every captured value is destroyed exactly once, after
  the call returns, whether it returned normally, with an error result, or by a
  panic.
- A named-function value or a capture-free closure has nothing to destroy.
- A local used as the callee is moved: the name is unusable afterward, with the
  usual diagnostic for a use of a moved value. No call-site marker is needed,
  because the callee of `go` is always consumed, in the same way an `own`
  argument is.
- A call-once closure (§16.6) may be spawned: `go finish()` is its single
  permitted call, deferred. This is the intended use. A call-once closure is
  still bound to a single-name `let` and still cannot be passed, returned, or
  stored.
- A closure whose spawn is skipped (the spawn is never reached, or an argument
  expression panics after the callee was evaluated) is an ordinary temporary or
  local: it is destroyed once by the normal cleanup, in reverse evaluation
  order.

```ore
func work(jobs own Array<Job>) {
    let finish = func() { handle(jobs) }   // owns `jobs`; call-once
    let task = go finish()                 // valid: finish moved into the task
    task.wait()
}

func twice() {
    let log = func() { println("x") }
    let a = go log()
    let b = go log()     // rejected: `log` was moved by the first `go`
    a.wait()
    b.wait()
}
```

Alternatives considered:

| Alternative | Why not |
| --- | --- |
| Borrow the callable for the task's lifetime | Violates §18.4: the borrow's validity cannot be proven independently of the spawner, and a join on the normal path is not proof |
| Copy or clone the callable | Closures own Move captures and have no clone (§16.3); copying would duplicate destruction |
| Allow the spawner to keep using the callable | A running closure may write through its captures, so concurrent reuse would be a data race by construction |
| Let the task call a non-call-once closure repeatedly | A task is a one-shot computation (§18.8); a single run is the only behavior it has |
| Require an explicit marker such as `go own f()` | Call sites have no move markers; and a new marker is out of scope for this issue |

## 5. Ownership inference for a spawned closure

§16.4 lists the positions that make a closure owning. This proposal adds one:

- the closure literal, or a local it initializes (directly or by rebinding), is
  the callee of `go`, or is a `go` argument passed to an `own` parameter of
  function type.

A spawned closure is therefore always owning: captured Copy values are copied
at creation and captured Move values are moved in. The outer locals follow the
normal rules afterward.

**A spawned closure may not change a captured Copy value.** Assigning,
compound-updating, or passing a captured Copy local to a `mut` parameter inside
a closure that is spawned is rejected. The task would change only its own copy,
which the spawner never sees, and a change that goes nowhere is the opposite of
the visible mutation the language wants. The error is reported at the
assignment. This is stricter than §16.4 for closures in general, where a
returned counter legitimately keeps and changes its own copy over many calls; a
spawned closure runs once, so no such need exists. A task that wants a mutable
local starts from a copy (`var local = n`) inside the body, and a task that
wants to share a change uses a channel or a `mutex`. A captured Move value may
still be changed inside the task, because the spawner gave it away and cannot
use it afterward.

Capture of anything the task could not own independently is rejected, by the
rules that already apply to owning closures and task inputs:

| Capture | Result | Reason |
| --- | --- | --- |
| Copy local (`int`, `string`, a channel handle) | Accepted | copied into the environment |
| Move local, owned by the spawner | Accepted | moved into the task; unusable afterward |
| Borrowed parameter that is a Move value | Rejected | cannot move out of a borrowed parameter; the diagnostic names the capture |
| Copy local that the closure assigns, compound-updates, or passes to `mut` | Rejected | the change would be invisible outside the task |
| Local that holds a view (slice, wrapper containing a slice) | Rejected | a view is not independent storage (§11.7, §18.4) |
| `mut` parameter or exclusively captured value | Rejected | exclusive access cannot be proven to end with the spawner (§18.4 rule 4) |
| Another closure that is borrowing | Rejected | it would capture the borrowed place |
| Another closure that is owning | Accepted | moved in; the same ownership rules apply to its own captures |

The diagnostic names the capture and the reason, as the escape diagnostics for
returned closures already do.

Accepted and rejected examples:

```ore
func start(id int, name string, work own Array<int>) {
    let t = go func() {
        println(id)            // Copy, copied
        println(name)          // text is Copy
        consume(work)          // Move, moved into the task
    }()
    t.wait()
    println(work.len())        // rejected: `work` was moved into the closure
}

func borrowed(values Array<int>) {
    let t = go func() { println(values.len()) }()   // rejected: captures a borrowed parameter
    t.wait()                                        // does not make it safe
}

func view(values own Array<int>) {
    let part = values[:]
    let t = go func() { println(part.len()) }()     // rejected: captures a view
    t.wait()
}

func counting() {
    var n = 0
    let t = go func() { n += 1 }()   // rejected: a task changes only its own copy of `n`
    t.wait()
}

func countingLocal() {
    let start = 0
    let t = go func() {
        var local = start             // accepted: the task starts from its own copy
        local += 1
        println(local)
    }()
    t.wait()
}
```

The diagnostic reads, in spirit: a task gets its own copy of `n`, so changing it
here does not change it outside; use a channel or a `mutex` to share a change,
or start from a local copy inside the task.

## 6. Arguments to a spawned callable

Arguments follow §18.4 and Q25(b), keyed to the **callable's own parameter
modes**:

- a Copy argument is copied into task-owned argument storage;
- an `own` argument is moved;
- a `mut` parameter is rejected;
- a view, or a Move value for a shared parameter, is rejected.

A function value stored in an argument is rejected today because it may borrow.
This proposal relaxes that for one case, delivered in the same stage as callee
spawning (`go runWorker(handler)`): an `own` argument of function type is
accepted when the closure it holds is owning and no captured value, recursively,
holds a view. Such a closure is a Move value that owns everything it uses, so it
is independently valid. A function-typed argument for a shared parameter stays
rejected, since a call needs exclusive use that a shared borrow cannot grant
(§16.4). A closure that holds a view stays rejected.

## 7. Results and provenance

The result list of the spawned callable determines `Task<...>`, exactly as for a
declared function (§18.8). Q25(c) is unchanged: a task result cannot be a
slice, hold a view, or be a function value, and a written `Task<...>` with such
a result is rejected. A spawned closure may therefore not return a view of its
captured storage, and the region checker's existing "no view of local or
owned-parameter storage may escape" rule applies to the closure body as it does
to any function.

Returning a function value from a task is a separate relaxation (an owning
closure result is independently valid) and is left to a later proposal.

## 8. Errors, panic, detach, process exit

- Errors: a closure result that ends in `error` makes the task `Task<..., error>`
  and is retrieved by `.wait()` or `await` as for any task (§18.7). `?` inside
  the closure needs a trailing `error` result declared by the closure itself
  (§16.1).
- Panic: the closure's own cleanup runs, including destruction of the
  environment, and the panic is reported as `panic in task N: message` and
  raised again at retrieval (§18.10, Q25(f)). The environment is destroyed
  exactly once.
- Detach: `go f()` as a statement and dropping a `Task` handle detach (§18.5,
  §18.6). A detached task's closure environment and results are destroyed when
  it finishes (Q25(d)).
- Process exit: when the initial task finishes, the process exits at once. An
  environment still held by a running task is not destroyed, matching Q25(h).
  The proposal adds no cancellation and no join.

## 9. Async callables

Unchanged and separate.

- A closure body is never async (§16.1, §17.2). Writing a closure inside an
  `async func` does not make it async.
- A spawned closure runs as a **plain-function task**: one run on a runtime
  thread, with worker compensation when it blocks (§18.3, §18.9).
- `ch.receive()` and the other waiting operations inside such a closure block
  the thread, as the existing conformance row for closures in async functions
  states.
- An `async func` name is **not** a function value here. The function type
  `func(...)` does not say whether calling it is `await`ed, and encoding that
  (for example `async func(int) int`) is a new type form and a separate
  decision. `go asyncFn(x)` on the declared name is unchanged.

## 10. Methods and bound receivers

Not included. `go v.Method(args)` on a declared method stays as it is. A method
value (`v.Method` as a value) needs separate rules for whether the receiver is
borrowed, copied, or moved, for `mut` receivers, and for what the resulting
closure captures. Adding it later cannot change anything above.

## Stages and implementation plan

Stage 2, after acceptance: declared synchronous function values.

- `resolve`: allow a function name in value position; record the function id.
- `hir/lower.rs`: replace the "declared functions used as values" rejection
  (`name_expr`, `Res::Function`) with a capture-free closure value; reject
  `async` functions, methods, and built-ins with specific messages.
- `mir/lower.rs`, `codegen`: reuse the existing closure value (code pointer,
  environment pointer, destructor pointer). An empty environment has a null
  environment pointer and no destructor; this is an internal representation,
  not a language contract.
- Ownership: the value is a Move local with no loans.

Stage 3: `go` on owning callables.

- `hir/lower.rs` (`go_expr`, `spawn_inputs_are_independent`): accept a
  function-typed callee; derive input checks from the callee's type; build the
  spawn thunk from the evaluated callee and arguments.
- `hir/lower/closure_kind.rs`: add the `go` position to the owning-closure
  inference; keep call-once handling.
- `ownership/region.rs` and `ownership/checker.rs`: callee move at the spawn;
  reject captures that are views or borrowed parameters; keep reuse after the
  move an ordinary moved-value error.
- `mir/lower.rs`, `codegen/task.rs`, `runtime/src/task.rs`: the task already
  runs an owning call-once closure. The change is that the closure is the user's
  callable and its environment destructor runs after the call.

No new dependency is needed.

## Conformance cases required before implementation

These move into `tests/conformance/closures.md` and
`tests/conformance/concurrency.md` when the proposal is accepted, and each gets
an executable test with the implementing stage.

Function values (stage 2):

| Scenario | Expected result |
| --- | --- |
| `let f = add` then `f(1, 2)` | Valid; `func(int, int) int` |
| A package-qualified function, written in Zore or native, converted, then called | Valid |
| Function value passed to a `func(...)` parameter | Valid; identity by signature |
| Function value stored in a struct field, `Array<T>`, or map value | Valid (§16.4) |
| `add == add`, printing a function value, using one as a map key | Rejected (§16.2) |
| Name of an `async func` as a value | Rejected |
| A method as a value | Rejected |
| `println`, `len`, `push` as values | Rejected |
| A parameter type or mode mismatch on assignment | Rejected |
| Callee and arguments evaluated once, callee first | Valid; observable in an evaluation-order test |

Spawned callables (stage 3):

| Scenario | Expected result |
| --- | --- |
| `go func() { ... }()` capturing Copy values | Valid; values copied |
| Captured Move value | Valid; unusable in the spawner afterward |
| Closure assigns, compound-updates, or passes a captured Copy local to `mut` | Rejected, at the change |
| Closure copies a captured Copy local into its own `var` and changes that | Valid |
| `go runWorker(handler)` with an owning function-typed `own` argument | Valid; the closure moves into the task |
| The same with a function-typed shared parameter, or a closure that holds a view | Rejected |
| Captured borrowed parameter that is a Move value | Rejected |
| Captured view, `mut` parameter, or exclusive capture | Rejected |
| Spawning a closure that captures another borrowing closure | Rejected |
| `go job()` then `job()` or a second `go job()` | Rejected: use of a moved value |
| `go finish()` of a call-once closure | Valid; consumes it |
| Callee or argument expression evaluated more than once | Never; counting test |
| Environment destroyed exactly once on normal return | Counting destructor test |
| Environment destroyed exactly once on an `error` result | Counting destructor test |
| Environment destroyed exactly once on panic, with the panic reported and re-raised at retrieval | Counting destructor test |
| Closure never spawned and then dropped or out of scope | Environment destroyed once |
| Argument expression panics after the callee was evaluated | Callable destroyed once; no task created |
| Detached task with an owning closure | Environment and results destroyed when it finishes |
| Result that is a slice, a view, or a function value | Rejected |
| Closure inside an `async func` spawned with `go` | Valid; runs as a plain task, body not async |
| `await` inside a spawned closure | Rejected |
| `ch.receive()` inside a spawned closure | Valid; blocks its worker, others keep running |
| Existing `go declaredFunc(args)` and closure calls | Unchanged |

## Specification edits on acceptance

- §16.2: replace the sentence that rejects declared function names with the
  conversion rule of section 1.
- §16.4: add a note that a closure spawned by `go` may not change a captured
  Copy value; add the `go` callee/argument position to the owning list and remove
  "use with tasks (`go`) … remain unsupported" for the synchronous case.
- §16.6: state that `go` of a call-once binding consumes it.
- §18.3: `go` accepts a declared function or method, a function-typed local,
  or a closure literal; the callable is moved into the task.
- §18.4: item 5 points at the owning-closure capture table of section 5; the
  relaxation for an owning function-typed `own` argument in section 6.
- `docs/spec-questions.md`: record this as a resolved question and narrow Q25(a)
  and Q25(b).
- `docs/roadmap.md`: replace "`go` on function values and closures" under
  remaining work.

## Choices already made

These were open in the first draft and are settled in this draft, subject to
the maintainer's acceptance of the whole proposal.

1. Native standard-library functions are convertible exactly like functions
   written in Zore, through a compiler-supplied wrapper (section 1).
2. A closure is not moved out of a field or element at `go` in this change. A
   whole local or a literal is the only callee form, so no partial-move rule is
   needed. Moving out of a container can be added later without changing
   anything above (section 3).
3. An `own` function-typed argument to a spawned call is part of stage 3
   (section 6).
4. A spawned closure may not change a captured Copy value; the error is
   reported at the change (section 5).
