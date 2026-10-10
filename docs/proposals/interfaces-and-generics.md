# Q42 proposal: interfaces and generics

Status: interfaces ACCEPTED and locked in spec §22.2, which is authoritative
for them; an owned interface value cannot hold a view, a narrowing of section 1
below. Generic functions are ACCEPTED and locked in spec §22.1, which is
authoritative for them, with the narrowings recorded in Q42. Generic types are
still PROPOSED: §40 and §41.9 keep them out of the language until a later
specification change under §53. Issue #88.

Scope: structural interfaces, generic functions and types, whether `error`
becomes an interface, their ownership, error, and async rules, compiler impact,
and the standard library work they unlock. Out of scope (later decisions, listed
at the end): type assertions and type switches, a general `any` value type,
variadic parameters, `fmt`, error wrapping, generic interfaces, union
constraints, and explicit type arguments at calls.

## Baseline today

The standard library refactor (Q37) had to work around the missing features:

- No shared reader or writer type. `bufio.NewReader` takes `own os.File`, so a
  `net.Conn` cannot be buffered, and every function that consumes bytes picks
  one concrete source.
- `sort` has only `Ints` and `Strings`. There is no `sort.Slice`, `slices`, or
  `maps` package, and helpers such as `Contains` exist once per element type.
- `error` is one concrete type (§15.1), with no structured errors or wrapping.
- `println` takes one printable value. There is no general printing.

What already exists and is reused below: methods on struct and named types
(§9.1, §8.5); closures that are borrowing or owning depending on where they go
(§16.4); return-borrow contracts that follow views through any value (§11.7);
the parameter modes shared, `mut`, and `own` (§7.3); the type-argument syntax of
`Array<T>`, `channel<T>`, `Mutex<T>`, and `Task<...>`; and the future-reserved
word `interface` (§3.16).

## Decisions requested

1. Interfaces are structural method sets declared with `type Name interface`.
   A type satisfies one by having the methods; there is no `implements`.
2. An interface value is **borrowed** when it is a shared or `mut` parameter, and
   **owned** everywhere else, the same split closures already use.
3. Generic functions and types use angle brackets, `func Max<T ordered>(...)`
   and `type Stack<T any> struct`, matching `Array<T>`.
4. Type arguments of a call are always inferred. There is no `Max<int>(...)`
   call syntax.
5. Four predeclared constraints, `any`, `copyable`, `comparable`, and
   `ordered`, plus any interface type as a constraint.
6. Generic bodies are checked once, at the declaration. The compiler then
   makes one copy per set of type arguments (monomorphization); there are no
   runtime dictionaries.
7. `error` stays one concrete Copy type. Wrapping is a later library decision.
8. A call through an interface behaves exactly like a direct call of the
   concrete method, including pausing a task inside async code when the method
   waits.

## 1. Interfaces

### Declaration

```ore
type Reader interface {
    Read(buf mut []byte) (int, error)
}

type Writer interface {
    Write(data []byte) (int, error)
}

type Closer interface {
    own Close() error
}

type ReadCloser interface {
    Read(buf mut []byte) (int, error)
    own Close() error
}
```

Each entry is a method name, its parameters with their modes, and its results,
as in a method declaration without the receiver. The receiver's mode is written
before the name: nothing for shared, `mut` for a mutable borrow, `own` for
ownership transfer. `async` may also come first (`async Fetch(id int) (User,
error)`). An interface declares at least one method. Names and parameter names
follow the usual rules; an unexported method can only be satisfied inside its
package. Embedding one interface in another (`type ReadCloser interface {
Reader; Closer }`) is not part of this step; list the methods.

`drop` and `clone` cannot appear in an interface. An interface is a type
declared in its package, exported by the uppercase rule like any type. The
keyword `interface` moves from the future-reserved list (§3.16) to the keyword
list (§3.17).

### Satisfying an interface

A type `T` satisfies interface `I` when, for every method of `I`, `T` has a
method with the same name, the same parameter types and modes in order, the same
results, and the same `async` property, and a receiver mode the interface
allows:

| Interface entry | Accepted receivers of `T`'s method |
| --- | --- |
| shared | shared |
| `mut` | shared or `mut` |
| `own` | shared, `mut`, or `own` |

The rule is the one a caller already lives by: holding a value exclusively, or
owning it, is enough to make any weaker call.

