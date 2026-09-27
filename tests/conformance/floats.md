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
