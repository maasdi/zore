# Zero value and `nil` conformance cases

Authority: spec §41.4, with cross-references §5.4, §8.4, §15.6, §18.9, §19.8,
§19.12, §13.3. These are mostly pending type-checking, lowering, and runtime
cases, not executable tests. The `Array<T>` and slice zero values produced by
`?` result filling are covered in `tests/codegen/native.rs`: an empty array
drops safely, and a zero slice has no backing loan and panics on indexing.

| Scenario | Expected result |
| --- | --- |
| `bool` zero value | `false` |
| Integer/`byte` zero value (any width, signed/unsigned) | `0` |
| `float32`/`float64` zero value | Positive zero (`+0.0`) |
| `rune` zero value | `U+0000` |
| `string` zero value | `""`, valid, length zero |
| Struct zero value | Each field set to that field's own zero value, recursively |
| Nested struct-in-struct zero value | Recurses through every nested field |
| `[T; N]` zero value | `N` elements, each the zero value of `T` |
| `[]T` or `mut []T` zero value | Empty, valid view of length zero with no backing-storage loan; not a distinct "nil slice" state |
| `Array<T>` zero value | Empty, valid, owned array with zero elements; not a distinct "nil array" state |
| `map[K]V` zero value | Empty, valid, owned map with zero entries; not a distinct "nil map" state |
| `error` zero value | `nil` |
| `Task<...>` zero value | `nil` |
| `channel<T>` zero value | An always-closed, empty channel; not `nil` |
| Drained closed channel receive, struct element type | `value` is the struct's zero value, `ok == false` |
| Drained closed channel receive, `Array<T>`/`map[K]V` element type | `value` is empty-but-valid, `ok == false` |
| Drained closed receive from a `channel<channel<int>>` | `value` is an always-closed channel; receiving from it returns `(0, false)` |
| `var err error = nil` | Valid; zero value literal for `error` |
| `var task Task = nil` | Valid; zero value literal for `Task` |
| `var ch channel<int> = nil` | Compile-time error: channels have no `nil` state |
| `var n int = nil` | Compile-time error: `nil` not valid for numeric types |
| `var s string = nil` | Compile-time error: `nil` not valid for `string` |
| `var arr Array<int> = nil` | Compile-time error: `nil` not valid for `Array<T>`; use the empty zero value instead |
| `var m map[string]int = nil` | Compile-time error: `nil` not valid for `map[K]V` |
| `var sl []int = nil` | Compile-time error: `nil` not valid for `[]T` |
| `task == nil`, `err == nil` | Allowed equality comparisons |
| `ch == nil` | Compile-time error: channels are not comparable |
| `arr == nil`, `m == nil`, `n == nil` | Compile-time error: type does not admit `nil` comparison |
| `nil` error assigned then left unused | Still subject to the error-result-use rule (§15.6); not exempt because it is `nil` |
| Zero-valued struct construction via drain/lookup | Does not invoke user-defined construction logic or bypass field visibility rules |
| Zero-valued `Task<...>`/`channel<T>` cleanup | No buffered values or running work; cleanup is a no-op |

Keep constant-expression coverage, string encoding/representation, and numeric
corner cases separate from this suite. Operations on zero-value channels and
`nil` tasks are covered in `tests/conformance/concurrency.md`.

## Resource zero-state contract (§41.4, §14.3)

These are runtime/API-contract cases, not claims that the compiler proves
arbitrary destructor code correct. Use an instrumented resource API to count
acquisitions and releases when executable tests become available.

| Scenario | Expected result |
| --- | --- |
| Drained channel receive produces a struct with custom drop | Custom drop and field cleanup still run normally; empty state releases no acquired resource |
| `?` fills an enclosing function's resource result with its zero value | Harmless empty resource plus propagated error; later cleanup releases nothing |
| Nested struct or fixed array contains zero resources | Every element/field is harmless to destroy recursively |
| Resource tracks `acquired bool` and `id int`; zero state has acquired false | No resource release, even if numeric id zero is a valid external handle |
| Successfully acquired resource has acquired true and id zero | Release that resource exactly once on cleanup |
| Failed acquisition leaves acquired false | Cleanup does not release an unacquired handle |
| Drop body is incorrectly written to release id unconditionally | Violates resource API contract; not necessarily a compile-time error; checked runtime behavior still applies |
| Empty resource operation requires an acquired resource | Documented error or panic allowed; empty-state destruction itself must be harmless |
| Built-in zero construction of a private-field resource | Allowed; field privacy does not exempt the resource from the zero-state contract |
| Missing map removal produces a zero resource | `let found, resource = resources.remove(key)` returns false and a harmless empty resource under §13.3; normal cleanup still runs |
