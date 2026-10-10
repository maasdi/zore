# Package and import conformance cases

Authority: spec §3.20, §4.1, and §37.2. Every row has an executable
counterpart in `tests/driver/cli.rs`, `tests/typecheck/check.rs`, or
`tests/codegen/native.rs`, and `examples/packages` runs natively.

## Packages and projects

| Scenario | Expected result |
| --- | --- |
| Two files in one folder, both `package main` | One package; each sees the other's declarations |
| A name declared in two files of one package | Reject as a duplicate |
| Files in one folder declaring different package names | Reject, naming both |
| Entry folder whose package is not `main` | `check` accepts it; `build` and `run` reject it as not executable |
| Sibling folders | Separate packages; not part of the entry package |
| `zore.toml` found in a parent folder | That folder is the project root |
| No `zore.toml` anywhere above | Project has no name; only `zore/...` imports resolve |
| Package folder with no `.ore` files | Reject the import |
| Package whose clause differs from its folder name | Reject |

## Import paths and use

| Scenario | Expected result |
| --- | --- |
| `import "myapp/shapes"` in project `myapp` | Loads folder `shapes` below the root |
| `import "other/shapes"` with a different first segment | Reject: unknown package |
| `import "zore/nope"` | Reject: no such standard package |
| Nested path `"myapp/geo/shapes"` | Loads `geo/shapes`; the name is `shapes` |
| `shapes.Area(c)` for an exported function | Valid |
| `shapes.area(c)` for an unexported function | Reject, naming the package |
| `shapes.Missing` | Reject, naming the package |
| Exported constant used across packages | Valid, including in constant expressions |
| `var c shapes.Circle` and `shapes.Circle{Radius: 2}` | Valid when every field is exported |
| Struct literal naming an unexported field from another package | Reject |
| Reading or writing an unexported field of an imported value | Reject |
| Calling an exported method of an imported type | Valid |
| Calling an unexported method of an imported type | Reject |
| Declaring a method on an imported type | Reject |
| `a.T` assigned to `b.T` | Reject: different types |
| Unused import | Reject |
| Same path imported twice, or two imports with one name | Reject |
| Import name equal to a package-level declaration | Reject |
| Package importing itself, or a cycle through others | Reject, listing the chain |
| Importing the `main` package | Reject |
| Import alias, `.` import, `_` import, grouped import | Reject |
| Package-level `let` or `var` in an imported package | Reject |
| Same type name in two packages | Distinct types; both usable in one program |
| Custom `drop` on an imported type | Runs at the usual points |
| A package imported by two files or two packages | Checked and linked once |
| Diagnostic inside an imported package | Names that file, line, and column |

## Standard packages

Authority: spec §3.20 and §37.2–§37.5.

| Scenario | Expected result |
| --- | --- |
| `strings.Contains`, `HasPrefix`, `HasSuffix`, `Index`, `LastIndex`, `IndexByte`, `IndexRune`, `Count` | As in §37.2, including empty patterns |
| `strings.ToUpper`, `ToLower`, `EqualFold`, `TrimSpace` | Unicode case mapping, case-insensitive comparison, and white space trimming |
| `strings.Trim`, `TrimLeft`, `TrimRight`, `TrimPrefix`, `TrimSuffix`, `Cut` | Cut sets, affixes, and a separator found or missing |
| `strings.Repeat("ab", 3)` and with a negative count | `ababab`; the negative count panics |
| `strings.ReplaceAll` and `Replace` with a count, with an empty `old` | Matches before each character and at the end, up to the count |
| `strings.Split`, `SplitN`, `Fields` with `","`, with `""`, and on `""` | Pieces, characters, one empty piece; limits; runs of white space |
| `strings.Join(parts[:], ", ")` | Elements joined; no elements give `""` |
| `strings.Builder` writes, `Len`, `String`, `Reset` | The text in order, its byte length, then empty |
| `strconv.Itoa` and `FormatInt` of positive, negative, and the minimum `int` | Digits in the base; base 99 panics |
| `strconv.Atoi` and `ParseInt` of valid text, a sign, prefixes, separators, junk, empty, and too large | Value and `nil`, or `0` and an error naming the quoted input |
| `strconv.FormatBool` and `ParseBool` | `true`/`false` text and errors |
| `strconv.Quote`, `QuoteRune`, and `Unquote` | Literals that decode back to their input; malformed literals are errors |
| `unicode` classes and case mappings of letters, digits, `½`, white space, and controls | As in §37.2 |
| `utf8` lengths, counts, validity, and decoding of valid, invalid, and cut sequences | As in §37.2 |
| `bytes` comparisons and searches, `Buffer` writes and reads to `EOF`, `Truncate` out of range | As in §37.2; the truncation panics |
| `errors.New` and `errors.Is` | Equal messages compare equal |
| `path.Clean`, `Join`, `Split`, `Base`, `Dir`, `Ext`, `IsAbs`, and `filepath.Abs` | As in §37.5 |
| `sort.Ints`, `Strings`, the `AreSorted` checks, and searches | Ascending order and insertion indices |
| `import "zore/os/exec"`, `"zore/path/filepath"`, `"zore/unicode/utf8"` | Used as `exec.`, `filepath.`, and `utf8.` |
| `import "zore/io"`, `"zore/cancel"`, `"zore/os/nope"` | Reject: no such standard package |
| `strings.Upper` | Reject: the package does not declare it |
| Unused `error` from `Atoi` | Reject, as for any error value |
| A function declaration without a body in user code | Reject |

## Package-level values (§3.21)

Covered by `tests/typecheck/check.rs`, `tests/packages/packages.rs`, and `tests/codegen/native.rs`.

| Scenario | Expected result |
| --- | --- |
| `let EOF = error("EOF")`, `let Limit int = 3`, a struct and a fixed array built from earlier values | Accept; read anywhere in the package, including in `async` functions and tasks |
| Initializers that print as they run | Run once each, in source order, before `main` |
| An imported package's values | Initialized before the importer's |
| `config.Port` from another package; `config.secret` | Exported value readable; unexported one rejected |
| `let Early = Late + 1` before `let Late = 2` | Reject: used before it is initialized |
| An initializer that calls or spawns a function reading a later value | Reject, naming the later value |
| An initializer that reaches its own value | Reject |
| `Limit = 4`, `config.Port = 1` | Reject: cannot assign |
| `var Counter = 0`, `let a, b = pair()` | Reject |
| `channel<int>(1)`, `Array<int>{1}`, `mutex(0)`, a function value | Reject: not storable in a package-level value |
| A panic in an initializer | The program ends with status 2; `main` does not run |
| Text built at run time and held by a package-level value | Not reported as a leak |