Only types that can have methods satisfy an interface: struct types, named types
(§8.5), and instances of generic types (section 2). Predeclared types such as
`int` and `string` have no methods and satisfy none. There is no declaration
that says "T implements I"; the check happens where a value is converted. A
generic type instance satisfies an interface when its methods, after the type
arguments are substituted, do.

### Interface values: borrowed and owned

Where a value of interface type appears decides what it is, mirroring
borrowing and owning closures (§16.4):

| Position | The interface value is | It can call |
| --- | --- | --- |
| Parameter `r Reader` | a shared borrow of the argument | shared methods |
| Parameter `r mut Reader` | an exclusive borrow of the argument | shared and `mut` methods |
| Parameter `r own Reader`, a local, a field, a result, an `Array<T>` or channel element, a map value | an owned value | shared methods; `mut` methods through a mutable place; `own` methods, which consume it |

```ore
func fill(r mut io.Reader, buf mut []byte) (int, error) {
    return r.Read(buf)
}

var file = os.Open("notes.txt")?
var conn = net.Dial("tcp", "localhost:8080")?
var buf = [byte; 4]{0, 0, 0, 0}
let n, err = fill(file, buf[:])     // borrows `file` exclusively for the call
let m, err2 = fill(conn, buf[:])    // the same function now reads a connection
```

Conversion is implicit wherever the target type is an interface and the source
type satisfies it: an argument, an initializer with a declared type, an
assignment, a return, a field value, `push`, or a send. The source can be a
concrete value or another interface value whose methods include the target's
(an `io.ReadCloser` can be passed as an `io.Reader`). There is no conversion
back to the concrete type in this step.

- **Borrowed.** Passing a value to a shared or `mut` interface parameter
  borrows it exactly as passing it to a parameter of its own type would. A
  `mut` parameter needs a mutable place (§11.6). Nothing is copied or moved, and
  nothing is allocated.
- **Owned.** Converting into an owned interface value moves a Move source or
  copies a Copy source into storage the interface value owns. An owned
  interface value is always Move, even when it holds a Copy value, for the same
  reason a closure is: the compiler no longer knows what is inside.

```ore
type Logger struct {
    out io.Writer           // a field: owned
}

func NewLogger(out own io.Writer) Logger {
    return Logger{out: out}
}

let logger = NewLogger(os.Stdout())   // the file moves into the logger
```

Interface types are not comparable, cannot be map keys, cannot be printed, have
no zero value and no `nil` (a binding must be initialized, as for function
types), and do not support `clone`. A shared `[]Reader` lets you call shared
methods on its elements; a `mut []Reader` allows `mut` methods too.

### Ownership

- **Calls.** A method call through an interface value uses the value with the
  entry's receiver mode, and the arguments follow ordinary parameter rules. A
  call to an `own` method consumes the interface value; the dynamic method then
  receives the concrete value by ownership, or by borrow when its receiver is
  weaker, and the storage is destroyed after the call.
- **Drop.** Destroying an owned interface value destroys the value inside it
  exactly once, running a custom `drop` if the concrete type has one, and then
  frees the storage. A borrowed interface value destroys nothing.
- **Views.** An interface value made from a value that holds views keeps those
  views' provenance (§11.7), as a closure keeps its captures'. It cannot outlive
  the storage they borrow.
- **Results that are views.** A method result that holds a view is treated, at
  a call through the interface, as borrowing from the receiver and from every
  borrowed parameter. A concrete method satisfies the entry only if its own
  inferred contract stays within that.
- **Tasks and channels.** An owned interface value can be passed to `go` as an
  `own` argument, captured by a spawned closure, or sent on a channel only when
  the value inside holds no views, the rule §18.4 uses for owning closures. A
  borrowed interface parameter cannot be captured by a spawned task, like any
  borrowed parameter.

### Errors and async

An interface adds no implicit error propagation; `?` works on a call through an
interface exactly as on any call whose last result is `error`. A panic inside a
dynamic method unwinds through the caller with ordinary cleanup.

An `async` entry is satisfied only by `async` methods, and a call through it
must be awaited or spawned with `go` (§17.8), matching async function values
(Q36).

A call through a non-`async` entry behaves exactly like a direct call of the
concrete method. Inside an `async func`, or in a standard library function that
waits, a direct call of a method that waits, such as `os.File.Read` or
`net.Conn.Read`, suspends the task and lets other tasks run (§17.7, §37.3). The
same call through an `io.Reader` suspends the same way. In a plain function it
waits on the spot, as the direct call does. A method that never waits completes
without suspending through either path.

