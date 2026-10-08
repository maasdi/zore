# Q35 proposal: `go` on a callee stored in a struct field

Status: PROPOSED. Nothing here is locked, and no implementation may rely on it
until the maintainer accepts it and the decisions are written into the
specification and locked (§53). The specification stays authoritative. This is
the follow-up that Q33 deferred: §18.3 says a callee that is a field, element,
map value, or call result is rejected.

Scope: letting the callee of `go` be a function-typed **field path rooted at a
local**, such as `(w.run)(5)`. Out of scope: array and map elements, call
results, method values (decided in Q34), and any new syntax.

## Baseline today

- `go` takes a declared function or method, a closure literal, or a
  function-typed local (Q33). The callable is moved into the task.
- A struct may hold a function value in a field, and `(w.run)(5)` calls it.
- The language already allows moving one Move field out of a struct, leaving the
  struct partially moved, unless a containing value has a custom `drop` (§31.2).
- A closure can be taken out of an `Array<func(...)>` with `pop` or out of a map
  with `remove`, which return the value as a plain local.

## Decision requested

**`go` may take a field path rooted at a local as its callee. The field is
moved out of its struct into the task, by the rule that already exists for
moving a field.** No new ownership concept is added: the callee operand is a
move of that place, exactly as `let run = w.run` would be.

```ore
type Worker struct {
    Name string
    run  func(int) int
}

func start(w own Worker) {
    let t = go (w.run)(5)     // `run` moves into the task; `w` is partially moved
    println(w.Name)           // valid: another field is still available
    println(t.wait())
}
```

Rules:

- **Callee form.** A name, or a chain of field selections ending at a
  function-typed field, rooted at a local (`(w.run)`, `(a.b.run)`).
- **Move.** The field is moved into the task when the task is created. Using
  `w.run` again is a use of a moved value. Using `w` as a whole is rejected
  until `run` is assigned again (§31.2). Reinitializing `w.run` after the spawn
  is allowed and creates a fresh, unrelated callable.
- **When the move is refused.** Where §31.2 refuses to move the field: if any
  containing value along the path has a custom `drop`; if the root is a borrowed
  parameter (a shared or `mut` parameter cannot give up what it does not own);
  or if the root is a loop item.
- **Everything else is Q33.** The task owns the closure, calls it once, and
  destroys it exactly once on a normal return, an error result, or a panic.
  Arguments follow §18.4, and a closure stored in a field is already owning
  (storing it in a field is an escape position in §16.4), so it holds no borrow.
- **Evaluation order.** The callee is evaluated first (moved), then the
  arguments left to right, each once, then the task is created. If an argument
  panics after the field was moved, the moved closure is destroyed once and no
  task is created.
- **Elements and call results stay rejected.** `go (jobs[0])()`, `go table["a"]()`
  and `go build()()` are rejected. A computed index cannot be partially moved
  (§31.2), and a call result is not a place. Take the closure out into a local
  first:

```ore
var jobs = Array<func() int>{}
jobs.push(compute)
let found, job = jobs.pop()
if found {
    let t = go job()
    println(t.wait())
}
```

## Accepted and rejected

Accepted:

```ore
let w = Worker{Name: "a", run: double}
let t = go (w.run)(21)

type Pipeline struct { stage Stage }
type Stage struct { run func() }
func f(p own Pipeline) { go (p.stage.run)() }
```

Rejected:

```ore
func f(w Worker) { go (w.run)(1) }               // `w` is borrowed; cannot move out of it
func g(w own Worker) {
    go (w.run)(1)
    go (w.run)(2)                                // second use of a moved field
}
let t = go (jobs[0])()                           // element of an array
let u = go (table["a"])()                        // map value
let v = go build()()                             // call result
type Guarded struct { run func() }
func (g mut Guarded) drop() {}
func h(g own Guarded) { go (g.run)() }           // `Guarded` has a custom drop
```

## Alternatives considered

| Alternative | Why not |
| --- | --- |
| Borrow the field for the task | Violates §18.4: the borrow cannot be proven valid for the task's life, and a join is not proof |
| Copy the closure out of the field | Closures own Move captures and are not clonable |
| Allow array and map elements with an implicit removal | Hides a removal behind `go`; the language has explicit `pop` and `remove` for this and no hidden mutation |
| Allow call results (`go build()()`) | It has no source place to move from, and it is the same as binding the result to a local first |
| Require the whole struct to move | Needlessly forces the caller to give up fields the task does not use |

## Compiler impact

- `hir/lower/spawn.rs`: in `spawn_callable`, accept a callee that is a field path
  rooted at a local. Today it accepts `Local` and a closure literal only. The
  callee expression becomes the first spawn argument, as a place read.
- MIR: the first spawn argument is already lowered to an operand and moved into
  the thunk's environment. A field of Move type lowers to a move of that place,
  the same operand `let run = w.run` produces. No MIR change is expected.
- Ownership: the existing partial-move analysis handles the moved field, its
  reinitialization, and the custom-`drop` ancestor restriction. No change is
  expected there either.
- `closure_kind.rs`: the callee is not a local, so there is no spawned-local to
  mark. A closure stored in a field is already owning. The only change is to
  let the first spawn argument be a field path where it today expects a local or
  a literal.
- The "callee that is a field, element, map value, or call result" rejection in
  `go_expr` narrows to element, map value, and call result.

## Conformance cases required before implementation

| Scenario | Expected result |
| --- | --- |
| `go (w.run)(5)` with `w` owned and `run` a function field | Valid; `run` moves into the task; `w.Name` still readable |
| Using `w.run` after the spawn | Rejected: use of a moved value |
| Using `w` as a whole after the spawn | Rejected until `run` is reassigned |
| Assigning `w.run` after the spawn, then using it | Valid; independent of the task |
| Nested field path `(a.b.run)` | Valid |
| Root is a shared or `mut` parameter, or a loop item | Rejected |
| A containing value has a custom `drop` | Rejected |
| Array element, map value, or call result as callee | Rejected |
| Closure taken from an array with `pop`, then spawned from the local | Valid |
| Environment destroyed once on return, error, and panic | Counting destructor tests |
| Argument expression panics after the field was moved | Closure destroyed once; no task created |
| The stored closure captured a view | Rejected: a spawned task cannot hold a borrow |
| Existing `go f(x)`, `go job()`, and closure literal callees | Unchanged |

## Specification edits on acceptance

- §18.3: replace "A callee that is a field, element, map value, or the result of
  a call is rejected" with the rule above: a field path rooted at a local is
  moved; an element, map value, or call result is rejected.
- `docs/spec-questions.md`: record Q35 as resolved.
- `docs/roadmap.md`: remove "`go` on callees stored in fields" from the
  remaining list.

## Open points for the maintainer

1. Should a loop item or a `mut` parameter be a valid root if the field is
   reassigned before the function ends? The proposal says no, because the
   language has no rule for moving out of borrowed storage and temporarily
   refilling it.
2. Should array and map elements ever be allowed directly? The proposal says no,
   so removal stays visible in the source (`pop`, `remove`).
