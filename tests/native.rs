//! Native build-and-run tests (decision record 0001). They require clang with
//! LLVM 15 or newer (or `ZORE_CC`); a missing toolchain fails loudly rather
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

/// Expect a panic in the initial task (§3.19, §18.10).
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
fn semantic_target_prints_john() {
    let source = std::fs::read_to_string("examples/semantic-target/main.ore").unwrap();
    prints(&source, "John\n");
}

#[test]
fn hello_prints() {
    let source = std::fs::read_to_string("examples/hello/main.ore").unwrap();
    prints(&source, "Hello, Zore!\n");
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
fn emitted_ir_is_deterministic_and_names_the_entry() {
    let source = std::fs::read_to_string("examples/semantic-target/main.ore").unwrap();
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
