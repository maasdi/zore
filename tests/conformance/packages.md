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
| Entry folder whose package is not `main` | Reject |
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

| Scenario | Expected result |
| --- | --- |
| `strings.Contains`, `HasPrefix`, `HasSuffix`, `Index` | As in §37.2 |
| `strings.Upper`, `Lower`, `TrimSpace` | Unicode case mapping and white space trimming |
| `strings.Repeat("ab", 3)` and with a negative count | `ababab`; the negative count panics |
| `strings.Replace` with an empty `old` | Matches before each character and at the end |
| `strings.Split` with `","`, with `""`, and on `""` | Pieces, characters, one empty piece |
| `strings.Join(parts[:], ", ")` | Elements joined; no elements give `""` |
| `strconv.Itoa` of positive, negative, and the minimum `int` | Decimal text |
| `strconv.Atoi` of valid text, text with a sign, junk, empty, and too large | Value and `nil`, or `0` and the documented error |
| `strconv.FormatBool` and `ParseBool` | `true`/`false` text and errors |
| Unused `error` from `Atoi` | Reject, as for any error value |
| A function declaration without a body in user code | Reject |
