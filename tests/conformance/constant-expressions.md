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
