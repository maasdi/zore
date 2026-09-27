# `println` conformance cases

Authority: spec §37.1, with §3.17–3.18, §6.5, §7.8, and §15.4. These are
pending resolution, typing, code-generation, and runtime cases, not passing
coverage. Outputs show exact stdout bytes, with `\n` meaning one line feed.

| Source / scenario | Expected result |
| --- | --- |
| `println("Hello, Zore!")` | `Hello, Zore!\n` |
| `println("")` | `\n` |
| `println("café 用户")` | UTF-8 contents unchanged, then `\n` |
| `println("a\nb")` | `a\nb\n`; decoded escapes are written as-is |
| `println(42)` | Untyped constant defaults to `int`; `42\n` |
| `println(-7)` and `println(int8(-128))` | `-7\n` and `-128\n` |
| `println(uint64(18446744073709551615))` | `18446744073709551615\n` |
| `println(0x1F)` | `31\n`; decimal output regardless of source base |
| `println(true)` / `println(false)` | `true\n` / `false\n` |
| `println('A')` and `println('😀')` | `A\n` and `😀\n`; the character, not its number |
| `println(user.Name)` with a `string` field | Field contents |
| `println(ratio)` with a `float64` value | Type-checks; native output awaits the float format decision |
| `println()` | Reject: exactly one argument |
| `println("a", "b")` | Reject: exactly one argument |
| `println(user)` with a struct value | Reject: type is not printable |
| `println(err)` with an `error` value | Reject: `error` is not a printable type |
| `println(nil)` | Reject |
| `let p = println`, passing `println` as an argument | Reject: `println` is not a value |
| `_ = println("x")` or `let x = println("x")` | Reject: no result |
| `let println = 1` | Reject predeclared-name shadowing (§3.18) |
| `println("x")` as a statement | Valid call statement (§7.8) |
| Argument is a Move-type local | Not a printable type; reject (no move occurs) |
| `println` inside an `async func` | Valid; does not suspend |
| Several tasks printing concurrently | Each line intact; line order unspecified |
| Standard output closed or failing | Calling task panics |
| Standard output blocks (e.g. a full pipe) while one task prints | Other ready tasks keep running |
