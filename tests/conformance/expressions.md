# Operator and expression conformance cases

Authority: spec §7.5–7.6. These are pending parser, semantic, ownership, lowering,
and runtime cases, not executable coverage. AST expectations are structural;
they do not require the compiler to expose these exact node names.

| Input / scenario | Expected result |
| --- | --- |
| `a + b * c` | Add(a, Multiply(b, c)) |
| `a - b - c` | Subtract(Subtract(a, b), c) |
| `a + b << c` | Add(a, ShiftLeft(b, c)) |
| `a << b & c` | BitAnd(ShiftLeft(a, b), c) |
| `a \| b ^ c` | BitXor(BitOr(a, b), c) |
| `a == b \|\| c && d` | Or(Equal(a, b), And(c, d)) |
| `(a + b) * c` | Multiply(Add(a, b), c) |
| `-f(x).field` | Negate(Field(Call(f, x), field)) |
| `^a`, `a ^ b` | Unary complement versus binary XOR |
| `!(!flag)` | Nested boolean negation |
| `a < b < c`, `a == b == c`, `a < b == c` | Reject unparenthesized comparison chains |
| `a < b && b < c` | Separate comparisons joined by boolean AND |
| `(a < b) == flag` | Explicit grouping; type-check resulting bool operands |
| `await operation()?` | Propagate(Await(Call(operation))) |
| `await object.method()?` | Propagate(Await(MethodCall(object, method))) |
| `await (operation()?)` | Await(Propagate(Call(operation))); type validity checked separately |
| `false && effect()` | Do not call effect |
| `true \|\| effect()` | Do not call effect |
| `true && effect()`, `false \|\| effect()` | Evaluate right side once after left |
| `1 && flag`, `!"text"` | Reject non-bool logic operands |
| Bitwise/complement/shift applied to floats | Reject non-integer operands |
| Float operands to `%` | Reject; remainder is integer-only |
| `"Hello, " + "Zore"` | String concatenation |
| `"count: " + 1` | Reject implicit numeric-to-string conversion |
| `a = b = value` | Reject chained assignment |
| Assignment used as a call argument or operand | Reject assignment expression |
| `x++`, `x--`, `flag ? a : b`, `a ** b` | Reject unsupported operator forms |
| Unary `&x` or `*x` | Reject source-level pointer operators |
| `a & b`, `a * b` with valid numeric types | Ordinary bitwise AND/multiplication |
| Short-circuit RHS contains move or await | No RHS effects on skipped path; analyze ownership on all possible paths |
| Awaited result propagates error after earlier owned temporaries were created | Preserve required cleanup and suspension safety |

Test every precedence boundary and left-associative level with parser cases.
Use event logs for evaluation ordering and explicit synchronization for async
tests. Keep numeric range/rounding, assignment sequencing, full `go` grammar,
and unspecified comparison-type cases pending their separate decisions.
