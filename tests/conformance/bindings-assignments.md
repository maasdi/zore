# Binding and assignment conformance cases

Authority: spec §5.4–5.7, §3.18, and §7.6. These are pending parser, resolution,
typing, ownership, lowering, and runtime cases, not executable coverage. Define
fixture calls with matching types and effects when the relevant stages exist.

| Source / scenario | Expected result |
| --- | --- |
| `let count = 0`, `var count = 0` | Inferred binding forms |
| `let count int64 = 0`, `var ratio float32 = 0.5` | Explicit type follows name |
| `let count: int64 = 0` | Reject colon annotation syntax |
| `let count int64`, `var count int64` | Reject missing initializer |
| `const limit = 100`, `const limit int64 = 100` | Constant forms; compile-time evaluation required |
| Constant initialized by a runtime-only operation | Reject non-constant initializer |
| `let value, err = load()` | One evaluation, matching result count |
| `var first, second = pair()` | Mutable multiple-result binding |
| `let a, b = 1, 2` | Reject multiple initializer expressions in a binding |
| Multiple binding with per-name or shared type annotation | Reject unsupported typed multiple binding |
| `let _, _ = pair()` | No duplicate binding; discard both results |
| `let a, a = pair()` | Reject duplicate named targets |
| Assign to immutable local or constant | Reject |
| Assign to writable field/index | Apply type, mutability, and ownership checks |
| Assign through shared borrow | Reject invalid write |
| `count += 1` | One target evaluation, binary operation, one store |
| `items[nextIndex()] += rhs()` | Index called once, before RHS; preserve exclusive update access |
| `_ += 1` | Reject compound target with no stored value |
| Each locked compound operator | Corresponding binary type rule; do not permit float remainder or bitwise operations |
| `left, right = right, left` | Retain RHS values before stores; swap Copy values |
| `left, right = pair()` | One RHS evaluation, two stores |
| `left, right, other = pair(), value` | Reject mixed multiple-result expansion |
| Assignment target/result-count mismatch | Reject |
| `a, a = 1, 2` | Reject overlapping targets |
| Targets with potentially aliasing dynamic indexes | Reject unless disjointness is proven |
| Two `_` assignment targets | Allowed; not overlapping places |
| Effectful target and RHS evaluations | Targets left to right, RHS left to right, stores left to right |
| RHS propagates an error | No assignment stores; clean required owned temporaries; preserve earlier effects |
| Replace still-owned resource | Required cleanup of old resource before replacement |
| Swap two distinct owned Move locals | Transfer each once; no duplicate or premature destruction |
| Earlier RHS move invalidates later RHS use or target | Reject invalid ownership use |
| Target retained across await | Require proof of validity/exclusivity across suspension |
| Two named declarations in one scope | Reject with both declaration locations |
| Nested declaration shadows ordinary outer name | Distinct IDs/ownership; outer name visible again after scope exit |
| Nested declaration shadows `println` or `int` | Reject predeclared-name shadowing |

Keep member writability, constant-expression details, declaration scope-entry
points, and panic cleanup tests pending their remaining decisions. Use explicit
event logs and resource-drop counters for sequencing and ownership tests.
