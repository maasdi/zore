# Open specification questions

This register does not amend the language specification. Open entries are unresolved;
they distinguish source-language questions from internal implementation choices.
Resolve affected language rules through spec §53 before treating them as locked.
Unrelated infrastructure work can proceed.

## Implementation priority

The project's current direction is to defer decisions until needed by the
active compiler milestone. M0–M4 are implemented; Q01 is resolved, and Q13/Q14
record the conservative lexer and parser choices made where the spec is
silent. Q05a resolves the entry-point and `println` subset needed by the
semantic target. See `roadmap.md` for stage gates.

The entry-point and `println` subset of Q05 needed by the first semantically
checked/runnable target is resolved (Q05a); resolve further Q05 items only when
a stage needs them. Remaining string operations, closures, imports, iteration, borrowed
map-entry APIs, and concurrency APIs stay open until their consuming stage.
Internal implementation choices need rationale and tests, not language approval.
A newly discovered semantic gap blocks its affected feature, not unrelated work.


| ID | Question / missing detail | Reference | Needed before |
| --- | --- | --- | --- |
| Q01 | Listed lexical choices and `_` target forms are resolved; see Q01a–o below. Explicit error discards are permitted by §15.6. This does not imply a complete formal grammar. | §3.5–3.18, §5.5, §15.6, §4.1, §41.1 | Affected lexer/parser/resolution behavior |
| Q02 | Remaining grammar for strings and full `go` expressions; collection loops are resolved in §5.10 (Q02h); closures and function types are resolved in Q02g, with owning and call-once closures in Q02i. Map borrowed entry APIs remain Q05. Arrays/indexing/slicing are resolved in Q02e and map construction/lookup/assignment/removal in Q02f; assignment, operators, calls, and struct construction are also locked. | §5.6, §7.5–7.8, §8.4, §41.1 | Expression parser/lowering |
| Q03 | Core bindings, blocks, conditionals, loops, scope-entry points, returns, result forwarding, and expression-statement policy are resolved. Detailed `?` typing is resolved (Q06b); task retrieval forms are resolved (Q09a); collection forms remain in Q02. | §5.4–5.10, §7.7–7.8, §41.2–3 | Statement parser/typing |
| Q04 | Zero/resource interaction resolved in Q04e; see Q04a–d for earlier decisions. Shift contradiction resolved in Q11. This does not imply a complete collection/task expression grammar (Q02) or predeclared conversion API (Q05). | §5.3–5.4, §6.5–6.6, §7.6, §10.2, §19.8, §41.4–5 | Type checking and runtime semantics |
| Q05 | Local package discovery/import mapping (resolved in Q24), complete predeclared API inventory, initial standard-library signatures beyond `println` (strings and strconv resolved in Q24; float text resolved in Q43), package variable initialization order (resolved for package-level `let` in Q41), and remaining type-layout validity rules. The entry-point contract and `println` are resolved in Q05a. Functions/types support forward references and method conflicts are defined (§7.8). Registry/solver remain out of MVP. | §3, §5.7, §7.8, §37, §42, §45 | Resolution/package checking and first native example |
| Q06 | Resolved; see Q06a–b below. Error wrapping/cause chains, sentinel error declarations, structured error payloads, and the full predeclared API remain open under Q05. | §5.5, §7.2, §15, §22.2 | Error checking/lowering |
| Q07 | Refined by Q07b: destructor-safe partial moves and recursive borrow contracts; see Q07a for earlier decisions. Array/slice syntax is locked in Q02e; map operations are locked in Q02f; string access and borrowed map-entry APIs remain Q02/Q05. | §5.6, §11.6–11.7, §12.5, §31.2 | Affected ownership analysis |
| Q08 | Drop/zero and partial-move interactions refined by Q04e/Q07b; clone precedence corrected in Q12. See Q08a for earlier decisions. Panics inside spawned tasks are resolved in Q09a. | §8.3, §10.7, §14.3–14.4, §15.4 | Destruction and runtime |
| Q09 | Lifetime proof refined by Q09b; see Q09a for runtime decisions. The full `go` expression grammar remains in Q02; the entry-point signature and exit status are resolved in Q05a (§3.19). | §6.3, §17.8, §18.8–18.11, §19.11–19.13, §20.2, §36.2, §41.4 | Tasks/channels/runtime |
| Q10 | The maintainer accepted the opt-in lexical scope design direction in `docs/proposals/scoped-tasks.md`. Exact syntax and semantics remain unresolved until a specification and conformance update locks them; the feature is not implemented. Ordinary tasks retain their locked detach and process-exit behavior. Q09b already rejects task-local spawned borrows, so Q10 is not required for current MVP safety. | §16.4, §17.8, §18.4–18.11 | Task borrowing and exit-cleanup guarantees |
| Q11 | Resolved: typed shifts discard high bits without overflow panic; counts remain checked. Untyped constants preserve exact values. See Q11 below. | §6.6 | Numeric checking/lowering |
| Q12 | Resolved: compiler instructions now select custom clone before structural cloning, with no fallback from an invalid custom method. See Q12 below. | §10.7 | Clone resolution |
| Q13 | Lexical gaps found while implementing M2: (a) §3.7 says horizontal whitespace is insignificant but never lists the characters; the lexer accepts only space, tab, and CR and rejects form feed, vertical tab, NBSP, other Unicode spaces, and a leading BOM as unexpected characters. (b) §7.6 excludes `++`/`--` and requires lexing to distinguish them from adjacent signs; the lexer rejects every adjacent `++`/`--`, so `a--b` and `x - -y` written as `x--y` are errors, and a space or parentheses is required. (c) Following §3.7's definition of newline as LF, a lone CR inside a double-quoted string or rune literal is accepted as content, not rejected as a physical newline. All three are conservative or follow the spec text literally; confirm or revise through §53. | §3.7–3.10, §7.6 | Lexer changes only; not blocking M3 |
| Q14 | Parser choices made in M3–M4 where the grammar is silent; each rejects rather than guesses and can be relaxed later. (a) `value?.field` and `value?()` are rejected, following the §7.6 table literally (postfix `?` is below calls/fields); write `(value?).field`. (b) Result lists: `(T)` with one type, `()`, and a trailing comma are rejected; §7.8 allows trailing commas only in parameter and argument lists. (c) Expression statements must be calls, optionally wrapped in `await`/`?`; `await task` alone is rejected (use `_ = await task`), since §7.8 names "call-based forms". (d) Struct literals need parentheses in a counting loop's update clause as well as its condition, because the update also precedes the body brace; §8.4 names only the condition. (e) Imports must precede other declarations; grouped `import (...)` and raw-string paths are rejected. (f) Parenthesized assignment targets such as `(a) = 1` and empty statements (`;;`) are rejected. (g) `let _ T = value` is accepted as the single-target typed form. Package-level `let`/`var` are parsed; their meaning remains Q05. | §5.4–5.6, §7.6, §7.8, §8.4, §3.3 | Parser changes only; not blocking resolution |
| Q15 | Resolved: untyped constants follow Go's model (§6.7). See Q15 below. | §6.5–6.7, §5.3 | Checker and constant-evaluation changes |
| Q16 | Slice choices made while implementing §12 where the text is silent; each rejects rather than guesses and can be relaxed later. (a) No implicit `mut []T` → `[]T` conversion: passing a mutable view where a shared one is expected is a type mismatch; write `s[:]` for a shared subslice. (b) Copying a `let`-bound `mut []T` (an exclusive reborrow, §12.3) is allowed; §12.6 restricts only passing it onward as a `mut []T` argument. (c) A `mut` mode on a slice-typed parameter (`s mut mut []T`, a mutable borrow of the descriptor) is rejected as unsupported. (d) A `mut []T` parameter is a mutable place for passing onward (§11.6), but its descriptor is not assignable, like any non-`mut` parameter (§7.3). (e) The parser reads `name mut []T` as a parameter of type `mut []T` (§12.2), not a `mut` mode applied to `[]T`, while `name mut [T; N]` stays a `mut` mode. (f) Constant indices and bounds that are negative are rejected for slices even though the length is unknown, since no length makes them valid. | §7.3, §11.6, §12.2–12.3, §12.6 | Parser, checker, and ownership changes only |
| Q17 | `Array<T>` choices made while implementing §12.6. (a) The parser recognizes `Array<` by spelling, in type and expression position: `Array` is predeclared and cannot be shadowed (§3.18), and without this `Array<int>{...}` is ambiguous with comparisons. Other `Name<...>` forms are rejected (`Task<...>` waits for M25). (b) A `>>`, `>=`, or `>>=` token is split when it closes a type argument list, so `Array<Array<int>>` closes both lists. (c) Resolved and locked in §3.7: the `>` that closes a type argument list is an eligible ending token, so a newline after `Items Array<int>` ends the field. The comparison `>` stays ineligible. The parser applies this rule at the closing `>`, because the lexer cannot distinguish it. (d) Like `[T; N]{...}`, an `Array<T>{...}` literal needs no parentheses in `if`/`for` headers. (e) Elements are destroyed in reverse index order, as for fixed arrays. (f) When an element's or field's old value panics during replacement, the new value is still stored before unwinding, so each slot always holds exactly one live value (§5.6 leaves this cleanup order open). The existing abort under a custom-`drop` ancestor is unchanged. (g) Allocation failure aborts the process. (h) The "statically known" length in §12.6 means a length known from the type, so out-of-range constant indices into `Array<T>` panic at run time, while negative ones are rejected. (i) Resolved in §12.7: `len`, `push`, and `pop`; capacity APIs and removal from the middle remain open, and other method calls on `Array<T>` are rejected with a note. | §3.7, §3.18, §5.6, §10.5, §10.7, §12.6, §14.5 | Parser, checker, and codegen changes |
| Q18 | Map choices made while implementing §13.3. (a) A map entry cannot be one of several assignment targets, even when the key expressions differ: §13.3 requires rejection when independence cannot be proven, and the compiler proves none, so it rejects every such target. (b) Destroying a map destroys its remaining values in the runtime's entry order, which §13.3 leaves unspecified. (c) A duplicate key found at run time in a literal panics with "duplicate key in map literal". The entry's value is destroyed by ordinary cleanup, as are the entries built so far. (d) Allocation failure aborts the process, as for `Array<T>` (Q17g). (e) `remove` on a map reached through a shared or `let` place uses the existing `mut`-argument diagnostic, since its receiver is `mut`. | §13.3 | Checker, runtime, and codegen changes |
| Q19 | Clone choices made while implementing §10.7 where the text is silent; each rejects rather than guesses and can be relaxed later. See Q19 below. | §8.3, §10.7, §11.7 | Checker and codegen changes only |
| Q20 | Choices for `mut []T` held in struct fields and fixed arrays (§11.7, §12.3); each rejects rather than guesses. See Q20 below. | §11.7, §12.3, §12.5, §10.7 | Checker and ownership changes only |
| Q21 | Choices for types whose custom `drop` can read a borrowed view (§11.7, §14.3); each rejects rather than guesses. See Q21 below. | §11.7, §14.3, §14.5 | Ownership changes only |
| Q22 | Output provenance: views stored through `mut` parameters and closure captures, and views returned through function values (§11.7, §16). See Q22 below. | §11.7, §16.3–16.4 | Ownership changes only |
| Q23 | Strings: byte length and indexing, boundary-checked slicing, loops by character, `string(rune)`, and when runtime-built strings are released (§6.8, §41.5). See Q23 below. | §6.8, §41.5 | Checker, codegen, and runtime changes |
| Q24 | Projects, packages, and imports: folders as packages, import paths, export checks, and the first standard packages (§3.20, §37.2). See Q24 below. | §3.20, §4.1, §37.2 | Loader, resolver, checker, and codegen changes |
| Q25 | Task implementation choices made while implementing §17–18 where the text is silent; each rejects rather than guesses and can be relaxed later. See Q25 below. | §17.8, §18.4, §18.8–18.11 | Parser, checker, codegen, and runtime changes |
| Q26 | Channel implementation choices made while implementing §19 where the text is silent; each rejects rather than guesses and can be relaxed later. See Q26 below. | §19.2–19.13 | Parser, checker, codegen, and runtime changes |
| Q27 | Async I/O choices made while implementing §37.3 where the text is silent; each rejects rather than guesses and can be relaxed later. See Q27 below. | §37.3, §36.2 | Library, checker, codegen, and runtime changes |
| Q28 | Mutex choices made while implementing §20.2 where the text is silent; each rejects rather than guesses and can be relaxed later. See Q28 below. | §20.2, §10.3, §3.18, §41.4 | Parser, checker, codegen, and runtime changes |
| Q29 | `select` choices made while implementing §19.14 where the text is silent. See Q29 below. | §19.14, §3.17 | Lexer, parser, checker, codegen, and runtime changes |
| Q30 | Byte-array API choices made while implementing §37.2–37.3 where the text is silent; revised by Q37. See Q30 below. | §37.2, §37.3, §41.5 | Checker, codegen, and runtime changes |
| Q31 | Time-limit and cancellation choices made while implementing §37.3–37.4 where the text is silent; revised by Q37. See Q31 below. | §37.3, §37.4 | Runtime and bundled-package changes |
| Q32 | Async functions as state machines without fibers, and what that does and does not change in the source language. See Q32 below. | §17.3, §17.7, §18.3, §20.2, §35.1, §37.3 | Compiler and runtime changes |
| Q33 | Declared function values and `go` on owning callables: locked in §16.2, §16.4, §16.6, §18.3, and §18.4. See Q33 below. | §16.2, §16.4, §16.6, §18.3, §18.4 | Parser, checker, ownership, and codegen changes |
| Q34 | Method values: locked in §16.2 and §9.1. See Q34 below. | §9.1, §16.2, §16.3 | Checker |
| Q35 | `go` on a callee stored in a struct field: withdrawn; the two-line form works. See Q35 below. | §18.3, §31.2 | None |
| Q36 | `async` function values: locked in §16.2, §17.2, §17.8, and §18.3. See Q36 below. | §16.2, §17.2, §17.8, §18.3 | Parser, types, checker, codegen |
| Q37 | Standard library refactor: one set of package names, signatures, and conventions across §37.2–37.5. See Q37 below. | §3.20, §37.2–§37.5 | Loader, async lowering, codegen, runtime, and bundled-package changes |
| Q38 | The `panic` call: locked in §3.17, §7.7, and §15.4. See Q38 below. | §3.17, §7.7, §15.4 | Resolver, checker, MIR, codegen, and runtime |
| Q39 | Rune conversions: locked in §6.6. See Q39 below. | §6.5, §6.6, §6.8 | Checker and codegen |
| Q40 | Named types: locked in §8.5, §3.20, §9.1, and §41.4. See Q40 below. | §3.20, §8.5, §9.1, §41.4 | Parser, resolver, checker, MIR, and codegen |
| Q41 | Package-level `let`: locked in §3.20 and §3.21; resolves the package initialization order part of Q05. See Q41 below. | §3.20, §3.21 | Resolver, checker, MIR, codegen, and runtime |
| Q42 | Interfaces locked and implemented in §22.2, with §3.16, §3.17, §10.3, §15.1, and §41.4; generic functions and struct types locked and implemented in §22.1, with §3.17, §3.18, §40, and §41.9. The proposal is `docs/proposals/interfaces-and-generics.md` (issue #88). See Q42 below. | §22.1, §22.2, §15.1, §40, §41.9 | Parser, resolver, types, checker, MIR, ownership, codegen, and the standard library |
| Q43 | Float text: locked in §37.1 and §37.2, with `zore/math`. See Q43 below. | §37.1, §37.2, §6.7 | Runtime, codegen, and bundled-package changes |

## Resolved decisions

- **Q15 — Untyped constants:** locked in §6.7 and amended §6.5–6.6 at the
  maintainer's direction, following the Go specification (checked against go1.27).
  Untyped integer and float kinds; mixed operands give float. Integer `/`
  truncates; `%` is integer-only; constant division by zero is an error.
  Untyped bitwise operators use infinite-precision two's complement (`^x` is
  `-x - 1`). Floats are representable if they round to the type without
  overflow (ties to even, negative zero to positive zero); integer types need
  an integral value in range. Constant conversions need representability, so
  `int64(2.5)` is invalid (runtime conversion still truncates). Integral
  untyped float constants may be shift operands. Implementations must support
  at least 256-bit integer constants and 256-bit-mantissa float constants.
  Zore keeps typed rune/bool/string literals, unlike Go. Implemented by
  `compiler/src/types/constant.rs` and `compiler/src/types/bignum.rs`; covered by `tests/typecheck/check.rs`.

