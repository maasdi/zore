# Interface conformance cases

Authority: spec §22.2, with §3.16, §3.17, §10.3, §15.1, and §41.4. Rows marked
*pending* are specified behavior the compiler rejects for now with a "not
supported yet" diagnostic; every other row has an executable counterpart in
`tests/typecheck/check.rs`, `tests/codegen/native.rs`,
`tests/packages/packages.rs`, or `tests/selfhost/parser.rs`.

## Declarations

| Scenario | Expected result |
| --- | --- |
| `type Reader interface { Read(buf mut []byte) (int, error) }` | Declares an interface type |
| Entries with `mut`, `own`, and no receiver mode | Shared, mutable, and consuming receivers |
| Entries on one line separated by `;` | Valid |
| `type Empty interface {}` | Reject: an interface lists at least one method |
| Two entries named `Read` | Reject as a duplicate method |
| An entry named `drop` or `clone` | Reject |
| A method declared on an interface type | Reject |
| `type Named Reader` | Reject: a named type needs a predeclared base |
| `let interface = 1` | Reject: `interface` is a keyword |
| An `async` entry | *Pending:* rejected as not supported yet |

## Satisfaction

| Scenario | Expected result |
| --- | --- |
| A struct or named type with every entry's method | Satisfies the interface |
| Shared entry served by a `mut` method | Reject, naming both receiver modes |
| `mut` entry served by a shared method | Valid |
| `own` entry served by a shared, `mut`, or `own` method | Valid |
| A method with different parameter or result types | Reject: different signature, showing both |
| A missing method | Reject: names the method |
| A predeclared type such as `int` | Satisfies no interface |
| An interface whose entries cover another's | Converts to it |
| An unexported entry served from another package | Reject |

## Borrowed and owned values

| Scenario | Expected result |
| --- | --- |
| A value passed to `r Reader` | Borrowed for the call; nothing moves |
| A value passed to `r mut Reader` | Exclusive borrow; needs a mutable place |
| A `let` binding passed to a `mut` interface parameter | Reject |
| `let c Counter = value` | Moves a Move value, copies a Copy value |
| A borrowed interface value returned or stored | Reject: it does not own the value inside |
| A shared borrowed value passed to a `mut` interface parameter | Reject |
| A value holding a slice converted to an owned interface value | Reject |
| A `mut` entry called through a shared borrowed value | Reject |
| An `own` entry called through a borrowed value | Reject |
| An `own` entry called on an owned value | Consumes it; a later use is a use of a moved value |
| An owned value in a field, `Array<T>`, map value, channel, `Mutex<T>`, or task | Valid; the value inside is destroyed exactly once |
| A borrowed interface value passed to a spawned call | Reject |
| A view returned by a method through an interface | Keeps the receiver borrowed while the view is used |
| Two `mut` interface arguments borrowing one place | Reject |

## Restrictions

| Scenario | Expected result |
| --- | --- |
| `a == b` on interface values | Reject |
| `println(c)` | Reject |
| `clone(c)` | Reject |
| `nil` as an interface value | Reject |
| An interface map key | Reject |
| `c.Count` without a call | Reject |
| `Counter(value)` | Reject: an interface is not called |
| A call through an empty interface value from a closed channel | Panics with the call's location |
| A call through an interface value inside an `async func` | *Pending:* rejected as not supported yet |
