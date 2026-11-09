# Seeded random numbers

`std/math/rand` makes repeatable streams of random-looking numbers. You choose
a starting number, the seed, and the same seed always gives the same numbers,
on every computer and in every later Zore version. That makes it good for:

- tests whose "random" data should be the same on every run,
- simulations and games you want to replay,
- shuffling a list or picking a sample.

> **Not for secrets.** Anyone who learns or guesses the seed can predict every
> number. For passwords, login tokens, keys or any security decision, use
> `std/crypto/rand`, which asks the operating system for unpredictable bytes.

```ore
package main

import "std/math/rand"

func main() {
    var r = rand.New(42)
    println(r.IntN(6) + 1)
    println(r.IntBetween(10, 20))
    println(r.Float64())
    var cards = Array<string>{"ace", "king", "queen", "jack"}
    rand.Shuffle(r, cards[:])
    println(cards[0])
    let winners = r.Sample(1000, 3)
    println(winners.len())
}
```

## What you can ask for

| Call | Gives |
| --- | --- |
| `r.IntN(n)` | A whole number from `0` up to, but not including, `n` |
| `r.IntBetween(low, high)` | A whole number from `low` to `high`, both included, so `IntBetween(1, 6)` is a dice roll |
| `r.Uint64N(n)`, `r.Uint64()` | The same for `uint64`, and all 64 bits at once |
| `r.Float64()` | A decimal from `0.0` up to, but not including, `1.0` |
| `r.Bool()` | `true` or `false` |
| `r.Perm(n)` | The numbers `0` to `n - 1` in random order |
| `r.Sample(n, k)` | `k` different numbers from `0` to `n - 1`; fast even when `n` is huge |
| `rand.Shuffle(r, items)` | Puts a slice in random order, in place |

Every choice is equally likely: `IntN(3)` never favours `0` the way
`Uint64() % 3` would. Asking for something impossible, such as `IntN(0)` or
`IntBetween(5, 1)`, is a mistake in the program, so it panics with a message
naming the call.

## Keeping it repeatable

- A generator lives in a `var`, because every draw moves it forward.
- There is no shared, hidden generator: each one comes from `rand.New`, so
  nothing else in the program can change your sequence.
- Copying a generator makes a twin that produces the same numbers from that
  point. To get different streams, use different seeds, such as
  `rand.New(seed + uint64(worker))` for each worker task.
- To share one generator between tasks, lend it `mut` inside a task scope, one
  task at a time, like any other `mut` value.

The exact method (SplitMix64 to spread out the seed, then xoshiro256\*\*) is
fixed by the specification, so recorded seeds keep working.

## A different run each time

When the run should not repeat, take the seed from `std/crypto/rand`. Both
packages are called `rand`, so give one of them another name in the import:

```ore
import "std/crypto/rand"
import "std/encoding/binary"
import seeded "std/math/rand"

func shuffled(items mut []string) error {
    let bytes = rand.Bytes(8)?
    let seed = binary.LittleEndian.Uint64(bytes[:])?
    var generator = seeded.New(seed)
    seeded.Shuffle(generator, items)
    return nil
}
```

Print or log the seed if you may want to replay that run later.