- **Q05a — Entry point and `println`:** locked in §3.19 and §37.1 at the
  maintainer's direction. `package main` requires exactly one `func main()` with no
  receiver, parameters, results, or `async`; normal return exits 0 and an
  initial-task panic or abort exits nonzero (value implementation-defined).
  `println` is a compiler-known call taking exactly one argument of type bool,
  integer, float, rune, or string, writing its text and a line feed to stdout;
  it has no results, is not a value, panics on write failure, and does not
  suspend. Each line is written as one unit, and a blocked write keeps the
  §18.9 progress guarantee. Float text format remains open under Q05. Pending cases:
  `tests/conformance/entry-point.md` and `tests/conformance/println.md`.

- **Q22 — Output provenance:** a function may store a view into a `mut`
  parameter. Its contract now records, besides each result's origins, which
  inputs' storage or views each `mut` parameter may receive, and each call
  applies it to the argument, as for results. (a) A stored view must borrow
  storage from outside the function: a parameter's storage or views, never a
  local or `own` parameter. (b) A view stored through a mutable slice, directly
  or by a callee, belongs to the slice's backing: every owner the slice
  exclusively borrows from now holds it too. A parameter or capture holding a
  `mut []T` whose elements can hold views is an output like a `mut` parameter,
  even when passed by value, so the stored view must borrow storage from
  outside the function. (c) A call through a function value cannot know the
  callee, so its results and its `mut` arguments are treated as borrowing from
  every argument and from everything the closure captured; this allows function
  types to return views. (d) A closure may store into a captured local only
  views of other captured locals; the outer local is treated as borrowing them
  from the closure's creation. Storing a view of the closure's own parameters
  into a capture is unsupported. Pending cases: `tests/conformance/ownership.md`.