```ore
async func serve(conn own net.Conn) {
    var lines = bufio.NewScanner(conn)  // conn is stored as an io.Reader
    for lines.Scan() {                  // suspends while waiting; other tasks run
        println(lines.Text())
    }
}
```

## 2. Generics

### Syntax

```ore
func Max<T ordered>(a T, b T) T {
    if a > b {
        return a
    }
    return b
}

func Contains<T comparable>(items []T, target T) bool {
    for item in items {
        if item == target {
            return true
        }
    }
    return false
}

type Stack<T any> struct {
    items Array<T>
}

func (s mut Stack<T>) Push(value own T) {
    s.items.push(value)
}

func (s Stack<T>) Len() int {
    return s.items.len()
}
```

Every type parameter has exactly one constraint. Type parameters are written in
angle brackets after the function or type name, consistent with `Array<T>`. A
method of a generic type names the type with its parameters (`Stack<T>`) and
uses them in its signature. A method cannot declare type parameters of its own,
which keeps interface satisfaction a plain comparison.

Generic types are used with type arguments: `Stack<int>` in a type, and
`Stack<int>{}` as a literal, which the parser already handles for `Array<int>{}`.
Async generic functions are allowed.

### Type arguments are inferred

A call to a generic function never writes type arguments. They come from the
argument types:

```ore
let biggest = Max(3, 9)                  // T = int
let found = Contains(names[:], "Ada")    // T = string
```

When the arguments do not decide a type parameter, the call is an error that
names it. A generic function used as a value is instantiated by the declared
type it is assigned to: `let pick func(int, int) int = Max`.

This rule avoids a real parsing problem. Comparisons do not chain (§7.6), but
`f(a < b, c > (d))` is already two valid arguments, so `Max<int>(a, b)` cannot
be told apart from comparisons without knowing what `Max` is. Literals do not
have the problem, because a `{` cannot follow a comparison.

### Constraints

| Constraint | Allowed type arguments | What the body may do with a `T` |
| --- | --- | --- |
| `any` | any type that can be a parameter type | pass, return, store, and move it |
| `copyable` | every Copy type (§10.2, §8.3, §10.4) except `mut []T` and types holding one | the above, and copy it |
| `comparable` | `bool`, integer types, `rune`, `string`, and named types built on them | the above, and `==`, `!=`, map keys |
| `ordered` | integer and float types, `rune`, `string`, and named types built on them | the above, and `<`, `<=`, `>`, `>=` |
| an interface type | types that satisfy it | the above, and call its methods |

`comparable` is exactly the set of map key types (§13.3), so `map[K]V` with
`K comparable` is always valid, and floats are left out for the same NaN reason.
Every `comparable` and `ordered` type is also `copyable`. A copied view keeps its
provenance (§11.7). `any`, `copyable`, `comparable`, and `ordered` become
predeclared names (§3.17), usable only as constraints. `any` is not a value
type.

```ore
func First<T copyable>(items []T) T {
    return items[0]                  // accepted: T values are copied
}

func Last<T any>(items []T) T {
    return items[items.len() - 1]    // rejected: T may be a Move type
}

println(First(numbers[:]))           // T = int
First(files[:])                      // rejected: `os.File` is not copyable
```

### Ownership in generic code

The body is checked once, without knowing `T`, so it must be correct for every
allowed type argument:

- Under `copyable`, `comparable`, and `ordered`, every allowed type is Copy,
  so `T` values are copied.
- Under `any` or an interface, `T` is treated as Move. A value can be moved but
  not implicitly duplicated, so reading `items[0]` out of a `[]T` into a binding
  is rejected, as it is for any Move element. `clone(value)` on a `T` is not
  available in this step.
- Borrowing, `mut` and `own` parameters, closures, views, and task rules apply
  to `T` unchanged. A view inside a type argument keeps its provenance through
  generic code.

An instance with a Copy type argument is still checked by these rules, which is
safe because they are stricter than what it needs.

### Predeclared parameterized types

`Array<T>`, `channel<T>`, `Mutex<T>`, `Task<...>`, and `map[K]V` stay built in,
with their own rules. They accept type parameters as arguments like any other
type, for example `Array<T>` inside `Stack<T>`. Their existing special methods
(`push`, `pop`, `len`, `remove`) are not redefined in Zore.

## 3. `error` stays concrete

`error` stays one concrete predeclared type: Copy, with a message, compared by
content (§15.1). Making it an interface would make every error value Move,
because owned interface values are Move, and every program that copies an error
or compares it with `==` would break. Structured errors also need a way to look
inside, such as type assertions, which this step does not add.

