use super::*;

const VECTORS: &str = r#"package main
import "std/math/rand"
import "std/strings"

func joined(values []int) string {
    var parts = Array<string>{}
    for value in values {
        parts.push(text(value))
    }
    return strings.Join(parts[:], " ")
}

func text(value int) string {
    return strings.TrimSpace(sprint(value))
}

func sprint(value int) string {
    if value < 0 {
        return "-" + digits(-value)
    }
    return digits(value)
}

func digits(value int) string {
    if value < 10 {
        return "0123456789"[value:value + 1]
    }
    return digits(value / 10) + digits(value % 10)
}

func main() {
    var zero = rand.New(0)
    println(zero.Uint64())
    println(zero.Uint64())
    println(zero.Uint64())
    var top = rand.New(18446744073709551615)
    println(top.Uint64())
    println(top.Uint64())
    var r = rand.New(42)
    var dice = Array<int>{}
    for var i = 0; i < 12; i += 1 { dice.push(r.IntN(6)) }
    println(joined(dice[:]))
    var between = Array<int>{}
    for var i = 0; i < 12; i += 1 { between.push(r.IntBetween(-3, 3)) }
    println(joined(between[:]))
    println(r.IntBetween(-9223372036854775807 - 1, 9223372036854775807))
    println(r.IntBetween(9223372036854775806, 9223372036854775807))
    println(r.Uint64N(1000000007))
    println(r.Float64())
    println(r.Float64())
    println(r.Float64())
    var flips = Array<string>{}
    for var i = 0; i < 8; i += 1 {
        if r.Bool() { flips.push("true") } else { flips.push("false") }
    }
    println(strings.Join(flips[:], " "))
    var p = rand.New(7)
    let perm = p.Perm(10)
    println(joined(perm[:]))
    var words = Array<string>{"a", "b", "c", "d", "e", "f"}
    var w = rand.New(7)
    rand.Shuffle(w, words[:])
    println(strings.Join(words[:], " "))
    var s = rand.New(9)
    let small = s.Sample(100, 5)
    println(joined(small[:]))
    var huge = rand.New(9)
    let large = huge.Sample(1000000000000, 4)
    println(joined(large[:]))
}
"#;

#[test]
fn fixed_seeds_give_the_published_sequences() {
    prints(
        VECTORS,
        "11091344671253066420\n13793997310169335082\n1900383378846508768\n10328197420357168392\n14156678507024973869\n0 0 5 5 4 0 4 3 4 5 1 1\n-1 2 2 -1 -2 3 0 3 3 -3 -3 0\n5792072832420444912\n9223372036854775806\n239955325\n0.7891219258506235\n0.6350567525507664\n0.23133453518969926\nfalse true true true true true true false\n8 3 9 0 7 2 1 6 5 4\nd f b c e a\n40 83 39 72 9\n50712223840 90217792946 227265174763 549090746597\n",
    );
}

#[test]
fn bounded_values_stay_in_range_and_cover_it_evenly() {
    prints(
        r#"package main
import "std/math/rand"

func main() {
    var r = rand.New(123)
    var counts = Array<int>{0, 0, 0}
    var ok = true
    for var i = 0; i < 30000; i += 1 {
        let value = r.IntN(3)
        ok = ok && value >= 0 && value < 3
        counts[value] += 1
    }
    println(ok)
    println(counts[0] > 9500 && counts[1] > 9500 && counts[2] > 9500)
    var seen = Array<bool>{false, false, false, false, false}
    for var i = 0; i < 1000; i += 1 {
        let value = r.IntBetween(-2, 2)
        ok = ok && value >= -2 && value <= 2
        seen[value + 2] = true
    }
    println(ok && seen[0] && seen[1] && seen[2] && seen[3] && seen[4])
    for var i = 0; i < 1000; i += 1 {
        let f = r.Float64()
        ok = ok && f >= 0.0 && f < 1.0
        ok = ok && r.IntN(1) == 0 && r.Uint64N(1) == 0 && r.IntBetween(5, 5) == 5
        ok = ok && r.Uint64N(18446744073709551615) < 18446744073709551615
    }
    println(ok)
    let empty = r.Perm(0)
    let none = r.Sample(5, 0)
    let all = r.Sample(4, 4)
    var found = Array<bool>{false, false, false, false}
    for value in all { found[value] = true }
    println(empty.len() == 0 && none.len() == 0 && found[0] && found[1] && found[2] && found[3])
    var one = Array<int>{7}
    rand.Shuffle(r, one[:])
    var nothing = Array<int>{}
    rand.Shuffle(r, nothing[:])
    println(one[0] == 7 && nothing.len() == 0)
}
"#,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn a_copied_generator_repeats_the_same_numbers() {
    prints(
        r#"package main
import "std/math/rand"

func roll(r mut rand.Rand) int {
    return r.IntN(100)
}

func main() {
    var first = rand.New(5)
    _ = first.Uint64()
    var second = first
    println(first.Uint64() == second.Uint64())
    println(roll(first) == roll(second))
    var again = rand.New(5)
    var other = rand.New(6)
    println(again.Uint64() != other.Uint64())
}
"#,
        "true\ntrue\ntrue\n",
    );
}

#[test]
fn invalid_bounds_panic_with_the_function_name() {
    for (call, message) in [
        ("r.IntN(0)", "rand.IntN: n must be positive"),
        ("r.IntN(-1)", "rand.IntN: n must be positive"),
        ("r.Uint64N(0)", "rand.Uint64N: n must be positive"),
        (
            "r.IntBetween(5, 1)",
            "rand.IntBetween: low is greater than high",
        ),
        ("r.Perm(-1).len()", "rand.Perm: n must not be negative"),
        ("r.Sample(3, 4).len()", "rand.Sample: need 0 <= k <= n"),
        ("r.Sample(3, -1).len()", "rand.Sample: need 0 <= k <= n"),
    ] {
        let source = format!(
            "package main\nimport \"std/math/rand\"\nfunc main() {{\n    var r = rand.New(1)\n    println(\"start\")\n    println({call})\n}}\n"
        );
        panics(&source, message, "start\n");
    }
}

#[test]
fn both_rand_packages_work_in_one_file_through_an_import_name() {
    prints(
        r#"package main
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

func main() {
    var items = Array<string>{"x", "y", "z"}
    let err = shuffled(items[:])
    var seen = 0
    for item in items {
        if item == "x" || item == "y" || item == "z" { seen += 1 }
    }
    println(err == nil && seen == 3)
}
"#,
        "true\n",
    );
}

#[test]
fn guide_examples_compile_and_run() {
    let guide = include_str!("../../docs/random.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 2);
    let output = run(blocks[0]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output).lines().count(), 5, "{}", stdout(&output));
    prints(
        &format!(
            "package main\n{}\nfunc main() {{\n    var items = Array<string>{{\"a\", \"b\"}}\n    println(shuffled(items[:]) == nil)\n}}\n",
            blocks[1]
        ),
        "true\n",
    );
}
