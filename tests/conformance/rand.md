# Seeded random numbers

The locked contract is specification §37.24. Native cases are in
`tests/codegen/rand.rs`; checking cases are in `tests/packages/packages.rs`.
The expected sequences were computed independently in Python from the
algorithm the specification fixes.

| Case | Required result |
| --- | --- |
| `New(0)` and `New(2^64 - 1)`, first `Uint64` values | The published sequence |
| `IntN`, `IntBetween`, `Uint64N`, `Float64`, `Bool` from seed 42 in a fixed order | The published values |
| `IntBetween` over the whole `int` range and its last two values | The published values; no overflow |
| `Perm(10)`, `Shuffle` of six strings, `Sample(100, 5)`, `Sample(10^12, 4)` | The published orders and values |
| 30,000 draws of `IntN(3)` | Every value in range; each bucket above 9,500 |
| `IntBetween(-2, 2)`, `Float64`, `IntN(1)`, `Uint64N(1)`, `IntBetween(5, 5)`, `Uint64N(2^64 - 1)` | In range; the single possible value |
| `Perm(0)`, `Sample(5, 0)`, `Sample(4, 4)`, shuffling one and zero items | Empty, empty, all four values, unchanged |
| A copied generator | Repeats the original's numbers; a different seed differs |
| `IntN(0)`, `IntN(-1)`, `Uint64N(0)`, `IntBetween(5, 1)`, `Perm(-1)`, `Sample(3, 4)`, `Sample(3, -1)` | Panics naming the call |
| `std/crypto/rand` and an import-named `std/math/rand` in one file | Both usable; a system-seeded shuffle keeps every item |
| A generator in a `let`, reading or building its private fields, a negative seed, shuffling non-copyable values | Rejected by the checker |
