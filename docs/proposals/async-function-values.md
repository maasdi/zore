# Q36 proposal: `async` function values

Status: PROPOSED. Nothing here is locked, and no implementation may rely on it
until the maintainer accepts it and the decisions are written into the
specification and locked (§53). The specification stays authoritative. This is
the follow-up that Q33 left open: Q33 allows the names of declared synchronous
functions as values and rejects an `async func` name because the function type
does not say whether a call must be awaited.

Scope: a function type that says "calling this is async", and the name of a
declared `async func` as a value of that type. Out of scope: async closure
literals (§16.1 leaves them unspecified), async methods as values (Q34 rejects
them), and any change to how plain function values work.

## Why

The use cases are a table of async handlers (route to handler), a helper that
takes an async function to run (`retry`, `withTimeout`), and a list of async jobs.
Today the workaround is a plain closure that spawns the function and returns its
task: `func(id int) Task<string, error> { return go fetch(id) }`. It works, but
every call starts a task even when the caller only wants to await the result, the
type shows `Task<...>` instead of the call it stands for, and each handler needs
its own wrapper.

## Decision requested

1. A function type may begin with `async`. It means calling a value of the type
   is an async call.
2. The name of a declared `async func` (and a package-qualified one) is a value
   of the matching async function type.
3. A call through a value of async function type follows the async call contract
   (§17.8): it must be the operand of `await` or of `go`.

```ore
async func fetchUser(id int) (User, error) { /* ... */ }
async func fetchOrder(id int) (Order, error) { /* ... */ }

type Route struct {
    Path    string
    handler async func(int) (string, error)
}

async func retry(attempts int, op async func(int) (string, error), id int) (string, error) {
    for var i = 0; i < attempts; i += 1 {
        let value, err = await op(id)
        if err == nil { return value, nil }
    }
    return "", error("gave up")
}

let h = fetchUser                      // async func(int) (User, error)
let user, err = await h(7)             // valid inside an async func
let task = go h(7)                     // valid anywhere; yields Task<User, error>
```

## Rules

**Syntax and identity.** `async func(params) results` is a function type. Two
function types are identical when they have the same `async` property, parameter
types and modes, and result types (§16.2 plus the `async` property). There is no
implicit conversion between `func(T) R` and `async func(T) R` in either
direction: a synchronous function cannot be adapted without a hidden wrapper,
and an async function cannot be called without `await` or `go`.

**Making a value.** The name of a declared `async func` has the type of its
signature with `async`. The value is a capture-free closure, Move like every
function value. An `async` method and a built-in operation stay rejected.
Evaluating the name has no effect.

**Calling.** The call rules of §17.8 apply to a value of async type exactly as to
a declared async function:

- `await op(x)` is valid only in the body of an `async func` (not in a plain
  function or a closure). The callee is evaluated first, then the arguments left
  to right, each once.
- `go op(x)` is valid anywhere. It follows Q33: the callable is evaluated first
  and moved into the task, an `own` function-typed argument is accepted when it
  holds no borrow, and a function value that arrived through a parameter cannot
  be spawned (§18.4). The task type is `Task<R1, ..., Rn>` from the result list.
- A bare call, a call whose result is bound, and a call passed as an argument are
  errors, as for a declared async function.

**Ownership across suspension.** An awaited call uses the callee exclusively for
the whole call, including while the task is suspended (§16.2, §17.6). The callee
value lives in the awaiting function's pinned frame, so the existing rule for
values across `await` applies with no new case. A parameter of async function
type may be awaited repeatedly, because each `await` is a separate exclusive use.

**Where the type may appear.** Everywhere a function type may (§16.4): results,
struct fields, `Array<T>` and map values, `own` parameters. Never a slice
element or a map key. A shared parameter whose type holds a function value is
rejected, as today.

**Errors, panic, detach, exit.** As for a declared async function. A panic in an
awaited call unwinds through the awaiting task. A panic in a spawned call follows
Q25(f). Dropping the task handle detaches it. The callable's environment is
destroyed exactly once.

## Accepted and rejected

```ore
let a = fetchUser                           // async func(int) (User, error)
let b func(int) (User, error) = fetchUser   // rejected: a plain func type cannot hold an async function
let c async func(int) (User, error) = plainLookup  // rejected: no adaptation
let d = a(7)                                // rejected: async call neither awaited nor spawned
func plain() { let u = await a(7) }         // rejected: await outside an async func
let e = func() { await a(7) }               // rejected: a closure body is not async
```

