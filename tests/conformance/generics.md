# Generic function and type conformance cases

Authority: spec §22.1, with §3.17, §3.18, §40, and §41.9. Every row has an
executable counterpart in `tests/typecheck/check.rs`, `tests/codegen/native.rs`,
`tests/packages/packages.rs`, or `tests/selfhost/parser.rs`.

## Declarations

| Scenario | Expected result |
| --- | --- |
| `func Max<T ordered>(a T, b T) T` | Declares a generic function |
| Several parameters, `<K comparable, V any>` | Valid |
| An interface type as a constraint, including one from another package | Valid |
| `func F<T>()` | Reject: a type parameter needs a constraint |
| `func F<>()` and `func F<T any,>()` | Reject as syntax errors |
| A constraint that is not `any`, `copyable`, `comparable`, `ordered`, or an interface | Reject |
| `any` used as a type, such as `x any` | Reject: it is a constraint |
| A method with type parameters | Reject |
| A generic function without a body | Reject |
| `func main<T any>()` | Reject |
| A local named `any` or `ordered` | Reject: shadows a predeclared name |
| A generic `async func` | Valid; calls are awaited or spawned |

## Calls and inference

| Scenario | Expected result |
| --- | --- |
| `Max(3, 9)` | `T` is `int` |
| `Max(1.5, 2)` | `T` is `float64` from the first untyped constant |
| `Contains(names[:], "Ada")` | `T` is `string`, taken from inside `[]T` |
| A parameter that no argument decides | Reject: cannot infer `T`, naming it |
| `Max<int>(1, 2)` | Not a call with type arguments: parses as comparisons |
| Arguments that give one parameter two different types | Reject at the second argument |
| A type argument outside the constraint, such as `Array<int>` for `comparable` | Reject, naming the constraint |
| A Move type for `copyable` | Reject |
| A float type for `comparable` | Reject |
| A function type, `mut []T`, or borrowed interface value as a type argument | Reject |
| A type that does not satisfy an interface constraint | Reject, naming the missing method |
| An interface type for its own interface constraint | Valid |
| A type parameter passed on under a constraint that promises as much | Valid |
| `copyable` or `ordered` passed on to `comparable`, or `any` to `copyable` | Reject: floats are `ordered` but not `comparable` |
| A generic function used as a value | Reject: not supported yet |
| A generic function called from another package | Valid |
| `var s Stack<int> = NewStack()`, where no argument decides `T` | `T` comes from the expected type |

## Bodies

| Scenario | Expected result |
| --- | --- |
| `a > b` for `T ordered`, `a == b` for `T comparable` | Valid |
| `a < b` for `T comparable` | Reject |
| `map[K]V` with `K comparable` | Valid |
| Reading `items[0]` out of a `[]T` under `copyable` | Valid: copied |
| The same under `any` | Reject: moving out of a slice |
| Using an `own T` twice under `any` | Reject: use of a moved value |
| An error in a generic function that is never called | Reported |
| A method of an interface constraint called on a `T` | Calls the type argument's method with the entry's receiver mode |
| A method on a `T` whose constraint lists none | Reject |
| A function literal, `go`, or converting a `T` to an interface in a generic body | Reject: not supported yet |
| Printing a `T` | Reject |
| A function that calls itself with `Array<T>` | Reject: endless new type arguments |
| A view passed through a `copyable` `T` | Keeps its borrow |

## Copies

| Scenario | Expected result |
| --- | --- |
| One generic function called with `int` and `string` | Each call runs its own copy |
| A Move type argument with a custom `drop` | Destroyed exactly once |
| A generic call inside a generic function | Gets a copy for the outer type arguments |
| An ownership error found in several copies | Reported once |

## Generic types

| Scenario | Expected result |
| --- | --- |
| `type Stack<T any> struct { items Array<T> }` with methods on `Stack<T>` | Declares a generic struct type and its methods |
| `Stack<int>{...}`, `Stack<Stack<int>>`, and `pkg.Stack<int>` | Distinct struct types, each with its own fields and method copies |
| A generic struct with `map[T]bool` under `T comparable` | Valid |
| `Stack` without type arguments, in a type or a literal | Reject: needs type arguments |
| `Stack<int, int>` | Reject: wrong number of type arguments |
| A type argument outside the constraint | Reject, naming the constraint |
| `Point<int>` for a struct that is not generic | Reject |
| `type Number<T any> int` or a generic interface | Reject: only struct types can declare type parameters |
| `func (s Stack) M()` on a generic type | Reject: the receiver names the type parameters |
| `func (s Stack<int>) M()` | Reject: the receiver lists parameter names, not type arguments |
| A method of a generic type with its own type parameters | Reject |
| Moving a `T` out of `s.items[0]` under `any` | Reject |
| `type Node<T any> struct { next Node<T> }` | Reject: contains itself by value |
| A generic struct whose fields nest it with growing type arguments | Reject |
| A custom `drop` on a generic type | Runs once for each value of each instance |
| A custom `clone` returning `Cell<T>` | Used by `clone(value)` for each instance |
| `Cell<int>` with `Get() T` converted to an interface needing `Get() int` | Satisfies it; `Cell<string>` does not |
| A method of a generic type used as a value | Reject: not supported yet |
| Converting a `Cell<T>` to an interface inside generic code | Reject: not supported yet |
| An `async` method of a generic type, awaited, and `go` on a method of an instance | Valid |
| An unexported field of a generic type from another package | Reject |
