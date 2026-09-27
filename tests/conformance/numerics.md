# Numeric type and arithmetic conformance cases

Authority: spec §3.11–3.14, §6.5–6.6, §7.6, and §10.2. These are pending type,
constant-evaluation, lowering, and runtime cases; they are not executable tests
or passing coverage.

| Scenario | Expected result |
| --- | --- |
| `int`, `int64`; `uint`, `uint64`; `byte`, `uint8` | Each pair has the same type identity and width |
| Same types compiled on hosts with different pointer widths | Same widths and behavior |
| `rune` versus `uint32` | Distinct types despite related representable values |
| Rune literal at U+0000, U+10FFFF | Valid Copy rune values |
| Rune conversion into surrogate range or above U+10FFFF | Reject constant; runtime conversion panics |
| Uncontextualized integer literal | Defaults to `int`/`int64` |
| Uncontextualized float literal | Defaults to `float64` |
| Representable `let small uint8 = 42` | Contextually type literal as uint8 |
| `let tooLarge uint8 = 256` | Compile-time range error; no truncation/wrap |
| Exact untyped constant arithmetic later assigned to narrow type | Evaluate exact result, then validate range |
| Runtime-only expression in a constant initializer | Reject as non-constant |
| `int64(uint8Value)`, `float32(0.5)` | Explicit conversions |
| Typed `int32` value used where `int64` expected | Reject implicit numeric conversion |
| Integer conversion out of range | Compile-time error for constant; runtime panic otherwise |
| Float-to-integer finite fractional conversion | Truncate toward zero, then range-check |
| Float-to-integer NaN or infinity | Reject constant or panic at runtime |
| Integer/float narrowing with representable finite range | Round-to-nearest, ties-to-even |
| Float narrowing whose finite magnitude exceeds destination range | Compile-time error or runtime panic |
| Float underflow below normal range | Follow IEEE subnormal/signed-zero behavior |
| `int + int`, `uint * uint` | Result has operand type if checked result fits |
| Mixed typed `int32 + int64` or signed + unsigned | Reject without explicit conversion |
| Constant integer overflow | Compile-time error in every build mode |
| Runtime integer overflow | Panic consistently in debug/release builds |
| `-7 / 3`, `-7 % 3` | `-2`, `-1`; remainder has dividend sign |
| Integer divide/remainder by zero | Constant error or runtime panic |
| Minimum signed integer divided by `-1` | Overflow error/panic |
| Constant shift count `-1` or equal to/exceeding width | Compile-time error |
| Runtime negative or out-of-width shift count | Runtime panic |
| Unsigned right shift | Vacated bits are zero |
| Signed right shift of negative integer | Vacated bits replicate sign |
| Left shift discarding high bits | High bits discarded, no overflow panic from discarded bits |
| Float division by zero | IEEE infinity/NaN as applicable; no panic solely for division by zero |
| Float overflow in runtime arithmetic | IEEE infinity where specified; no integer-style overflow panic |
| Float constant overflow beyond finite required type | Compile-time error |
| Float constant rounding | IEEE binary32/64 round-to-nearest, ties-to-even |
| `int32(1) < uint8(2)` | Reject mixed-type comparison; explicit conversions still yield different types |
| Bool `==`/`!=` | Allowed |
| Bool `<` or ordering on unrelated types | Reject |
| Rune ordering | Compare Unicode scalar values |
| String ordering | Lexicographic UTF-8 byte order |
| Struct/map/function comparison without a rule | Reject |
| Numeric arithmetic result used later | Retains primitive Copy semantics |

Use exact boundary values for each integer width. Run overflow tests in every
build mode once runtime execution exists. Keep zero-value rules, string storage,
and unlisted constant-expression operators separate from this suite.

## Shift boundaries and constant/runtime agreement (§6.6, Q11)

| Scenario | Expected result |
| --- | --- |
| `uint8(128) << 1` | uint8 zero; no overflow diagnostic |
| Runtime uint8 value 128 shifted left by 1 | Zero in every build mode |
| `int8(64) << 1` | int8 -128; signed bit pattern is reinterpreted |
| `int8(-1) << 1` | int8 -2 |
| `int8(-2) >> 1` | int8 -1 |
| `uint8(128) >> 7` | uint8 1 |
| `uint8(1) << 7` | uint8 128 |
| Signed/unsigned value shifted by zero | Same value and type |
| `uint8(1) << 8`, `uint8(1) >> 8`, or count -1 | Compile-time error; both directions validate counts |
| Runtime negative count or count equal to/greater than width | Panic; never mask the count |
| `uint8(1) << uint64(7)` | uint8 128; count need not have left operand's type |
| `uint8(1) << 1.0` | Valid (§6.7): integral untyped float count; uint8 2 |
| `uint8(1) << 1.5` | Reject; count is not representable as an integer |
| `var bits uint8 = 128; bits <<= 1` | bits becomes zero; target evaluated once |
| `const wide = 128 << 1; let small uint8 = wide` | Reject; exact untyped 256 does not fit uint8 |
| `const bits = uint8(128) << 1` | Typed uint8 constant zero |
| `const n = 1 << 64; let value uint64 = n` | Reject; count is not less than required width (result also does not fit) |
| Checked uint8 arithmetic 128 + 128 | Constant error/runtime panic; shift truncation does not change arithmetic overflow |

Run the typed boundary cases for both constant evaluation and runtime inputs,
in debug and release modes, once the compiler supports them. These are pending
expectations, not executed Zore programs.
