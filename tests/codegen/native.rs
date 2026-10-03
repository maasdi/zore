//! Native build-and-run tests (decision record 0001). They require clang with
//! LLVM 15 or newer (or `ZORE_CC`) and rustc 1.98+ (or `ZORE_RUSTC`).
//! A missing toolchain fails loudly rather
//! than skipping, so absent coverage is visible.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use zore::build::{BuildError, TempDir, build, emit_llvm};
use zore::source::SourceMap;

/// Build `source` and run it, returning the process output.
fn run(source: &str) -> Output {
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("program");
    build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    Command::new(&executable)
        .output()
        .expect("run built program")
}

fn build_file(source: &str, output: &Path) -> Result<(), BuildError> {
    let mut sources = SourceMap::new();
    let id = sources.add("test.ore", source.into()).unwrap();
    build(sources.file(id).unwrap(), output)
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn main_body(stmts: &str) -> String {
    format!("package main\n\nfunc main() {{\n{stmts}\n}}\n")
}

/// Expect a successful run printing exactly `expected`.
fn prints(source: &str, expected: &str) {
    let output = run(source);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), expected);
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn error_values_preserve_nil_and_message_equality() {
    let source = "package main
func make(message string) error { return error(message) }
func ok() error { return nil }
func pair() (int, error) { return 7, error(\"failed\") }
func main() {
    println(error(\"x\") == error(\"x\"))
    println(error(\"\") != nil)
    println(nil == ok())
    println(make(\"x\") == error(\"x\"))
    println(make(\"y\") != error(\"x\"))
    let value, _ = pair()
    println(value)
    _ = ok()
}";
    prints(source, "true\ntrue\ntrue\ntrue\ntrue\n7\n");
}

#[test]
fn named_error_can_be_checked_and_replaced_after_use() {
    let source = "package main
func main() {
    var err = error(\"first\")
    if err != nil { println(\"handled first\") }
    err = error(\"second\")
    _ = err
}";
    prints(source, "handled first\n");
}

#[test]
fn propagation_forwards_results_and_zero_fills_on_error() {
    let source = "package main
func source(fail bool) (int, string, error) {
    if fail { return 9, \"discarded\", error(\"failed\") }
    return 7, \"ok\", nil
}
func forward(fail bool) (int, string, error) { return source(fail)? }
func main() {
    let good, text, goodErr = forward(false)
    println(good)
    println(text)
    println(goodErr == nil)
    let bad, empty, badErr = forward(true)
    println(bad)
    println(empty == \"\")
    println(badErr == error(\"failed\"))
}";
    prints(source, "7\nok\ntrue\n0\ntrue\ntrue\n");
}

#[test]
fn propagation_skips_later_arguments_and_drops_owned_values() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func source(fail bool) (int, error) {
    if fail { return 0, error(\"failed\") }
    return 7, nil
}
func later() int { println(99); return 2 }
func sum(a int, b int) int { return a + b }
func work(fail bool) (int, error) {
    let first = Guard{id: 1}
    let second = Guard{id: 2}
    let value = sum(source(fail)?, later())
    println(value)
    return value, nil
}
func main() {
    let failed, failure = work(true)
    println(failed)
    println(failure == error(\"failed\"))
    let good, success = work(false)
    println(good)
    println(success == nil)
}";
    prints(source, "2\n1\n0\ntrue\n99\n9\n2\n1\n9\ntrue\n");
}

#[test]
fn propagation_cleans_failed_move_results_and_returns_zero_move_value() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func source(fail bool) (Guard, error) {
    if fail { return Guard{id: 3}, error(\"failed\") }
    return Guard{id: 4}, nil
}
func forward(fail bool) (Guard, error) {
    let outer = Guard{id: 1}
    return (source(fail)?)
}
func main() {
    let failed, failure = forward(true)
    println(failed.id)
    println(failure == error(\"failed\"))
    drop(failed)
    let good, success = forward(false)
    println(good.id)
    println(success == nil)
    drop(good)
}";
    prints(source, "3\n1\n0\ntrue\n0\n1\n4\ntrue\n4\n");
}

#[test]
fn propagation_of_error_only_call_returns_nil_on_success() {
    let source = "package main
func source(fail bool) error {
    if fail { return error(\"failed\") }
    return nil
}
func forward(fail bool) error { return source(fail)? }
func constructed() error { return error(\"constructed\")? }
func main() {
    println(forward(false) == nil)
    println(forward(true) == error(\"failed\"))
    println(constructed() == error(\"constructed\"))
}";
    prints(source, "true\ntrue\ntrue\n");
}

/// Expect a panic in the initial task.
fn panics(source: &str, message: &str, stdout_before: &str) {
    let output = run(source);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{source}\n{}",
        stderr(&output)
    );
    assert_eq!(stdout(&output), stdout_before, "{source}");
    let err = stderr(&output);
    assert!(err.starts_with("panic in the main task: "), "{err}");
    assert!(
        err.contains(message),
        "{source}\nexpected {message:?} in {err:?}"
    );
}

#[test]
fn move_values_drop_on_scope_return_and_transfer() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func consume(g own Guard) { println(100) }
func early() {
    let first = Guard{id: 1}
    {
        let second = Guard{id: 2}
        let third = Guard{id: 3}
    }
    return
}
func main() {
    early()
    let transferred = Guard{id: 4}
    consume(transferred)
    drop(Guard{id: 5})
}";
    prints(source, "3\n2\n1\n100\n4\n5\n");
}

#[test]
fn move_values_drop_on_branches_and_loop_exits() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func main() {
    if true { let a = Guard{id: 1} } else { let b = Guard{id: 2} }
    var count = 0
    for {
        let guard = Guard{id: 3}
        count += 1
        if count == 1 { continue }
        break
    }
}";
    prints(source, "1\n3\n3\n");
}

#[test]
fn panic_unwinds_custom_drops_in_reverse_order() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func inner() {
    let first = Guard{id: 1}
    let second = Guard{id: 2}
    var zero = 0
    println(5 / zero)
}
func main() {
    let outer = Guard{id: 3}
    inner()
}";
    panics(source, "division by zero", "2\n1\n3\n");
}

#[test]
fn panic_during_unwind_aborts_before_other_drops() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() {
    if g.id == 2 {
        var zero = 0
        println(1 / zero)
    } else {
        println(g.id)
    }
}
func main() {
    let first = Guard{id: 1}
    let second = Guard{id: 2}
    var zero = 0
    println(1 / zero)
}";
    let output = run(source);
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn custom_drop_precedes_fields_and_replacement_drops_old_value() {
    let source = "package main
type Inner struct { id int }
func (i mut Inner) drop() { println(i.id) }
type Outer struct {
    first Inner
    second Inner
}
func (o mut Outer) drop() { println(100) }
func main() {
    var outer = Outer{first: Inner{id: 1}, second: Inner{id: 2}}
    outer.second = Inner{id: 3}
}";
    prints(source, "2\n100\n3\n1\n");
}

#[test]
fn partial_move_through_a_call_drops_only_the_remaining_field() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
type Wrapper struct { a Guard; b Guard }
func consume(g own Guard) { println(100) }
func main() {
    var w = Wrapper{a: Guard{id: 1}, b: Guard{id: 2}}
    consume(w.a)
    drop(w.b)
}";
    prints(source, "100\n1\n2\n");
}

#[test]
fn partial_move_then_reinitialize_drops_each_value_once() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
type Wrapper struct { a Guard }
func main() {
    var w = Wrapper{a: Guard{id: 1}}
    let taken = w.a
    drop(taken)
    w.a = Guard{id: 2}
}";
    prints(source, "1\n2\n");
}

#[test]
fn partial_move_without_reinitialization_skips_cleanup_for_that_field() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
type Wrapper struct { a Guard; b Guard }
func main() {
    var w = Wrapper{a: Guard{id: 1}, b: Guard{id: 2}}
    drop(w.a)
}";
    prints(source, "1\n2\n");
}

#[test]
fn arrays_construct_index_and_print() {
    let source = main_body(
        "var xs = [int; 3]{1, 2, 3}
        println(xs[0])
        println(xs[1])
        println(xs[2])
        xs[1] = 9
        println(xs[1])",
    );
    prints(&source, "1\n2\n3\n9\n");
}

#[test]
fn array_index_out_of_range_panics() {
    panics(
        &main_body("let xs = [int; 3]{1, 2, 3}\nvar i = -1\nprintln(xs[i])"),
        "index out of range",
        "",
    );
    panics(
        &main_body("let xs = [int; 3]{1, 2, 3}\nvar i = 3\nprintln(xs[i])"),
        "index out of range",
        "",
    );
    // A huge unsigned index must not wrap around to a valid one when
    // widened to int64 and compared (§12.6).
    panics(
        &main_body(
            "let xs = [int; 3]{1, 2, 3}\nvar i uint64 = 18446744073709551615\nprintln(xs[i])",
        ),
        "index out of range",
        "",
    );
}

#[test]
fn array_of_custom_drop_elements_drops_in_reverse_order_at_scope_exit() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func main() {
    var xs = [Guard; 3]{Guard{id: 1}, Guard{id: 2}, Guard{id: 3}}
}";
    prints(source, "3\n2\n1\n");
}

#[test]
fn array_index_assignment_drops_the_old_element_first() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func main() {
    var xs = [Guard; 2]{Guard{id: 1}, Guard{id: 2}}
    xs[0] = Guard{id: 3}
}";
    prints(source, "1\n2\n3\n");
}

#[test]
fn struct_containing_array_field_drops_elements_then_struct() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
type Wrapper struct { items [Guard; 2] }
func (w mut Wrapper) drop() { println(100) }
func main() {
    var w = Wrapper{items: [Guard; 2]{Guard{id: 1}, Guard{id: 2}}}
}";
    prints(source, "100\n2\n1\n");
}

#[test]
fn replacing_an_array_element_through_a_custom_drop_ancestor_aborts_on_panic() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() {
    if g.id == 99 {
        var zero = 0
        println(1 / zero)
    } else {
        println(g.id)
    }
}
type Container struct { items [Guard; 1] }
func (c mut Container) drop() { println(100) }
func main() {
    var c = Container{items: [Guard; 1]{Guard{id: 99}}}
    c.items[0] = Guard{id: 1}
}";
    let output = run(source);
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn semantic_target_prints_john() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/semantic-target/main.ore"
    ))
    .unwrap();
    prints(&source, "John\n");
}

#[test]
fn hello_prints() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/hello/main.ore"
    ))
    .unwrap();
    prints(&source, "Hello, Zore!\n");
}

#[test]
fn rust_runtime_preserves_empty_nul_and_long_strings() {
    let long = "x".repeat(8192);
    prints(
        &main_body(&format!(
            "var empty string = \"\"\nprintln(empty)\nprintln(empty == \"\")\n\
             println(\"a\\u0000b\")\nprintln(\"{long}\")"
        )),
        &format!("\ntrue\na\0b\n{long}\n"),
    );
}

#[test]
fn methods_call_with_receiver_first() {
    let source = "package main

type Counter struct {
    Label string
    Step int
}

func (c Counter) next(from int) int {
    return from + c.Step
}

func (c own Counter) describe() string {
    return c.Label
}

func main() {
    let counter = Counter{Label: \"steps\", Step: 5}
    println(counter.describe())
    println(counter.next(counter.next(1)))
}
";
    prints(source, "steps\n11\n");
}

#[test]
fn mut_parameters_and_receivers_mutate_the_callers_place() {
    let source = "package main

type Counter struct {
    Label string
    N int
}

type Pair struct {
    Left Counter
    Right Counter
}

func bump(c mut Counter, by int) {
    c.N += by
}

func set(n mut int, value int) {
    n = value
}

func twice(c mut Counter) {
    bump(c, 1)
    bump(c, 1)
}

func (c mut Counter) reset() {
    c.N = 0
}

func (p mut Pair) swapLabels() {
    p.Left.Label = \"right\"
    p.Right.Label = \"left\"
}

func total(p Pair) int {
    return p.Left.N + p.Right.N
}

func main() {
    var c = Counter{Label: \"c\", N: 1}
    bump(c, 4)
    println(c.N)
    twice(c)
    println(c.N)
    c.reset()
    println(c.N)
    var n = 7
    set(n, 42)
    println(n)
    var pair = Pair{Left: Counter{Label: \"l\", N: 1}, Right: Counter{Label: \"r\", N: 2}}
    bump(pair.Right, 10)
    twice(pair.Left)
    pair.swapLabels()
    pair.Left.reset()
    println(pair.Left.Label)
    println(pair.Right.Label)
    println(total(pair))
    println(pair.Left.N)
}
";
    prints(source, "5\n7\n0\n42\nright\nleft\n12\n0\n");
}

#[test]
fn rust_runtime_links_to_an_output_path_with_spaces() {
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("output with spaces");
    std::fs::create_dir(&folder).unwrap();
    let executable = folder.join("hello world");
    build_file(&main_body("println(42)"), &executable).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "42\n");
}

#[test]
fn println_formats_each_printable_type() {
    prints(
        &main_body(
            "println(\"text\")
            println(\"\")
            println(\"café 用户\")
            println(\"a\\tb\")
            println(42)
            println(-7)
            println(-9223372036854775807 - 1)
            println(uint64(18446744073709551615))
            println(int8(-128))
            println(uint8(255))
            println(0x1F)
            println(true)
            println(false)
            println('A')
            println('é')
            println('😀')",
        ),
        "text\n\ncafé 用户\na\tb\n42\n-7\n-9223372036854775808\n18446744073709551615\n-128\n255\n31\ntrue\nfalse\nA\né\n😀\n",
    );
}

#[test]
fn functions_structs_and_control_flow() {
    let source = "package main

type Point struct {
    X int
    Y int
}

type Line struct {
    From Point
    To Point
    Label string
}

func pair() (int, string) {
    return 42, \"answer\"
}

func forward() (int, string) {
    return pair()
}

func fib(n int) int {
    if n < 2 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

func length(l Line) int {
    return l.To.X - l.From.X + l.To.Y - l.From.Y
}

func classify(n int) string {
    if n < 0 {
        return \"negative\"
    } else if n == 0 {
        return \"zero\"
    } else {
        return \"positive\"
    }
}

func main() {
    println(fib(20))
    let n, s = forward()
    println(n)
    println(s)
    var total = 0
    for var i = 0; i < 10; i += 1 {
        if i == 3 { continue }
        if i == 8 { break }
        total += i
    }
    println(total)
    var line = Line{Label: \"diag\", To: Point{X: 4, Y: 6}, From: Point{X: 1, Y: 2}}
    line.To.X = 10
    println(length(line))
    println(line.Label)
    var left = 1
    var right = 2
    left, right = right, left
    println(left * 10 + right)
    var c = 0
    for c < 5 { c += 2 }
    println(c)
    for {
        c -= 1
        if c < 3 { break }
    }
    println(c)
    println(classify(-3))
    println(classify(0))
    println(classify(9))
}
";
    prints(
        source,
        "6765\n42\nanswer\n25\n13\ndiag\n21\n6\n2\nnegative\nzero\npositive\n",
    );
}

#[test]
fn integer_float_and_string_operations() {
    prints(
        &main_body(
            "var bits uint8 = 128
            bits <<= 1
            println(bits)
            var neg = int8(-2)
            println(neg >> 1)
            var u uint8 = 128
            println(u >> 7)
            var one int8 = 64
            println(one << 1)
            println(^uint8(5))
            var a = 17
            var b = 5
            println(a % b)
            println(-a / b)
            println(-a % b)
            println(a & 12 | 2 ^ 1)
            var f = 2.75
            f = f * 2
            println(int64(f))
            println(int64(-f))
            println(int32(uint8(200)))
            let x float32 = 1.5
            println(int(float64(x) * 4))
            var g float32 = 255.9
            println(uint8(g))
            println(\"apple\" < \"banana\")
            var s = \"same\"
            println(s == \"same\")
            println(s != \"same\")
            println(\"\" < s)
            println(\"ab\" < \"a\")
            println('a' < 'b')
            println(1.5 < f)
            println(!true)",
        ),
        "0\n-1\n1\n-128\n250\n2\n-3\n-2\n3\n5\n-5\n200\n6\n255\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue\nfalse\n",
    );
}

#[test]
fn evaluation_order_is_left_to_right() {
    let source = "package main

type Pair struct {
    A int
    B int
}

func tag(label string, value int) int {
    println(label)
    return value
}

func noisy(label string, value bool) bool {
    println(label)
    return value
}

func add(a int, b int) int {
    return a + b
}

func main() {
    let p = Pair{B: tag(\"b\", 2), A: tag(\"a\", 1)}
    println(p.A * 10 + p.B)
    println(add(tag(\"first\", 1), tag(\"second\", 2)))
    println(tag(\"left\", 3) * tag(\"right\", 4))
    println(noisy(\"and-left\", false) && noisy(\"and-right\", true))
    println(noisy(\"or-left\", true) || noisy(\"or-right\", true))
    println(noisy(\"x\", true) && noisy(\"y\", false))
}
";
    prints(
        source,
        "b\na\n12\nfirst\nsecond\n3\nleft\nright\n12\nand-left\nfalse\nor-left\ntrue\nx\ny\nfalse\n",
    );
}

#[test]
fn checked_operations_panic_with_locations() {
    panics(
        &main_body("println(1)\nvar x int8 = 127\nx += 1"),
        "integer overflow at test.ore:6:1",
        "1\n",
    );
    panics(
        &main_body("var x uint8 = 0\nx -= 1"),
        "integer overflow",
        "",
    );
    panics(
        &main_body("var x = 9223372036854775807\nprintln(x * 2)"),
        "integer overflow",
        "",
    );
    panics(
        &main_body("var x int8 = -128\nprintln(-x)"),
        "integer overflow",
        "",
    );
    panics(
        &main_body("var d = 0\nprintln(10 / d)"),
        "division by zero at test.ore:5:9",
        "",
    );
    panics(
        &main_body("var d = 0\nprintln(10 % d)"),
        "division by zero",
        "",
    );
    panics(
        &main_body("var m int8 = -128\nvar n int8 = -1\nprintln(m / n)"),
        "integer overflow",
        "",
    );
    panics(
        &main_body("var s = 64\nprintln(1 << s)"),
        "shift count out of range",
        "",
    );
    panics(
        &main_body("var s int8 = -1\nprintln(1 << s)"),
        "shift count out of range",
        "",
    );
    panics(
        &main_body("var b uint8 = 1\nvar s = 8\nb <<= s"),
        "shift count out of range",
        "",
    );
    panics(
        &main_body("var big = 300\nprintln(uint8(big))"),
        "integer conversion out of range",
        "",
    );
    panics(
        &main_body("var neg = -1\nprintln(uint64(neg))"),
        "integer conversion out of range",
        "",
    );
    panics(
        &main_body("var f = 1e300\nprintln(int64(f))"),
        "float to integer conversion out of range",
        "",
    );
    panics(
        &main_body("var f = -1.0\nprintln(uint8(f))"),
        "float to integer conversion out of range",
        "",
    );
    panics(
        &main_body("var f = 1e300\nlet g = float32(f)"),
        "float conversion out of range",
        "",
    );
}

#[test]
fn closed_standard_output_panics() {
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("spam");
    build_file(&main_body("for {\nprintln(\"spam\")\n}"), &executable).unwrap();
    let mut child = Command::new(&executable)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Closing the read end makes the program's next write fail.
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("failed to write to standard output"));
}

#[test]
fn unsupported_backend_features_are_diagnosed() {
    for (stmts, message) in [
        (
            "var s = \"a\"\nprintln(s + \"b\")",
            "runtime string concatenation is not supported",
        ),
        (
            "println(1.5)",
            "printing floating-point values is not supported",
        ),
        (
            "let data = [int; 2]{1, 2}\nlet view = data[:]\nprintln(view[0])",
            "borrowed slices are not supported by the native backend yet",
        ),
    ] {
        let mut sources = SourceMap::new();
        let id = sources.add("test.ore", main_body(stmts)).unwrap();
        match emit_llvm(sources.file(id).unwrap()) {
            Err(BuildError::Diagnostics(diagnostics)) => {
                assert!(
                    diagnostics.iter().any(|d| d.message().contains(message)),
                    "{diagnostics:?}"
                );
            }
            other => panic!("expected diagnostics, got {other:?}"),
        }
    }
    // Constant concatenation is folded by the checker and works.
    prints(&main_body("println(\"con\" + \"cat\")"), "concat\n");
}

#[test]
fn shared_array_parameters_read_the_callers_storage() {
    prints(
        "package main
type Grid struct { cells [int; 3]\n label string }
func (g Grid) total() int { return g.cells[0] + g.cells[1] + g.cells[2] }
func sum(xs [int; 3]) int { return xs[0] + xs[1] + xs[2] }
func label(g Grid) string { return g.label }
func bump(xs mut [int; 3]) { xs[1] += 10 }
func main() {
    var xs = [int; 3]{1, 2, 3}
    println(sum(xs))
    bump(xs)
    println(sum(xs))
    println(sum([int; 3]{4, 5, 6}))
    let g = Grid{cells: xs, label: \"grid\"}
    println(g.total())
    println(label(g))
    println(xs[1])
}
",
        "6\n16\n15\n16\ngrid\n12\n",
    );
}

#[test]
fn only_valid_executables_are_built() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("x");
    match build_file("package tools\nfunc helper() {}\n", &out) {
        Err(BuildError::NotExecutable(message)) => assert!(message.contains("not executable")),
        other => panic!("{other:?}"),
    }
    match build_file(&main_body("let x uint8 = 300"), &out) {
        Err(BuildError::Diagnostics(d)) => assert!(d[0].message().contains("does not fit")),
        other => panic!("{other:?}"),
    }
    assert!(!out.exists());
}

fn zore() -> Command {
    Command::new(env!("CARGO_BIN_EXE_zore"))
}

#[test]
fn cli_build_and_run() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("greet.ore");
    std::fs::write(&source, main_body("println(\"hi\")")).unwrap();
    let output = zore()
        .arg("build")
        .arg(&source)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    let built = Command::new(dir.path().join("greet")).output().unwrap();
    assert_eq!(stdout(&built), "hi\n");

    let output = zore().arg("run").arg(&source).output().unwrap();
    assert_eq!(
        (output.status.code(), stdout(&output)),
        (Some(0), "hi\n".into())
    );

    let panicking = dir.path().join("boom.ore");
    std::fs::write(
        &panicking,
        main_body("println(\"before\")\nvar d = 0\nprintln(1 / d)"),
    )
    .unwrap();
    let output = zore().arg("run").arg(&panicking).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "before\n");
    assert!(stderr(&output).contains("division by zero"));

    let invalid = dir.path().join("bad.ore");
    std::fs::write(&invalid, main_body("println(missing)")).unwrap();
    for command in ["build", "run"] {
        let output = zore()
            .arg(command)
            .arg(&invalid)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let err = stderr(&output);
        assert!(err.contains("cannot find `missing`"), "{err}");
        assert!(err.contains("zore: build failed with 1 error"), "{err}");
    }
    assert!(!dir.path().join("bad").exists());
}

#[test]
fn missing_toolchain_is_reported() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("main.ore");
    let mut file = std::fs::File::create(&source).unwrap();
    file.write_all(main_body("println(1)").as_bytes()).unwrap();
    let output = zore()
        .arg("build")
        .arg(&source)
        .current_dir(dir.path())
        .env("ZORE_CC", "/nonexistent/zore-test-cc")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("requires clang with LLVM 15 or newer"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn missing_rust_toolchain_is_reported_but_check_stays_independent() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("main.ore");
    std::fs::write(&source, main_body("println(1)")).unwrap();
    let output = zore()
        .arg("build")
        .arg(&source)
        .current_dir(dir.path())
        .env("ZORE_RUSTC", dir.path().join("missing-rustc"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("requires rustc 1.98 or newer"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("main").exists());

    let output = zore()
        .arg("check")
        .arg(&source)
        .env("ZORE_RUSTC", dir.path().join("missing-rustc"))
        .env("ZORE_CC", dir.path().join("missing-clang"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

#[test]
fn emitted_ir_is_deterministic_and_names_the_entry() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/semantic-target/main.ore"
    ))
    .unwrap();
    let emit = || {
        let mut sources = SourceMap::new();
        let id = sources.add("main.ore", source.clone()).unwrap();
        emit_llvm(sources.file(id).unwrap()).unwrap()
    };
    let ir = emit();
    assert_eq!(ir, emit());
    assert!(
        ir.contains("%\"main.User\" = type { { ptr, i64 } }"),
        "{ir}"
    );
    assert!(ir.contains("define void @\"main.greet\""), "{ir}");
    assert!(ir.contains("define void @zore_entry()"), "{ir}");
    assert!(ir.contains("c\"John\""), "{ir}");
}