- **Q21 — Destructors that observe views:** a type with a custom `drop` may
  contain a view. A value whose destruction runs such a `drop` keeps its
  borrows until it is destroyed: at the end of its scope, at `return` (including
  `?`), when it is replaced by assignment, and during panic cleanup, not just
  until its last ordinary use. (a) Moving it out whole, or passing it to
  `drop`, ends its borrows there, since the new owner destroys it. (b) Since
  locals are destroyed in reverse declaration order on every exit, such a value
  must be declared after any local whose storage it views; viewing storage
  declared later is rejected even when every exit would be safe.
  (c) Whether a type's destruction can observe a view is decided by its type:
  it contains a struct with a custom `drop` that itself contains a view.
  Pending cases: `tests/conformance/ownership.md`.

- **Q20 — Mutable views held in composites:** a `mut []T` may be a struct
  field or fixed-array element at any depth. (a) A parameter (including a
  receiver, a closure or function-type parameter) whose type holds such a nested
  view must be `mut` or `own`: a shared borrow of a container gives no mutable
  access through a view inside it, and requiring an owned or mutable container
  enforces that without tracking it. A parameter of type `mut []T` itself is
  unchanged. (b) A mutable view may be held in an `Array<T>`, map, or
  `mut []T` element, directly or through a struct; reading it out of an
  `Array<T>` or mutable slice is an exclusive reborrow, as for a fixed array.
  A shared slice cannot hold one, since copies of the shared slice would hand
  out the same mutable view twice, and a map lookup cannot copy one out
  (`remove` can). (c) Any clone of a value
  holding a mutable view is rejected, since a struct holding one cannot declare
  a custom `clone` (its receiver would be a shared parameter). (d) Copying such
  a value is an exclusive reborrow of every view inside it, and counts as a
  write of the whole copied place; the source's other fields stay usable.
  (e) Assigning a container ends the reborrows taken through its old views.
  Pending cases: `tests/conformance/ownership.md`.

- **Q19 — Clone details:** §10.7 is implemented as locked; these gaps are
  filled conservatively. (a) Only structs, fixed arrays, `Array<T>`, and maps
  can be cloned: `clone` of a primitive, string, `error`, slice, or closure is
  rejected, since Copy values are copied by assignment. (b) A Copy field or
  element inside a structural or built-in clone is copied bitwise even when its
  type declares a custom `clone`; a custom clone runs when the cloned value
  itself has one, or when a non-Copy part does. (c) "Clonable by this same
  rule" for a field or element means having a custom or a structural clone.
  (d) `value.clone()` works only for a declared custom `clone`; there is no
  synthesized method for the structural or built-in clones. (e) A custom
  `clone` must have a shared receiver, no parameters, and exactly its own type
  as the only result. (f) Parts are cloned in declaration or index order, and
  map entries in the map's entry order. If a custom clone panics, the parts
  already cloned are dropped (in reverse order, map values in entry order), the
  new storage is freed, and unwinding continues. (g) Allocation failure aborts,
  as for `Array<T>` (Q17g). Pending cases: `tests/conformance/destruction.md`.

- **Q28 — Mutex choices:** locked in §20.2 at the maintainer's direction. (a)
  `Mutex<T>` is a Copy handle to one shared cell, not a Move value, since a Move
  value cannot be shared between tasks; §10.3 is revised to say so. (b) The
  constructor is the predeclared `mutex(value)`, with the type inferred from the
  value as in the spec's conceptual example, and both `mutex` and `Mutex` are
  protected names (§3.18). (c) The only way to reach the value is
  `withLock(f)` with `f` of type `func(mut T) R...`; there is no separate lock
  and unlock, so a lock cannot be forgotten. (d) A guarded value and a callback
  result cannot hold a slice or function value. (e) A poisoned mutex stays
  poisoned and every later `withLock` panics; `isPoisoned` is the only other
  method, and there is no way to recover the value. (f) The lock is not
  reentrant, and the lock is given to waiters in arrival order. (g) A waiting
  task counts as blocked for deadlock detection, so locking a mutex inside its
  own callback, or two tasks locking two mutexes in opposite orders with nothing
  else running, is reported as a deadlock. (h) The zero mutex (from a drained
  closed channel, for example) has no lock: `withLock` panics, `isPoisoned` is
  `false`. (i) No read-write lock, `try` lock, timeout, or condition variable;
  a task that needs to wait for a state change can use a channel. Pending
  cases: `tests/conformance/mutex.md`.

- **Q29 — Select choices:** locked in §19.14 at the maintainer's direction. (a) `select` is the only new keyword; `case` and `default` are special only at the start of an arm, so they stay ordinary identifiers. (b) Every channel operand and send value is evaluated once, in order, before a case is chosen. (c) When several cases can proceed, the runtime starts from a rotating position so no case starves; the choice is otherwise unspecified. (d) A zero-value channel is always ready to receive and panics on send, like a closed one. (e) A `select` with no `default` and nothing that can ever proceed is reported by the deadlock detector. (f) A send case that is not chosen keeps its value, which is dropped at the end of the `select`.
- **Q30 — Byte-array API choices:** locked in §37.2–37.3 at the maintainer's direction; this answers the byte-array part of Q05. (a) A byte array is an ordinary `Array<byte>`; there is no new type. (b) `strings.Bytes` copies a string's bytes; `strings.FromBytes` copies bytes into a string and fails on invalid UTF-8, as §41.5 requires, so no invalid string can exist. (c) Functions that take bytes take a `[]byte` view, written `data[:]` from an array, the same as `strings.Join`. (d) `os.ReadBytes`, `os.WriteBytes`, `Conn.ReadBytes`, and `Conn.WriteBytes` never check or change the bytes; `ReadBytes` returns up to `max` bytes and waits for at least one. (e) Mixing `Read` and `ReadBytes` on one connection is allowed: bytes of a character that `Read` held back are returned first by `ReadBytes`. (f) Standard input stays line text; there is no `io.ReadBytes` yet. Q37 makes files, standard input, and connections carry bytes and removes the text reads.
- **Q31 — Time limits and cancellation:** locked in §37.3–37.4 at the maintainer's direction. (a) Cancellation is cooperative through a `cancel.Token` built from a channel and a mutex; there is no way to stop or cancel a task from outside, so `Task` still has only `wait`. (b) A time limit is a per-connection setting (`SetTimeout`) that applies to each wait, rather than a parameter on every call, so existing calls keep their meaning. (c) `time.After` is a channel so it works with `select`; the timer is a sleeping task, which the deadlock check already treats as able to make progress. (d) A timed-out read loses nothing and a timed-out write may have sent part of its data. (e) Reads of standard input, whole-file operations, and name lookups have no limit yet, and a token cannot interrupt an operation that is already waiting. Q37 replaces `cancel.Token` with `context.Context` and per-wait limits with deadlines.
- **Q32 — Async as state machines:** locked in §17.3, §17.7, §18.3, §20.2, §35.1, and §37.3 at the maintainer's direction; the proposal is `docs/proposals/async-state-machines.md`. (a) Async functions are compiler-generated state machines that the runtime polls, and all six implementation slices are complete: fibers, stack switching, and the per-task thread fallback are removed. Plain entries run once on the shared pool. (b) The source language does not change: waiting operations stay ordinary calls without `await`, which suspend the task in an `async func` and block the thread in any other function; an `await` on every wait and a `go` limited to `async func`s were considered and rejected to keep the language simple. (c) `go` accepts any declared function, and a plain-function task holds a thread while it waits, so programs with very many tasks should make their task functions `async`. (d) A worker thread that blocks in a plain function is replaced, which is the progress guarantee already in §18.9. (e) The first compiler kept every local of an async function in one heap frame, and does not destroy the frames of tasks that are still suspended when the program ends, matching the process-exit behavior of the former fiber runtime. Current status (issue #75): poll-local storage and same-type slot sharing now shrink some frames, as described in `docs/conformance/async-runtime.md`; this entry records the original decision.