## Alternatives considered

| Alternative | Why not |
| --- | --- |
| Convert an async function to `func(...) Task<...>` implicitly | Hides a spawn in every call, loses the ability to await inline, and changes the meaning of the name |
| One function type that can hold either, deciding at run time | Breaks §17.8: the compiler can no longer classify a call by its callee's `async` property, and a bare call could not be rejected |
| Allow a plain function value where an async type is expected | Needs a hidden wrapper that manufactures an async function; the author can write the `async func` instead |
| Async closure literals in the same change | §16.1 leaves them out on purpose; captures of a suspended closure need their own decision |
| Another spelling, such as `func async(...)` | The declaration is written `async func name(...)`; the type mirrors it |

## Compiler impact

- `parser`: the type grammar accepts a leading `async` before `func`.
- `types`: `FuncSignature` gains an `is_async` property that is part of type
  identity. Display names and error messages show it.
- `hir/lower.rs`: `function_value` stops rejecting an `async func` and builds the
  forwarding closure with the async property. A call through a value is
  classified by the callee type's `async` property, and the §17.8 checks (must be
  `await` or `go`) apply to it. `go_expr` already accepts a function-typed
  callee; it additionally records the callee's async property for the task body.
- `mir` and `async_lowering`: a new suspension kind for an awaited call through a
  value, next to the existing call, task, channel, sleep, mutex, and I/O kinds.
  It creates the callee's frame through the value's code pointer and polls it as a
  child, as an awaited declared call does through its known constructor and poll
  function.
- `codegen`: an async function value's code pointer is the callee's frame
  constructor, taking the environment and arguments and returning a frame that
  carries its poll and destroy functions. The generated forwarding closure is an
  internal async function with no captures; it is not an async closure literal and
  adds nothing to the source language. This representation is not a source
  contract.
- `runtime`: no change expected, because a task is already a frame with a poll
  function. `go` on an async value wraps the constructed frame in a task as `go`
  on a declared async function does.
- Ownership and region analysis: no new rule. The callee is an ordinary Move local
  used exclusively across a suspension point.

## Staging

1. Spec and conformance cases (this document, then the locked spec text).
2. Types, parsing, and checking: the `async func` type, converting an async
   function name to a value, and the call contract. Reports a clear
   "not yet supported by the code generator" error for a program that would need
   code generation, so the checker never accepts what the backend cannot build.
3. Code generation for `await` through a value.
4. `go` on an async value.

Each stage is its own PR, passes the full check list, and leaves the previous
behavior intact.

## Conformance cases required before implementation

| Scenario | Expected result |
| --- | --- |
| `let h = fetchUser` | Type `async func(int) (User, error)` |
| `await h(7)` inside an `async func` | Valid |
| `go h(7)` in a plain function and in an async function | Valid; `Task<User, error>` |
| `h(7)` as a statement, bound, or passed as an argument | Rejected: neither awaited nor spawned |
| `await h(7)` in a plain function or a plain closure | Rejected |
| `async func(T) R` assigned to `func(T) R`, and the reverse | Rejected |
| Async function value in a struct field, `Array<T>`, map value, or `own` parameter | Valid |
| Async function value as a slice element or map key | Rejected |
| Parameter of async function type awaited twice | Valid |
| `go op(x)` where `op` is a parameter | Rejected: came in through a parameter |
| Package-qualified `async func` as a value | Valid |
| Async method, built-in, or closure literal as an async value | Rejected |
| Callee and arguments evaluated once, callee first | Evaluation-order test |
| Environment destroyed once on return, error, panic, and when unused | Counting destructor tests |
| A suspended awaited call through a value keeps the callee exclusive | Rejected use of the callee meanwhile |
| Existing plain function values and `go` forms | Unchanged |

## Specification edits on acceptance

- §16.2: add the `async` function type and its identity rule, and the async
  function value rule.
- §17.8: state that a call through a value of async type is an async call.
- §17.2: add the type form to the async function syntax section.
- §18.3: `go` accepts an async function value as the callee.
- `docs/spec-questions.md`: record Q36 as resolved.
- `docs/roadmap.md`: remove async callables from the remaining list.

## Open points for the maintainer

1. Should async method values be included now? The proposal says no, because the
   receiver would be captured by a closure that is live across a suspension, which
   needs the conservative borrow rule of §17.6 spelled out for captures.
2. Should async closure literals be specified next, or never? The type form here
   leaves room for them without changing anything above.
3. Is `async func(...)` the spelling you want for the type?
