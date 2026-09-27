# Constant-expression conformance cases

Authority: spec §5.3, with cross-references §6.5–6.6, §7.6, §41.4. These are
pending resolution, type-checking, and constant-evaluation cases; they are not
executable tests or passing coverage.

| Scenario | Expected result |
| --- | --- |
| `const x = 42` | Valid; untyped int constant |
| `const s = "abc"` | Valid; string constant |
| `const r = 'A'` | Valid; rune constant |
| `const b = true` | Valid; bool constant |
| `const f = 1.5` | Valid; untyped float64 constant |
| `const bufferSize = pageSize * 4` referencing `const pageSize = 4096` declared later | Valid; forward reference to a later constant |
| `const a = b` / `const b = a` | Compile-time error: constant definition cycle |
| `const a = b + 1` / `const b = a + 1` (indirect cycle) | Compile-time error: constant definition cycle |
| `const label = "buf-" + "v2"` | Valid; constant string concatenation |
| `const half = maxRetries / 2` where `const maxRetries uint8 = 5` | Valid; typed uint8 constant arithmetic |
| `const mixed = maxRetries + someInt64Const` (differing explicit types) | Compile-time error: no implicit conversion between differently typed constants |
| `const n = int64(someFloatConst)` where value is representable | Valid; constant explicit conversion |
| `const n = int64(someFloatConst)` where value is not representable | Compile-time error: conversion out of range |
| `const n = int64(2.5)` | Compile-time error: fractional constant is not representable (§6.6–6.7) |
| `const n = int64(2.0)` | Valid; integer 2 |
| `const bad = loadValue()` | Compile-time error: calls are not constant expressions |
| `const bad = someArray[0]` | Compile-time error: indexing is not a constant expression |
| `const bad = someStruct.field` | Compile-time error: field access is not a constant expression |
| `const bad = clone(x)` / `drop(x)` | Compile-time error: not constant expressions |
| `const bad = await task` / `expr?` | Compile-time error: not constant expressions |
| `const bad = go work()` | Compile-time error: not a constant expression |
| `const bad = User{Name: "x"}` | Compile-time error: struct literal is not a constant expression in the MVP |
| `const bad = nil` | Compile-time error: `nil` is never a constant expression |
| `var buf [pageSize; byte]` where `pageSize` is a constant expression | Valid; array size accepts a constant expression |
| `var buf [n; byte]` where `n` is a runtime-only value | Compile-time error: array size must be a constant expression |
| `var buf [-1; byte]` (constant but negative) | Compile-time error: array size must be non-negative |
| Constant integer overflow inside an expression (`const x = maxInt64 + 1`) | Compile-time error, per §6.6 |
| Constant division/remainder by zero | Compile-time error, per §6.6 |
| Constant shift count negative or exceeding operand width | Compile-time error, per §6.6 |
| Untyped constant later assigned to a narrow representable type | Evaluate exact result, then range-check per §6.5 |
| Untyped constant later assigned to a type where the exact result is not representable | Compile-time error, no truncation |

Keep zero-value rules, string storage/encoding, and package-level variable
initialization order (still open) separate from this suite.

## Untyped constant kinds and operators (§6.7)

These follow Go's constant model; Go spec examples are reused where they apply.

| Scenario | Expected result |
| --- | --- |
| `const a = 2 + 3.0` | Untyped float 5.0 (mixed kinds give float) |
| `const b = 15 / 4` | Untyped integer 3 (truncated division) |
| `const c = 15 / 4.0` | Untyped float 3.75 |
| `const d = -7 / 2`, `const e = -7 % 3` | -3 and -1; truncation toward zero, remainder has dividend's sign |
| `const f = 7.5 % 2` or `const g = 7 % 2.0` | Reject: `%` needs untyped integers |
| `const h = 1 / 0`, `const i = 1.0 / 0.0`, `const j = 5 % 0` | Reject: constant division by zero |
| `const k = ^1` | Untyped integer -2 |
| `const l = ^-1` | Untyped integer 0 |
| `const m = 6 & 3`, `const n = 6 \| 3`, `const o = 6 ^ 3` | 2, 7, 5 |
| `const p = -4 \| 1`, `const q = -4 & 7` | -3 and 4 (infinite-precision two's complement) |
| `const r = 1.5 & 1` or `const s = ^1.0` | Reject: bitwise operators need untyped integers |
| `uint8(^1)` | Reject: -2 is not representable as uint8 |
| `^uint8(1)`, `^int8(1)` | Typed 254 and -2 |
| `const t = 1 << 3.0`, `const u = 1.0 << 3` | Untyped integer 8 |
| `const v = 1 << 3.5`, `const w = 1.5 << 1` | Reject: operands not representable as integers |
| `const huge = 1 << 100; const four int8 = huge >> 98` | Valid; 4 of type int8 |
| `const x = 1 << 254` then `const y = x >> 253` | Valid; 2, within the 256-bit minimum precision |
| `const half float64 = 3 / 2` | 1.0: integer division, then typed |
| `const exact float64 = 3 / 2.0` | 1.5 |
| `let whole uint8 = 42.0`, `let big uint64 = 1e10` | Valid integer values 42 and 10000000000 |
| `let bad int = 1.1` | Reject: not an integer value |
| `let precise float32 = 2.718281828459045` | Valid; nearest float32 (round to nearest, ties to even) |
| `let f float32 = 0.1` | Valid; nearest float32 |
| `let tiny float64 = -1e-1000` | Valid; positive zero |
| `let over float64 = 1e1000`, `let over32 float32 = 1e39` | Reject: overflows after rounding |
| `let r rune = 65` | Reject: untyped constants cannot take type `rune` |
| `uint8(-1)`, `int64(3.14)`, `int64(huge)` | Reject: not representable |
| A float constant needing more than the implementation's precision | Rounded to nearest, not rejected |
| An integer constant the implementation cannot represent exactly | Compile-time error |