- **Q33 — Function values and spawned closures:** locked in §16.2, §16.4, §16.6, §18.3, and §18.4 at the maintainer's direction; the proposal is `docs/proposals/function-values-and-spawned-closures.md`. (a) A declared synchronous function's name, including a package-qualified one, is a capture-free Move function value; `async func` names, methods, and built-in operations are not. (b) `go` takes a declared function or method, a closure literal, or a function-typed local as its callee; the callable is moved into the task, run once, and destroyed. A field, element, map value, or call result is not a callee. (c) A spawned closure is owning: Copy captures are copied, Move captures moved, and borrowed parameters, views, `mut` and exclusive captures are rejected; changing a captured Copy value inside it is rejected at the change. (d) An owning function-typed `own` argument is accepted when no captured value holds a view; a function value received through a parameter is rejected as a spawned callable or argument, since its captures are unknown. (e) Closure bodies are never async, so a spawned closure is one plain task. Q25(a) and Q25(b) are narrowed accordingly; task results still cannot be slices, views, or function values. Pending cases: `tests/conformance/closures.md`.

- **Q34 — Method values:** locked in §16.2 and §9.1 at the maintainer's direction; the proposal is `docs/proposals/method-values.md`. `value.Method` without a call is the closure literal that calls the method, so the receiver is captured by the existing rules: shared borrow, exclusive borrow of a mutable place, or moved in as a call-once closure for an `own` receiver; an escaping or spawned method value is owning. The receiver is a local or a field path rooted at one. `async` methods, `drop`, and method expressions on a type are not method values. Pending cases: `tests/conformance/closures.md`.

- **Q35 — `go` on a callee stored in a field (WITHDRAWN):** proposed in `docs/proposals/spawn-field-callee.md` and withdrawn by the maintainer; nothing was locked or implemented. A closure in a field of a local struct is started by moving it into a local first (`let run = w.run`, then `go run(...)`); a closure in an array or map comes out with `pop` or `remove`. A function value that came in through a parameter cannot be spawned in either form (§18.4). Q33's rejection of a field, element, map value, or call result as a `go` callee stands.

- **Q36 — `async` function values:** locked in §16.2, §17.2, §17.8, and §18.3 at the maintainer's direction; the proposal is `docs/proposals/async-function-values.md`. (a) `async func(...)` is a function type whose identity includes the `async` property, with no conversion either way. (b) A declared `async func` name, including a package-qualified one, is a capture-free Move value of it. (c) A call through such a value must be the operand of `await` or `go`; an awaited call uses the callee exclusively across suspension, and `go` follows Q33. (d) Async method values, built-in operations, and closure literals are not async function values; async closure literals remain a later decision. Pending cases: `tests/conformance/closures.md` and `tests/conformance/concurrency.md`.

- **Q37 — Standard library refactor:** locked in §3.20 and §37.2–§37.5 at the maintainer's direction, revising Q30 and Q31; old names are removed with no transition period. (a) A standard import path may have several segments (`"zore/os/exec"`, `"zore/unicode/utf8"`, `"zore/path/filepath"`); the package name is the last one. (b) Names follow one pattern across packages: `ToUpper`/`ToLower` replace `Upper`/`Lower`, `ReplaceAll` is the old `Replace`, and `Replace` takes a count. (c) Files, standard input, and connections carry bytes: `os.ReadFile` returns `Array<byte>` and `os.WriteFile` takes bytes and permission bits, replacing `ReadBytes`/`WriteBytes`; `Conn.Read` and `File.Read` fill a `mut []byte` and return a count, `Write` returns a count, and text goes through `strings.FromBytes`. The text reads of `net.Conn` and the carried-over partial characters of Q30(e) are gone. (d) `zore/io` is removed; lines are read with `bufio.Scanner` or `bufio.Reader` over `os.Stdin()`. (e) `zore/time` counts `int` nanoseconds with unit constants (`time.Millisecond`), and `time.Now`/`Since`/`Until` replace `time.Millis`; there is no separate duration type yet, since the MVP has no named non-struct types. (f) Network deadlines replace timeouts: `SetDeadline`, `SetReadDeadline`, and `SetWriteDeadline` take a `time.Now` reading, `Listen`/`Dial` take a network name (`tcp`, `tcp4`, `tcp6`), `Addr`/`LocalAddr`/`RemoteAddr` replace `Port`, and `Close` takes its receiver with `own`. (g) `zore/cancel` becomes `zore/context`: `WithCancel` returns a cancel function, `Err` reports `context canceled` or `context deadline exceeded`, and `Token.Sleep` is removed in favor of `select` on `Done()` and `time.After`. (h) Parse errors name their input (`strconv.Atoi: parsing "x": invalid syntax`). (i) New packages written mostly in Zore: `bytes`, `errors`, `unicode`, `unicode/utf8`, `path`, `path/filepath`, `sort`, `sync`, `bufio`, and `os/exec`, plus new `strings`, `strconv`, and `os` functions. (j) Without package-level variables there is no `io.EOF` or `os.Args` value: errors compare by message (`err == error("EOF")`) and `os.Args()` is a function. Without zero-value declarations, `strings.NewBuilder`, `bytes.NewBuffer`, `sync.NewWaitGroup`, and `sync.NewOnce` construct those types. (k) Bundled functions that the runtime provides may now take `rune` and narrow integer arguments, and any bundled function that calls a waiting function is lowered as waiting. Pending cases: `tests/conformance/io.md` and `tests/conformance/packages.md`.

- **Q38 — The `panic` call:** locked in §3.17, §7.7, and §15.4 at the maintainer's direction. (a) `panic` is a predeclared name and takes exactly one `string`; it is not a value, like `println`. (b) A call statement of `panic` never completes, so it ends a path for the completion rule without a following `return`; since predeclared names cannot be shadowed, the checker recognizes the bare name. (c) The reported message is the argument, ` at `, and the call's location, matching the runtime's own panics. (d) An `error` argument was considered and left out; a program writes `panic("...")` with its own text. Pending cases: `tests/conformance/errors.md`.

- **Q39 — Rune conversions:** locked in §6.6 at the maintainer's direction. (a) Integer types and `rune` convert both ways with `T(r)` and `rune(n)`; `rune` stays a distinct type, not an alias. (b) Both directions are checked: a runtime value that does not fit, or that is not a Unicode scalar value, panics, and a constant one is a compile-time error. (c) Floats and `bool` do not convert to or from `rune`; a program converts through an integer type. Pending cases: `tests/conformance/runes.md`.

- **Q40 — Named types:** locked in §8.5, §3.20, §9.1, and §41.4 at the maintainer's direction. (a) `type Name Base` declares a distinct type built on `bool`, a number type, `rune`, `string`, or another named type; collection, function, struct, and `error` bases are left for later. (b) The base type's operations apply and keep the named type; a named type never mixes with another type implicitly, and untyped constants adopt it as they adopt the base. (c) `bool`, `rune`, and `string` literals stay typed, so a named value from a literal needs a conversion such as `Name("x")`. (d) A named type may have methods but not a custom `drop` or `clone`, and it does not get its base type's methods. (e) This is the representation the standard library uses for durations (`time.Duration`) once #87 merges. Pending cases: `tests/conformance/functions-structs.md`.

- **Q41 — Package-level `let`:** locked in §3.20 and §3.21 at the maintainer's direction; this answers the package variable initialization order part of Q05. (a) Only `let`, one name per declaration: package-level values never change, so tasks can read them without the shared-mutation rules of §20.1. (b) Only Copy values without slices, function values, tasks, channels, or mutexes; channel and mutex values are left for later because their handles need a decision about cleanup at exit. (c) Values are computed once before `main`, imported packages first and then in source order; using a later value, directly or through called or spawned functions and closures, is a compile-time error. (d) A panic during initialization ends the program and `main` does not run. (e) `init` functions and package-level `var` remain out. Pending cases: `tests/conformance/packages.md`.

