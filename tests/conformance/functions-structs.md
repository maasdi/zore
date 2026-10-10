# Function, call, and struct conformance cases

Authority: spec §7.8, §8.4, §7.5, and §15.6. These cases await parser, resolution,
typing, ownership, and lowering implementation; they are not passing coverage.

| Scenario | Expected result |
| --- | --- |
| `func add(a int, b int) int { return a + b }` | Valid individually typed parameters |
| Grouped `a, b int` parameter syntax | Reject |
| Default, named-argument, or user variadic syntax | Reject |
| Too few/many call arguments | Reject count mismatch |
| Trailing comma in parameter or argument list | Allow; honor multiline semicolon rules |
| Missing argument between commas | Reject |
| Bodyless user function or method declaration | Reject; illustrative spec signatures do not authorize it |
| Function calls later-declared package function | Resolve forward reference |
| Type references later-declared package type across files | Resolve identity; validate layout separately |
| Duplicate package function names with different signatures | Reject overloading |
| Method on a type from another package | Reject |
| Same method name twice on one receiver type | Reject, regardless of parameter or ownership differences |
| Field and method share a name on one type | Reject member conflict |
| Same method name on different receiver types | Allowed |
| `return pair()` with exact matching declared result list | Evaluate once and forward results |
| Forwarded results have mismatched count/types | Reject |
| `return 1, pair()` where pair has multiple results | Reject mixed result expansion |
| `add(pair())` where pair has multiple results | Reject argument expansion |
| Bind pair results then `add(left, right)` | Allowed with valid types/contracts |
| Call/task creation as statement | Allowed, subject to error-result policy |
| `await operation()` or `operation()?` as call-based statement | Check async/error contracts and remaining error results |
| Bare arithmetic, literal, or local as statement | Reject |
| Ignored owned non-error call result | Perform normal cleanup; do not leak |
| Ignored error call result | Reject per §15.6 |
| Struct initializer supplies every named field once | Allow |
| Positional, missing, unknown, or duplicate field initialization | Reject |
| Empty initializer for empty struct | Allow |
| Fields written in reverse declaration order | Evaluate expressions in written order; place values in resolved fields |
| Trailing comma after final field | Allow |
| Name another package's private field in initializer | Reject; require a defining-package construction API |
| Later field expression propagates error | Clean previously acquired owned values once; no cleanup of uninitialized fields |
| Later field expression awaits | Preserve earlier acquired values and prove borrow validity |
| Unparenthesized struct literal directly in if/for condition | Reject ambiguous form |
| `if (Point{X: 1}).X == 1 { work() }` | Explicit literal boundary; type-check normally |

Use observable field-evaluation logs and resource-drop counters for construction
tests. Keep package variable initialization, built-in signatures, and recursive
layout validity separate. Forwarded Move results must not be duplicated or
destroyed before the caller receives ownership.

## Named types (§8.5)

Covered by `tests/typecheck/check.rs`, `tests/packages/packages.rs`, `tests/codegen/native.rs`, `tests/parser/parser.rs`, and the self-hosted parser comparison.

| Input / scenario | Expected result |
| --- | --- |
| `type Duration int`, `type Name string`, `type Flag bool`, `type Letter rune`, `type Ratio float64` | Accept; each is a new type |
| `type Seconds Duration` | Accept; the base type is `int` |
| `5 * Second` with `const Second Duration = ...` | A `Duration` |
| `d + x` for a `Duration` and an `int` | Reject: mismatched types |
| `func f() Name { return "zore" }` | Reject: a string literal is a `string` |
| `Duration(s)`, `int(d)`, `string(name)`, `Name(s)`, `rune(letter)`, `Letter(code)` | Convert without changing the value |
| `Name(5)`, `Flag(1)` | Reject |
| `Name` concatenation, `len()`, slicing, and a `for` loop over its characters | As for `string`; slicing gives a `Name` |
| `if f && !f` for a `Flag` | Accept as a condition |
| A `Name` as a map key and as an `Array` element | Works like `string` |
| Methods on a named type, including from another package | Called like struct methods; unexported ones are rejected outside the package |
| `func (d mut Duration) drop()` | Reject: `drop` and `clone` are only for struct types |
| `func (i int) Double()` | Reject: methods need a type declared in the package |
| `type A B` with `type B A` | Reject: built on itself |
| `type P Point` for a struct, `type Items Array<int>`, `type E error` | Reject: unsupported base type |
| `type Alias`, `type Alias = int` | Reject at parse time: expected a type |
