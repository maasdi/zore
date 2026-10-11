# Floating-point literal conformance cases

Authority: spec §3.12–3.14, §3.7, and §10.2. These are pending lexer/decoder
cases, not executable coverage. Values describe decimal mathematical values,
not rounded binary representations or a selected target float type.

| Source / scenario | Expected result |
| --- | --- |
| `0.5`, `1.0`, `1e6`, `1.5e-3`, `2E+8` | Valid decimal floats |
| `1e0`, `1.0e0`, `1.0E+0`, `1.0e-0` | Same mathematical value, 1 |
| `01.50`, `00e2` | Decimal values 1.5 and 0 |
| `1_000.25`, `1_000.2_5`, `1_0e+0_2` | Valid separators; values 1000.25, 1000.25, 1000 |
| `1_000.2_5e1_0` | Valid separators in all three digit sequences |
| `1` versus `1.0` and `1e0` | Integer token versus float tokens |
| `.5`, `1.`, `1.e2` as numeric initializers | Reject unsupported float forms |
| `1e`, `1E`, `1e+`, `1.0e-`, `1e++2`, `1e+-2` | Reject malformed exponents |
| `1_.0`, `1._0`, `1.0_`, `1__0.0` | Reject invalid significand separators |
| `1_e2`, `1e_2`, `1e+_2`, `1e2_`, `1e2__0`, `1.0_e2` | Reject invalid exponent-adjacent separators |
| `0x1.0p2`, `0b1.1`, `0o1.1` as numeric initializers | Reject non-decimal float forms |
| Non-ASCII digits in float spelling | Reject |
| `1.field` | Integer, dot, identifier token boundary; no `1.` float token |
| `1e-3` | One float token including exponent sign |
| `1.5` followed by newline/EOF | Insert semicolon per §3.7 |
| `1.5f32`, `1.5float32`, `1e3f64`, `1.0i`, `1.5_name` | Reject suffixes/attached identifier continuations |
| `1_000.25f64` | Valid separators do not permit a suffix |
| `1e3`, `1E+3`, `1.5e-3` | Exponents are valid float syntax, not suffixes |

Assert source spans and malformed-exponent/separator diagnostics. Do not add
target rounding, overflow, or unary-expression expectations until their
rules are locked. At semantic milestones, verify primitive Copy behavior.

## Float text and parsing (§37.1, §37.2)

| Scenario | Expected result |
| --- | --- |
| `strconv.FormatFloat(3.14159, 'f', 2, 64)` | `3.14` |
| `strconv.FormatFloat(1234.5678, 'e', 3, 64)` and `'E'` with -1 | `1.235e+03`, `1.2345678E+03` |
| `strconv.FormatFloat(1234.5678, 'g', 3, 64)` and `strconv.FormatFloat(100.0, 'g', 5, 64)` | `1.23e+03`, `100` |
| `strconv.FormatFloat(v, 'g', -1, 64)` | The same text as `println(v)` |
| `strconv.FormatFloat` with format `'x'` or bit size 16 | Panics naming the bad argument |
| `strconv.ParseFloat("2.5e3", 64)`, `"-2"`, `".5"`, `"7."` | `2500.0`, `-2.0`, `0.5`, `7.0` with `nil` |
| `strconv.ParseFloat("NaN", 64)`, `"-Inf"`, `"infinity"` | Not-a-number, negative and positive infinity |
| `strconv.ParseFloat("1_000", 64)`, `"0x1p-2"`, `""`, `"1e"` | `0` and `strconv.ParseFloat: parsing Q: invalid syntax` |
| `strconv.ParseFloat("1e400", 64)`, `"1e39"` with bit size 32 | `+Inf` and `value out of range` |
| `strconv.ParseFloat("0.1", 32)` | The `float32` nearest 0.1, as a `float64`: `0.10000000149011612` |
| Every finite `float64` printed and parsed back | The same value |

## `zore/math`

| Scenario | Expected result |
| --- | --- |
| `math.Sqrt(2)`, `math.Pi` | `1.4142135623730951`, `3.141592653589793` |
| `math.Floor(-2.5)`, `Ceil`, `Trunc`, `Round` | `-3.0`, `-2.0`, `-2.0`, `-3.0` |
| `math.Sqrt(-1)`, `math.Log(-1)` | `NaN`, without a panic |
| `math.IsInf(math.Inf(-1), -1)`, `math.IsInf(math.Inf(1), 0)`, `math.IsNaN(math.NaN())` | `true` |
| `math.Max(1, math.NaN())` | `NaN` |
| `math.Abs` of a negative zero | `0.0` |