- **Q42 — Interfaces and generics:** interfaces are locked in §22.2 at the maintainer's direction, following `docs/proposals/interfaces-and-generics.md`, and generic functions and struct types in §22.1 (see (i) to (l)). (a) `type Name interface { ... }` lists methods with their receiver modes; a type satisfies it by having them, and a `mut` or `own` entry also accepts a method that needs less access. (b) A shared or `mut` interface parameter borrows its argument; everywhere else an interface value owns the value inside and is Move. (c) Conversion is implicit where an interface type is expected, including from another interface that covers it; there is no conversion back. (d) An owned interface value cannot hold a view, a narrowing of the proposal that keeps owned values sendable; it can be lifted later. (e) Interface values are not comparable, printable, clonable, or `nil`; their zero value is empty and panics when called. (f) `error` stays one concrete Copy type. (g) A call through an interface behaves like a direct call, including suspending in async code; each method table holds a start adapter per entry that builds the method's frame when it can suspend and a finished frame otherwise, and `async` entries are awaited. (h) `interface` moves from the future-reserved words to the keywords. Method values and `go` on calls through interface values are not part of this decision. (i) Generic functions are locked in §22.1 as the next step: type parameters in angle brackets with one constraint each (`any`, `copyable`, `comparable`, `ordered`, or an interface type), type arguments always inferred from the arguments, bodies checked once for every allowed type, and one compiled copy per set of type arguments. (j) Narrowings of the proposal, each able to be lifted later: a type argument cannot hold a function value, a `mut []T`, or a borrowed interface value; `ordered` does not pass on to `comparable`, since floats are ordered but not map keys; and generic functions as values, function literals, `go`, method values, conversion of a `T` to an interface, `clone`, and printing inside generic bodies are not supported yet. (k) Generic struct types are locked in §22.1: `type Stack<T any> struct`, methods on `Stack<T>` that name the type's parameters and declare none of their own, `Stack<int>` and `Stack<int>{...}` with explicit type arguments, one struct and one set of method copies per set of type arguments, and instances that satisfy interfaces through their methods with the type arguments in place. Generic named types, generic interfaces, and methods of generic types as values are not part of this decision. (l) A call whose arguments leave a type parameter undecided takes it from the type the call is expected to have, so `var s Stack<int> = NewStack()` works without explicit type arguments. (m) The library follow-up adds `zore/io` (`Reader`, `Writer`, `Closer`, and their combinations, `EOF`, `Copy`, `ReadAll`, `ReadFull`, `WriteString`) and builds `bufio` on `io.Reader` and `io.Writer` (§37.3). It also adds `sort.Slice` and the generic `zore/slices` and `zore/maps` packages (§37.5). `strings` and `bytes` stay separate, since folding them needs a union constraint; `fmt`, error wrapping, and type assertions remain later decisions.

- **Q43 — Float text:** locked in §37.1 and §37.2 at the maintainer's direction (issue #89). (a) `println` and `strconv.FormatFloat(v, 'g', -1, 64)` share one default form: the shortest decimal that reads back to the same value of the float's own type. (b) A whole value keeps a point and one digit, `3.0`, so a float never looks like an integer. (c) The exponent form starts when the first digit's decimal exponent is below -4 or at least 21, as Go's `%v`, written Go's way with a sign and at least two digits: `1e+21`, `1e-05`. (d) The special values are `NaN`, `+Inf`, and `-Inf`; a computed negative zero prints `-0.0`. (e) `strconv.FormatFloat` takes Go's `f`, `e`, `E`, `g`, and `G` formats with Go's precision rules; its format argument is a `rune` rather than Go's `byte`, because Zore rune literals do not convert to `byte` implicitly. Unknown formats and bit sizes panic, as `FormatInt` does for a bad base. (f) `strconv.ParseFloat` reads plain decimal text only, with `NaN`, `Inf`, and `Infinity` in any case; digit separators and hexadecimal are rejected, and overflow returns the signed infinity with `value out of range`, as in Go. (g) A small `zore/math` package follows: constants, `Inf`, `NaN`, `IsNaN`, `IsInf`, `Abs`, `Max`, `Min`, `Sqrt`, `Floor`, `Ceil`, `Trunc`, `Round`, `Pow`, `Mod`, `Exp`, and `Log`. Pending cases: `tests/conformance/floats.md` and `tests/conformance/println.md`.

- **Q27 — Async I/O choices:** locked in §37.3 at the maintainer's direction
  (time, standard input, whole files, and TCP, waiting on an event loop). (a)
  Strings are well-formed UTF-8, so every function that returns text from
  outside reports an error for other bytes; byte-array functions are in Q30.
  (b) `net.Read` keeps an unfinished trailing character with the connection, so
  a result can exceed `max` by up to three bytes. (c) A `Conn` or `Listener`
  holds a runtime handle number that is never reused, so a closed or zero
  handle can only fail; user code cannot build one because its field is not
  exported and struct literals must name every field. (d) A connection is a Move
  value used by one task at a time, so one task cannot read while another
  writes; sharing one connection between tasks waits for a shared-state
  feature. (e) Files, standard input, name lookup, and connecting use helper
  threads because regular files cannot be polled; socket reads, writes, and
  accepts and all timers use the event loop. (f) The messages after the fixed
  prefixes come from the operating system and are not specified. (g) Waiting in
  the initial task blocks its thread. (h) Time limits and cancellation are in Q31. Pending cases: `tests/conformance/io.md`.

- **Q26 — Channel implementation choices:** (a) `channel<T>(n)` takes an `int`
  capacity; a constant negative capacity is rejected and a negative runtime one
  panics with "negative channel capacity". (b) `receive()` gives `(value, ok)`,
  value first, so it must be bound or discarded as two results. (c) The element
  type cannot hold a slice or function value, so a queued message never borrows
  the sender's storage (§19.4); an owned array, a string, a channel handle, and
  a Task are accepted. (d) A send that finds the channel closed, or is woken by
  a close, panics with "send on a closed channel", after the runtime destroys
  the value that was never queued; closing twice, or closing a zero-value
  channel, panics with "close of a closed channel". (e) A receive from a
  zero-value channel returns `(zero, false)` at once and a send panics. (f)
  Waiting senders and receivers are served first come, first served, and a
  receive that makes room moves the oldest blocked sender's value into the
  buffer. (g) Channel handles are counted with an atomic reference count; a
  handle cycle through buffers is never freed (§19.13). (h) A program whose
  tasks all wait on channels or on each other, with nothing that could wake one (no timer, descriptor, or helper thread), stops with "fatal error: all tasks are asleep" and exit status 2; a deadlock among some tasks while others run is not detected. (i)
  `select` is not available. Pending cases: `tests/conformance/concurrency.md`.

- **Q25 — Task implementation choices:** (a) `go` takes a call to a declared
  function or method, including an `async func`; Q33 adds a closure literal and a
  function-typed local as the callee, and the built-in operations stay rejected.
  (b) Inputs follow §18.4 conservatively: a `mut` parameter, any argument whose
  type holds a slice, a function value for any parameter but an `own` one (Q33),
  and a Move argument for a shared parameter are rejected;
  Copy values, including text, are copied into the task, and `own` arguments are
  moved in. (c) A task cannot return a slice or function value, and a written
  `Task<...>` type with such a result is rejected. (d) `go f(args)` as a
  statement detaches the task, and so does dropping or overwriting a handle; a
  detached task's results are destroyed when it finishes. (e) Tasks are
  scheduled on the shared worker pool: async functions use persistent heap frames
  and polls; plain functions run once and block their OS worker during waits.
  Worker compensation prevents starvation. The former fiber implementation was
  replaced by Q32 without changing the source language. There is no preemption;
  large waiting workloads should use async task functions. (f) A task
  panic is reported on standard error as `panic in task N: message` when it
  happens, then raised again with the same message at `.wait()` or `await`.
  Waiting on a `nil` task panics with "wait on a nil task". (g) Runtime-built
  text is reference counted under one lock, so any task may hold, copy, and
  release it. (h) When the initial task finishes, the process exits at once
  without freeing the text that running tasks hold. (i) `await task` and
  `.wait()` are the only operations on a task; there is no cancellation,
  timeout, or join-all. Pending cases: `tests/conformance/concurrency.md`.

