# fmt conformance cases

Authority: spec §37.2 (`zore/fmt`), with §15.1 and §37.1. Every row has an
executable counterpart in `tests/typecheck/check.rs` or `tests/codegen/native.rs`.

## Printing

| Scenario | Expected result |
| --- | --- |
| `fmt.Println("sum:", 3, true, 2.5, 'é')` | `sum: 3 true 2.5 é` and a line feed |
| `fmt.Print("a", "b", 1, 2, "\n")` | `ab1 2`: spaces only between two non-strings |
| `fmt.Println()` | A line feed |
| A struct with `String() string`, a named float, and a generic instance with `String` | Their `String` text, and the float's default text |
| `fmt.Sprint`, `Sprintln`, `Sprintf` | The same text, returned |

## Directives

| Scenario | Expected result |
| --- | --- |
| `%s %d %.2f %x %X %o %b %q %c %t` | Go's text for each, with `%c` and `%q` on runes |
| `%5d`, `%-5d`, `%05d` of 42 and -42, `%6s`, `%-6s`, `%8.3f` | Padded with spaces, on the right, or with zeros after the sign |
| `%v`, `%s`, `%q` of a `String()` type | Its text, its text, its text quoted |
| `%v` of a `nil` error, `%e` and `%g` of floats, `%v` of `uint8` and `%d` of `int8` | `<nil>`, `1.234500e+03`, `1.2e-05`, `200`, `-5` |
| `%%` | `%` |
| `%d` of a `string` | Reject, naming the directive and type |
| More or fewer arguments than directives | Reject, counting both |
| A format that is not a constant | Reject |
| `%w` outside `Errorf`, or twice in one `Errorf` | Reject |
| A struct without `String()` | Reject |
| An unknown verb, a format ending in `%`, a precision on `%d` | Reject |
| `fmt.Println` used as a value, `fmt.Printf()` without a format | Reject |

## Errors

| Scenario | Expected result |
| --- | --- |
| `fmt.Errorf("loading %q: %w", "cfg", NotFound)` | Message `loading "cfg": not found`; `errors.Is(err, NotFound)` is `true` |
| `fmt.Errorf("code %d", 7)` | Message `code 7` with no cause |
