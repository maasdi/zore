# Q34 proposal: method values

Status: PROPOSED. Nothing here is locked, and no implementation may rely on it
until the maintainer accepts it and the decisions are written into the
specification and locked (§53). The specification stays authoritative. This is
the follow-up that issue #52 and Q33 left open: Q33 allows only the names of
declared functions as function values and says methods and bound receivers need
their own rules.

Scope: using `value.Method` as a function value. Out of scope: method
expressions written on the type (`Counter.read`), `async` methods as values,
and any new syntax.

## Baseline today

- `go value.Method(args)` works (a declared method call).
- `value.Method` without a call is rejected, with a message that wrongly says
  the type has no such field.
- A closure literal can already do the same job by hand, and the existing
  capture rules decide what it borrows, copies, or moves (§16.3, §16.4).

## Decision requested

**A method value is exactly the closure literal that calls the method.**
`value.Method` means

```ore
func(params) results { return value.Method(params) }
```

with the parameter list and results of the method (the receiver is not a
parameter), written by the compiler. No new capture, ownership, or lifetime rule
is introduced: the receiver is captured the way the closure literal would
capture it, by the rules that exist.

```ore
type Counter struct { N int }

func (c Counter) read() int { return c.N }
func (c mut Counter) bump() { c.N += 1 }
func (c own Counter) finish() int { return c.N }

func main() {
    var counter = Counter{N: 1}

    let read = counter.read       // func() int; shared capture of `counter`
    println(read())               // 1
    let bump = counter.bump       // func(); exclusive capture of `counter`
    bump()
    println(counter.N)            // rejected: `bump` is used again below
    bump()

    let done = counter.finish     // consumes `counter`; call-once
    println(done())
}
```

What follows from that one rule:

| Receiver mode | Capture of the receiver local | Closure kind |
| --- | --- | --- |
| Shared (`c Counter`) | Shared borrow | Borrowing, unless it escapes |
| `mut` | Exclusive borrow; the local must be a mutable place | Borrowing, unless it escapes |
| `own` | The closure owns the value; the local is unusable afterward | Call-once (§16.6) |

When the method value escapes (returned, stored, passed to an `own` parameter, or
spawned by `go`), §16.4 makes it owning: a Copy receiver is copied at creation, a
Move receiver is moved in. Nothing else about spawning changes: a spawned
method value follows §18.4 as any spawned closure does, so a `mut` receiver on a
Copy local is rejected ("the task would change only its own copy") and a
borrowed Move receiver cannot move into the task.

## Details

- **Receiver expression.** The receiver must be a name or a field path rooted at
  a local, because capture is per whole local (§16.3). A call result, an index
  expression, or a map lookup is rejected as a receiver of a method value; bind
  it to a local first.
- **When the receiver is read.** A borrowing method value refers to the receiver
  in place, like any borrowing closure, so a call sees the receiver's current
  value. An owning method value holds the value it captured at creation. This is
  the same difference the closure rules already have.
- **Evaluation order.** Creating the value evaluates nothing but the capture.
  Calling it follows §16.2: callee first, then arguments, once each.
- **Type.** The function type is the method's signature without the receiver,
  modes included. It is identical to the type of the written literal.
- **Cleanup.** Exactly the closure rules: a borrowing method value owns nothing;
  an owning one destroys its captured receiver once, when the value is
  destroyed, called as call-once, or consumed by a task.
- **Errors and panics.** As for any call: a panic or `?` in the method unwinds
  through the method value's caller; a method that returns `error` gives a
  function value whose last result is `error`.
- **Async.** A method declared `async` is rejected as a value, for the same
  reason Q33 rejects an `async func` name: the function type does not say a call
  must be awaited.
- **Exported names.** A method of another package is usable as a value exactly
  where a call is allowed (§3.20).

## Accepted and rejected

Accepted:

```ore
let read = counter.read                  // shared capture
let bump = counter.bump                  // exclusive capture
let task = go func() int { return read() }()   // read moves into the task
func apply(f func() int) int { return f() }
println(apply(counter.read))
```

Rejected:

```ore
let a = Counter{N: 1}.read               // receiver is not a local
let b = makeCounter().read               // receiver is a call result
let c = list[0].read                     // receiver is an index expression
var d = Counter{N: 1}
let e = d.bump
println(d.N)                             // `e` holds `d` exclusively and is used below
e()
let f = client.fetch                     // `fetch` is declared `async`
```

## Alternatives considered

| Alternative | Why not |
| --- | --- |
| Bind a copy of the receiver at evaluation, as Go does | Hides a copy of a Move value or silently detaches a `mut` receiver from the original; the language has no such implicit copies |
| Always bind by borrow | Would force every spawned or returned method value into a separate, weaker rule; the existing escape inference already decides this |
| Always bind by move | Makes `counter.read` unusable afterward even for a plain read, which is more surprising than a shared loan |
| Method expressions (`Counter.read` taking the receiver first) | A different feature: it needs a rule for the receiver as the first parameter and its mode. It can be added later without changing anything here |
| A new capture marker | Explicitly out of scope for issue #52 and against the language's no-markers principle |

## Compiler impact

- `hir/lower.rs`, selector handling: when a selector names a method and is not
  being called, build a closure whose body is the method call. This is the
  forwarding closure already built for function values, with one captured local
  for the receiver. The existing capture inference, exclusivity marking, and
  closure-kind inference then apply unchanged.
- `resolve`: record the receiver local as a capture of the generated closure.
  This is the only part that needs care, because the resolver today records
  captures for written literals.
- Replace the misleading "no field" message with a precise rejection for the
  rejected receivers above.
- No change to MIR, ownership, or code generation: the result is an ordinary
  closure.

## Conformance cases required before implementation

| Scenario | Expected result |
| --- | --- |
| `let read = c.read` then `read()` | Valid; shared capture; `c` readable meanwhile, not writable while `read` is live |
| `let bump = c.bump` then `bump()` | Valid; exclusive capture; `c` unusable meanwhile |
| `own` receiver method value | Valid; call-once; `c` unusable afterward; a second call is rejected |
| Method value returned or stored | Owning; receiver copied or moved at creation |
| `go` on a method value held in a local | Valid; moves into the task; `mut` receiver on a Copy local rejected; borrowed Move receiver rejected |
| Method value passed to a function-typed parameter | Valid |
| Receiver that is a field path of a local | Valid; whole local captured |
| Receiver that is a call result, index, or map lookup | Rejected |
| Method of another package, exported / not exported | Valid / rejected |
| Method declared `async` | Rejected |
| Result type, parameter modes | Same as the literal's |
| Receiver destroyed exactly once for owning values on return, error, panic, and unused | Counting destructor tests |

## Specification edits on acceptance

- §16.2: extend "Declared functions as values" with the method-value rule.
- §8 (methods): state that `value.Method` without a call is a method value,
  with the table above.
- §16.4 and §18.4: no change; they already cover escaping and spawning.
- `docs/spec-questions.md`: record Q34 as resolved.
- `docs/roadmap.md`: remove method values from the remaining list.

## Open points for the maintainer

1. Should a field path receiver (`s.inner.read`) be allowed, or only a plain
   local? The proposal allows field paths because capture of the whole root local
   already works that way; a plain-local-only rule is simpler to explain.
2. Should method expressions (`Counter.read`) be a separate follow-up, or never?