- **Q24 — Projects, packages, and imports:** locked in §3.20 and §37.2 at the
  maintainer's direction: a folder is a package, import paths start with the
  project name (or `zore` for standard packages), and `zore run file.ore` builds
  the file's whole folder. Filled conservatively: (a) a non-`main` package's
  name must equal its folder name, so the qualifier is the last path segment;
  (b) unused or duplicate imports are errors, as in Go; (c) import aliases,
  dot and blank imports, dependencies on other projects, package-level
  `let`/`var`, and `init` functions are rejected; (d) a struct with an
  unexported field cannot be constructed outside its package, because a struct
  literal must name every field; (e) the entry folder without `zore.toml` has no
  project name and imports only standard packages; (f) a method may be declared
  only on a type of its own package; (g) `Split`, `Join`, and the other standard
  functions are the first slice of Q05's library inventory, with Unicode-aware
  helpers beyond case mapping left open (float text is resolved in Q43). Pending cases:
  `tests/conformance/packages.md`.

- **Q23 — Strings:** locked in §6.8 at the maintainer's direction. Length,
  indexing, and slicing count bytes; `for ch in s` visits characters with byte
  indexes; `+` concatenates at run time; `string(rune)` is the only string
  conversion. Filled conservatively: (a) a slice must start and end on character
  boundaries or it panics, so strings stay well-formed; (b) `s[i]` is a
  read-only `byte`; (c) runtime-built string storage is reference counted: each
  kept copy of a `string`, including a slice that shares the buffer, is one
  owner, and the buffer is freed when the last owner goes (`string` stays Copy
  for the programmer; the compiler adds and removes owners itself, also while a
  panic unwinds); appending to the newest text in a buffer grows the buffer in
  place (doubling), which is safe because no string reads past its own end, so
  building one text in a loop costs memory proportional to its final length;
  the buffer table is shared by every task and guarded by a lock, so text
  can cross tasks (Q25); (d) number
  formatting stays out of the language and lives in `zore/strconv`. Pending
  cases: `tests/conformance/strings.md`.

- **Q02i — Owning and call-once closures:** locked in §16.4 and §16.6 at the
  maintainer's direction. Ownership is inferred: a literal is owning when it, or
  a local it initializes (followed through `let g = f`), is returned, stored in
  a field, fixed array, `Array<T>`, or map value, or passed to `own`. A closure
  that consumes a captured Move value is call-once, which also makes it owning;
  it must initialize a `let` binding that is only called directly, and the call
  consumes it. Filled conservatively: (a) a Copy local an owning closure
  assigns is treated as moved unless it is a by-reference parameter; (b) an
  owning closure may not store a view of one captured value's storage into
  another, since the values move together; (c) a shared parameter whose type
  holds a function value inside a struct, array, or collection is rejected, and
  slices never hold function values; (d) captured values are destroyed in
  reverse capture order. Executable cases are listed in
  `tests/conformance/closures.md`.

- **Q02h — Collection loops and methods:** locked in §5.10 and §12.7 at the
  maintainer's direction. `for item in c`, `for i, item in c`, and `for key,
  value in m`; `in` is a keyword. The item is a shared borrow and the loop
  shared-borrows a place collection until it ends; a temporary collection is
  held by the loop. `len` on every collection, and `push` and `pop` (presence
  first, like map removal) on `Array<T>`. Filled conservatively: (a) a loop over
  elements that hold a `mut []T` is rejected; (b) a view copied out of an item
  keeps the element's provenance, not the loop's borrow; (c) `Array<T>` keeps a
  capacity that doubles from 4, and `pop` does not shrink it; (d) map keys read
  by a loop are the stored keys, so a string key keeps its original text
  storage, which is always a literal today. Executable cases are listed in
  `tests/conformance/control-flow.md` and `tests/conformance/arrays-slices.md`.

- **Q02g — Closures and function types (first slice):** locked in §16.
  `func(params) results { body }` literals; unnamed function types
  `func(T, mut U) R`; captures inferred per whole local (shared borrow for reads,
  exclusive borrow for writes and for captured closures or `mut []T` values);
  closure values are Move and non-escaping (no function-typed results, fields,
  or elements, no `go`, no liveness across `await`). Calling a closure, or
  passing one to a function-typed parameter, uses it exclusively. Function-typed
  parameters cannot be `mut` or `own`. Implemented end to end; executable cases
  are listed in `tests/conformance/closures.md`. **Still open:** escaping
  closures with owned environments, call-once closures that consume a captured
  Move value (both resolved in Q02i), declared functions used as values,
  storing a view of a closure's own parameter into a capture (Q22), `async`
  closures, field-level captures, and closures with tasks (M25+).

- **Q02f — Map construction, lookup, assignment, removal:** accepted and
  locked in §13.3. Explicit typed literals; bool/integer/rune/string keys;
  duplicate constant keys rejected, dynamic duplicates panic after key/value
  evaluation. Lookup copies eligible values through shared access and always
  returns `(bool, V)`; Move or exclusive-view copies are rejected. Assignment
  inserts/replaces through a mutable map; no compound map assignment or general
  entry places. `remove(key)` mutably detaches and transfers a value, including
  Move values. Both operations return false plus V's zero on absence. Error V
  stays last; map subscript `?` remains invalid. Per-entry state preserves
  cleanup on duplicate failure, replacement panic, and removal. Pending cases:
  `tests/conformance/maps.md`. Borrowed in-place access, iteration, and remaining
  library operations are still Q02/Q05 dependencies.

- **Q02e — Arrays, indexing, and slicing:** accepted and locked in §12.6.
  Explicit `[T; N]{...}` and `Array<T>{...}` literals; exact fixed counts;
  left-to-right construction and partial-initialization cleanup. Integer bounds
  checked before narrowing, with static errors or runtime panic. Runtime indices
  may be writable without granting partial moves or disjointness. Half-open
  slicing supports omitted bounds; views are shared by default, exclusive in
  `mut []T` context from a mutable source. `edit(data[:])` is explicitly allowed
  by §11.6; no implicit array-to-slice conversion. Pending cases:
  `tests/conformance/arrays-slices.md`. Maps, strings, iteration, collection APIs,
  closures, and full `go` grammar remain separate decisions.

- **Q11 — Fixed-width shifts:** locked in §6.6. Typed left shifts keep the
  low bits at the left operand's width, including for signed two's-complement
  operands and typed constants. Discarded bits do not cause overflow; invalid
  counts still fail at compile time or panic at runtime. Untyped constants
  preserve exact values until typing; named constants are not retroactively
  truncated. Checked arithmetic is unchanged. Pending cases: numerics.

- **Q12 — Custom clone precedence:** §10.7's compiler instructions now agree
  with the normative custom-first rule. A declared custom clone is validated
  and selected before structural cloning; invalid declarations or borrow/type
  failures are errors, never grounds for silent fallback. Copy/Move, borrow,
  error, and async behavior remain unchanged. Pending cases: destruction.

- **Q01a — ASCII identifiers:** locked in spec §3.5 and §4.1. Start with
  `A–Z`, `a–z`, or `_`; continue with those or `0–9`. Names are case-sensitive.
  Initial `A–Z` exports package declarations/fields; lowercase and underscore
  prefixes are private. Standalone `_` is reserved, with discard contexts defined
  in §5.5. Strings/comments may contain Unicode. Pending executable
  conformance cases: `tests/conformance/identifiers.md`.

- **Q01b — Comments:** locked in spec §3.6. `//` extends to line end or EOF;
  `/* ... */` blocks do not nest and end at the first `*/`. Unterminated blocks
  are lexical errors. Comments separate tokens and allow Unicode text. Their
  newline effects on statement boundaries are defined in §3.7. Pending executable
  conformance cases: `tests/conformance/comments.md`.

- **Q01c — Statement boundaries:** locked in spec §3.7. Automatic semicolon
  insertion after identifiers, literals, `break`, `continue`, `return`, `true`,
  `false`, `nil`, `)`, `]`, `}`, and postfix
  `?` at newline/EOF. Explicit separators are allowed; final separators may be
  omitted before `)`/`}`. Delimiters do not suppress insertion. Comment newlines
  participate, LF/CRLF terminate lines, and CR alone is whitespace. New keyword
  decisions must specify insertion eligibility. Pending executable conformance
  cases: `tests/conformance/statement-boundaries.md`.