Wrapping and cause chains (`errors.Is`, `errors.Unwrap`, `errors.Join`) are a
separate library decision that can be made without interfaces: the error value
can carry an optional cause. Structured payloads wait for type assertions.

## 4. Compiler impact

- **Lexer and both parsers.** `interface` becomes a keyword. Parse interface
  declarations; type parameter lists on functions, types, and methods; type
  arguments on user type names in types and literals. The self-hosted parser in
  `compiler-zore/` gets the same forms, and the parser comparison test covers
  them.
- **Resolver.** Declare interface types and type parameters in scope. Resolve
  `any`, `comparable`, and `ordered` as constraints only.
- **Types.** New type kinds for interface types, type parameters, and generic
  type instances, interned like other types. A satisfaction check with
  diagnostics that name the missing or mismatched method.
- **Checker (HIR).** Insert implicit conversions to interface types. Check
  method calls on interface values and on type parameters against the entry or
  constraint. Infer type arguments at calls. Check generic bodies once.
- **MIR.** A conversion rvalue (borrowed or owned) and a dynamic method call
  callee. A generic body stays generic in MIR until after ownership checking.
- **Ownership and regions.** Interface values are loan holders, like closures.
  Type parameters classify by constraint. Return-borrow contracts get the
  conservative rule for view results.
- **Drop insertion and code generation.** Monomorphize after ownership
  checking: each function or type instance gets its own MIR copy, then drop
  insertion and code generation, cached by type arguments. An interface value is
  a data pointer plus a method table holding one entry per method, the
  destructor, and a type identity reserved for future type assertions. Owned
  values store their data in heap storage freed by the destructor, as owning
  closure environments are.
- **Async lowering.** `async` entries suspend like async function values. Every
  other call through an interface is a possible suspension point in an async
  body or a waiting standard library function. Each method placed behind an
  interface gets two method-table entries: its ordinary form for plain callers,
  and a resumable form for suspending callers. For a method that never waits,
  the resumable form runs the ordinary one and completes in one step. Both are
  generated only for methods actually converted to an interface.
- **Runtime.** No new runtime services; storage uses the existing allocator.

## 5. Delivery plan

One pull request per step, each with its spec change, conformance cases,
paired accept and reject tests, and the self-hosted parser kept in step:

1. This proposal.
2. Interfaces: declarations, satisfaction, borrowed and owned values, dynamic
   calls, drop, and the task and channel rules.
3. Generic functions and the three constraints, plus interface constraints.
4. Generic types and their methods.
5. Library: a new `zore/io` package with `Reader`, `Writer`, `Closer`,
   `ReadWriter`, `ReadCloser`, `Copy`, and `ReadAll`; `bufio` built on
   `io.Reader` and `io.Writer`, so it works over files and connections;
   `sort.Slice`; new `slices` (`Contains`, `Index`, `Sort`, `Reverse`, `Max`,
   `Min`) and `maps` (`Keys`, `Values`) packages. `strings` and `bytes` stay
   separate, since `string` and `[]byte` are different types and folding them
   needs a union constraint.

## Later decisions, not part of this proposal

- Type assertions and type switches, and with them a general `any` value type,
  `errors.As`, and structured error payloads.
- Variadic parameters and a `fmt` package with general printing.
- Error wrapping and cause chains.
- Generic interfaces (`Iterator<T>`), interface embedding, union constraints
  (`int | float64`), a constraint for clonable types, and methods with their own
  type parameters.
- Explicit type arguments at calls, if inference proves too weak, with a syntax
  that avoids the comparison ambiguity.

## Pending conformance plan

On acceptance, add paired cases for: declaring and satisfying interfaces,
including each receiver-mode row and `async` entries; missing, mismatched, and
unexported methods; borrowed and owned conversions; `mut` interface parameters
needing a mutable place; interface-to-interface conversion; consuming `own`
methods; drop of the value inside exactly once on every exit path; views kept
through interface values; rejected spawns and sends of values holding views;
rejected `==`, `nil`, `clone`, and printing; calls through an interface that
suspend a task in async code, wait in plain code, and complete at once for
methods that never wait; generic functions with each constraint, including
`copyable` accepting copies and rejecting Move types; inference failures; Move
rules under `any`; generic types, their literals, and their methods; generic
instances satisfying interfaces; and generic async functions. These are proposed
cases, not executable or passing tests.