- **Q01d — String forms:** locked in spec §3.8. Double-quoted strings interpret
  escapes and cannot contain physical newlines; the escape set is defined in §3.9.
  Backtick strings allow literal multiline text, preserving whitespace and line
  endings without escapes. Both allow Unicode; neither supports interpolation.
  Pending executable conformance cases: `tests/conformance/strings.md`.

- **Q01e — String escapes:** locked in spec §3.9. Accept `\n`, `\r`, `\t`,
  `\\`, `\"`, four-digit `\uXXXX`, and eight-digit `\UXXXXXXXX`. Unicode
  escapes require scalar values; reject surrogates, out-of-range values, malformed
  digits, and unknown escapes. No byte/octal escapes or recursive decoding.
  Raw strings remain literal. Pending executable cases: `tests/conformance/strings.md`.

- **Q01f — Rune literals:** locked in spec §3.10. Single quotes contain exactly
  one Unicode scalar after decoding. Support string escapes plus `\'`. Reject
  empty/multiple-scalar contents, physical newlines, malformed escapes, surrogates,
  and out-of-range values. No normalization or raw rune form. Representation and
  conversions remain type-model questions. Pending cases: `tests/conformance/runes.md`.

- **Q01g — Integer bases:** locked in spec §3.11. Decimal plus explicit `0b`,
  `0o`, and `0x` prefixes, with uppercase prefix variants. Digits are ASCII;
  hexadecimal letter digits accept either case. Leading zeros remain decimal.
  Prefixes require digits valid for their base. Separators are defined in §3.12;
  suffixes are excluded by §3.14 and typing remains open. Pending cases: `tests/conformance/integers.md`.

- **Q01h — Digit separators:** locked in spec §3.12. One underscore between
  valid digits only; no leading, trailing, consecutive, or post-prefix separators.
  Group sizes are unrestricted and values unchanged. Floating-point digit
  sequences follow the same rule. Pending cases: `tests/conformance/integers.md`.

- **Q01i — Float forms:** locked in spec §3.13. Decimal fractions require digits
  on both sides of the point; scientific notation uses `e`/`E`, an optional sign,
  and required exponent digits. Separators only between digits. No `.5`, `1.`,
  or non-decimal float forms. Type/rounding/range rules remain open. Pending cases:
  `tests/conformance/floats.md`.

- **Q01j — No numeric suffixes:** locked in spec §3.14. Reject integer/float
  suffixes and attached identifier continuations; preserve valid hexadecimal
  digits and exponent notation. Literal defaults, context, annotations, and
  conversions remain separate typing decisions. Pending cases:
  `tests/conformance/integers.md` and `tests/conformance/floats.md`.

- **Q01k — Keyword reservation policy:** locked in spec §3.15. Reserve adopted
  MVP keywords and an explicitly selected set of future-feature words. Reservation
  does not enable or promise the associated features. MVP keywords are locked in
  §3.17 and future-reserved words in §3.16. Pending conformance requirements:
  `tests/conformance/keywords.md`.

- **Q01l — Future-reserved list:** locked in spec §3.16: `interface`, `trait`,
  `impl`, `enum`, `match`, `unsafe`, `macro`, `defer`. Exact lowercase matches
  only; no identifier escape mechanism. These words do not trigger semicolon
  insertion or enable features. Pending cases: `tests/conformance/keywords.md`.

- **Q01m — MVP keywords:** locked in spec §3.17. Adopt the structure, binding,
  ownership, control-flow, concurrency, type-syntax, and literal keyword table.
  `break`, `continue`, `return`, `true`, `false`, and `nil` trigger insertion;
  other keywords do not. Built-in primitive names, `Array`, `Task`, `error`,
  `println`, `clone`, and `drop` are predeclared identifiers, protected by §3.18.
  Unspecified control-flow/type rules remain open. Pending cases:
  `tests/conformance/keywords.md` and `tests/conformance/statement-boundaries.md`.

- **Q01n — Predeclared-name protection:** locked in spec §3.18. Reject user
  declarations that shadow predeclared names in unqualified lookup at any scope.
  Member names are not rejected solely for matching a predeclared spelling;
  user-defined `drop` methods retain their cleanup contract. Built-in inventory
  additions remain separate decisions; ordinary shadowing follows §5.7. Pending
  cases: `tests/conformance/keywords.md`.

- **Q01o — Discard targets:** locked in spec §5.5. `_` may discard result
  positions in `let`/`var` bindings and ordinary assignments; it creates no name
  and cannot be read. Evaluate normally, enforce result counts and ownership,
  and destroy discarded owned values without destroying borrowed backing storage.
  Discarded Task handles detach. Explicit error discards are allowed by §15.6.
  Pending cases: `tests/conformance/discards.md`.

- **Q06a — Explicit error discards:** locked in spec §15.6. Allow `_` for error
  results; reject silently ignored error-bearing results and never-used named
  error bindings. This includes awaited results and retrieved task results without
  changing detachment semantics. More detailed flow-sensitive requirements remain
  open. Pending cases: `tests/conformance/errors.md`.

- **Q02a — Evaluation order:** locked in spec §7.5. Evaluate operands and call
  arguments left to right, callee/receiver before arguments. Early propagation
  skips later evaluation; explicit await completes before evaluating the next
  operand. Preserve short-circuit exceptions, ownership, and cleanup. This does
  not introduce implicit task waits or settle assignment/initializer sequencing.
  Pending cases: `tests/conformance/evaluation-order.md`.

- **Q02b — Operators and expressions bundle:** locked in spec §7.6. Arithmetic,
  comparison, bool logic, integer bitwise/shift operators, unary signs/complement,
  and string concatenation with the agreed precedence table. No comparison chains,
  implicit truthiness, numeric-to-string conversion, assignment expressions,
  increment/decrement, ternary, exponentiation, or pointer operators. Boolean logic
  short-circuits; `await operation()?` propagates after awaiting. `go` requires a
  call; its full grammar remains open. Pending cases: `tests/conformance/expressions.md`.

- **Q02c/Q03a — Bindings and assignments bundle:** locked in §5.4, §5.6–5.7.
  Require initializers; optional `name Type` annotations on single bindings and
  constants. Multiple bindings use one matching multiple-result expression, without
  per-name annotations. Support compound updates with one target evaluation and
  multiple assignments with target/RHS/store phases; reject overlapping targets.
  Preserve ownership and replacement cleanup. Reject same-scope duplicates;
  permit nested shadowing of user-defined names only. Pending cases:
  `tests/conformance/bindings-assignments.md`.

- **Q03b — Control-flow bundle:** locked in §5.8–5.10 and §7.7. Statement
  blocks, bool conditions with required braces, if/else-if/else without initializers,
  infinite/conditional/counting loops, nearest-loop unlabelled exits, counting-loop
  continue through the update, and required exit cleanup. No range/foreach or
  labelled jumps. Explicit matching returns, no named result parameters, and no
  reachable result-function fallthrough. Locals enter scope after initialization;
  parameters and outermost body declarations share a scope. Pending cases:
  `tests/conformance/control-flow.md`.

- **Q02d/Q03c/Q05a — Functions, calls, and structs bundle:** locked in §7.8
  and §8.4. Individually typed positional parameters, exact argument counts,
  optional trailing commas, required user function bodies, package function/type
  forward references, no overloading, local-package receivers, and unique members.
  Named complete struct initialization evaluates fields in written order and
  respects visibility; parenthesize condition literals. Permit exact whole-result
  return forwarding, not argument expansion; restrict expression statements to
  calls/task creation (including explicit await/propagation call forms). Pending
  cases: `tests/conformance/functions-structs.md`.

- **Q04a — Numeric types and arithmetic:** locked in spec §6.5–6.6. `int` is
  `int64`, `uint` is `uint64`, `byte` is `uint8`, all architecture-independent;
  rune is a distinct Unicode-scalar Copy type. Unsuffixed literals default to
  `int`/`float64` and may take representable expected types. Typed conversions are
  explicit and checked. Integer overflow/division-by-zero and invalid shifts are
  checked consistently; float formats and rounding follow IEEE 754 binary32/64.
  Numeric comparisons require matching types; string ordering is UTF-8
  lexicographic. Pending cases: `tests/conformance/numerics.md`.

- **Q04b — Zero values and `nil`:** locked in spec §41.4. Every type has a
  defined zero value, produced only by specific built-in operations (closed-channel
  drain and map lookup/removal misses (§13.3)), never as a way to skip explicit
  initialization (§5.4, §8.4 are unaffected). `nil` is restricted to `error`,
  `Task`, and `channel<T>`; no other type admits a `nil` state or `nil`
  equality comparison. Structs and fixed arrays zero recursively per field/
  element; slices, `Array<T>`, and `map[K]V` zero to an empty-but-valid value
  with no separate nil state. This does not resolve `error`'s full
  representation (Q06) or map operations (subsequently resolved in Q02f). Pending cases:
  `tests/conformance/zero-values.md`. **Revised by Q09a:** channels no longer
  admit `nil`; the zero value of `channel<T>` is an always-closed, empty channel
  (§19.12), so `nil` now applies only to `error` and `Task<...>`.

- **Q04c — Constant-expression subset:** locked in spec §5.3. Constant
  expressions are restricted to scalar-typed values (`bool`, integer, float,
  `rune`, `string`); literals, named-constant references, the §7.6 operator
  set with constant operands, and constant-operand explicit numeric
  conversions are constant expressions, while calls, indexing, member access,
  `clone`/`drop`/`await`/`?`/`go`, and composite literals are not. Constants
  may forward-reference later constants; cycles are a compile-time error.
  Untyped-constant preservation (§6.5) and typed-conversion rules (§6.6)
  continue to apply, including where a constant expression is required outside
  `const`, such as a fixed array's size `N`. Pending cases:
  `tests/conformance/constant-expressions.md`.

- **Q04d — String value and encoding:** locked in spec §41.5. A `string`
  value is always well-formed UTF-8, for every string that exists at runtime,
  not only literals; no currently locked operation (literal decoding,
  concatenation, zero value) can produce an invalid one. `string` values are
  immutable. This constrains any future bytes-to-string conversion API (Q05)
  to validate and reject invalid input rather than repair it, without
  introducing that API here. Indexing/slicing/iteration (Q02) and internal
  buffer representation/concatenation allocation strategy (implementation
  detail) remain separate. Pending cases: `tests/conformance/strings.md`.

- **Q06b — Error representation and propagation:** locked in spec §7.2, §15.1–15.2,
  §15.6, §6.6. `error` is one concrete predeclared Copy type, not a general
  user-satisfiable interface (§22.2 is unaffected); constructed only via the
  predeclared `error(message string) error`, following the type-name-as-call
  convention from §6.6. `==`/`!=` between two `error` values is content
  equality; zero value remains `nil` (§41.4). A result list may hold at most
  one `error` result, which must be last. `?` requires the enclosing function
  to declare a trailing `error` result, fills non-error results with their
  zero value on an early return, and otherwise yields the non-error result(s)
  with the error consumed. The never-used-binding rule (§15.6) is now
  path-sensitive: every reachable path must use a named `error` binding's
  current value before reassignment or scope exit, but this tracking does not
  follow an error value stored into a composite. Wrapping, sentinel values,
  and structured payloads are explicitly not decided here. Pending cases:
  `tests/conformance/errors.md`.

- **Q04e — Harmless resource zero states:** locked in §41.4 and §14.3.
  Every resource type's recursive zero state owns no acquired resource and must
  be harmless to destroy. Normal custom/field cleanup still runs. Resource
  authors must distinguish acquisition from numeric handle values; this is an
  API contract, not a compiler proof of arbitrary destructor behavior. Applies
  equally to channel drain, `?` result filling, and map lookup/removal misses. Zero
  slices have no backing-storage loan. Pending cases: zero-values/destruction.

- **Q07b — Ownership safety review refinements:** locked in §11.7, §12.3,
  §12.5, §14.3, and §31.2; supersedes narrower wording in Q07a below.
  Partial moves may not cross a containing custom-destructor value, even with
  planned reinitialization. Moving that value whole remains valid. If old-field
  destruction during replacement panics after invalidating a field required by
  a containing destructor, abort rather than invoke an incomplete receiver.
  Borrow provenance propagates recursively through composites and captures,
  independently of Copy/Move. Return contracts include nested views; no view of
  local/owned-parameter storage may escape. Mutable descriptor copies create
  exclusive reborrows that suspend overlapping source access. Shared container
  borrowing does not confer mutable access. Pending cases: ownership/destruction.

- **Q09b — Task/channel escape safety:** locked in §18.4, §18.10, §19.4.
  Spawned borrows of another task's local/temporary or borrowed-parameter storage
  are rejected even with immediate retrieval. Copy arguments are materialized
  in task storage; Move inputs need ownership transfer, not an implicit move
  into a shared parameter. Contained borrows must remain independently valid;
  returning views into task argument/local storage is rejected. Channel messages
  cannot borrow sender-local storage, even for unbuffered sends. Check all exit
  paths including `?` and panic. Detachment is unchanged; Q10 stays open as an
  extension. Pending cases: concurrency/ownership.

- **Q07a — Return-borrow contracts, caller mutability, partial moves, slice
  aliasing:** locked in spec §11.6–11.7, §12.5, §31.2. A `mut` parameter,
  receiver, or slice position requires the caller's argument to be a mutable
  place (a `var` binding or reached entirely through mutable places), never a
  `let` binding or a shared borrow. A returned borrowed view, including one nested in a composite, must retain
  provable external backing provenance, never function-local owned storage
  (Q07b; zero views have no loan); the returned slice's region is then bound to
  the call site's corresponding argument, checked under ordinary exclusivity
  (§11.3). Partial moves leave a struct's still-available fields individually
  usable while the whole value is unusable until every field is reinitialized;
  this tracking covers only fixed field-access paths, not computed indices.
  Slice-borrow exclusivity is checked against the originating place, not the
  runtime index range — two disjoint-range mutable sub-slices of the same
  source still conflict; this is a stated MVP limitation, not an oversight.
  Pending cases: `tests/conformance/ownership.md`.

- **Q08a — Drop receiver, Copy/clone interaction, panic unwinding:** locked in
  spec §8.3, §10.7, §14.3–14.4, §15.4. `drop` must use a `mut` receiver, never
  shared or `own`, so it can mutate but never move a field out; it forces the
  type to be Move regardless of field composition; automatic per-field cleanup
  still runs after a custom `drop` body; calling a `drop` method directly via
  `.drop()` is rejected, leaving `drop(value)` and end-of-lifetime cleanup as
  the only triggers. `clone(value)` is available via a structural field/element-
  wise default (barred for any `drop`-bearing type), an explicit
  `func (c Type) clone() Type` method (which takes precedence and is the only
  option for a `drop`-bearing type), or a built-in element/entry-wise clone for
  `Array<T>`/`map[K]V`, which cannot receive user methods. `panic()` unwinds
  and runs pending drops; it is not catchable/recoverable, and a panic during
  unwinding aborts the process immediately. Pending cases:
  `tests/conformance/destruction.md`. **Refined by Q09a:** a panic unwinds only
  its own task; it terminates the process only when it occurs in the initial
  task, and otherwise is raised again at retrieval of the panicked task.

- **Q09a — Task, async, and channel runtime contracts:** locked in spec §6.3,
  §7.6, §15.2, §15.4, §17.8, §18.8–18.11, §19.11–19.13, §20.2, §36.2, §41.4.
  `go f()` has type `Task<R1, ..., Rn>` mirroring `f`'s result list (plain
  `Task` for no results); tasks are Move. An async call must be awaited or
  spawned; `await` is valid only in async bodies. Results are retrieved once:
  `task.wait()` blocks and is valid only outside async bodies; `await task`
  suspends and is valid only inside them; both consume the handle. The runtime
  must keep other tasks running while a `.wait()` blocks. A panic unwinds its
  own task; in the initial task it ends the process, in a spawned task it is
  re-raised at retrieval and otherwise only reported on stderr; a mutex held by
  a panicking task is poisoned. The process terminates immediately when the
  initial task completes, abandoning running tasks. Double close panics;
  closing wakes blocked senders (which panic) and receivers. Channels have no
  `nil`: their zero value is an always-closed, empty channel (revising Q04b).
  Buffered values are dropped exactly once when the last handle is gone, except
  in handle cycles, which are a stated memory-safe leak. Scoped tasks remain
  open as Q10. Pending cases: `tests/conformance/concurrency.md`.

## Interpretation notes

Examples such as §24's Move `User`, §48's undeclared resource types, and §20's
conceptual dereference syntax must be read under §0's normative-rule precedence.
They do not override derived Copy classification or introduce raw pointer syntax.

Internal decisions do not need new language syntax: LLVM version/bindings, host
target and ABI, pass ordering, string representation, allocator, scheduler, and
test-harness implementation. Record those with rationale as implementation needs
arise. No permanent internal choice has been made by listing it here.
