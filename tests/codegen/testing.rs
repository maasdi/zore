use super::*;

#[test]
fn fmt_formats_type_parameter_values_like_their_concrete_types() {
    let source = r#"package main
import "std/fmt"

type Level int

type Name struct {
    text string
}

func (n Name) String() string {
    return "<" + n.text + ">"
}

func pair<T comparable>(got T, want T) string {
    return fmt.Sprintf("got %v, want %4v|", got, want)
}

func plain<T any>(value T) string {
    return fmt.Sprint(value) + fmt.Sprint(value, value) + fmt.Sprintln(value, value)
}

func nested<T comparable>(value T) string {
    return pair(value, value)
}

func failure<T any>(value T) error {
    return fmt.Errorf("value %v", value)
}

func main() {
    println(pair(1, 2))
    println(pair(int8(-3), int8(4)))
    println(pair(uint64(18446744073709551615), uint64(0)))
    println(pair("a b", ""))
    println(pair(true, false))
    println(pair('x', 'y'))
    println(pair(Level(7), Level(8)))
    println(nested(5))
    fmt.Print(plain(1.5))
    fmt.Print(plain(error("boom")))
    fmt.Print(plain(Name{text: "n"}))
    fmt.Print(plain("s"))
    println(failure(42) == error("value 42"))
}
"#;
    prints(
        source,
        "got 1, want    2|\ngot -3, want    4|\ngot 18446744073709551615, want    0|\ngot a b, want     |\ngot true, want false|\ngot x, want    y|\ngot 7, want    8|\ngot 5, want    5|\n1.51.5 1.51.5 1.5\nboomboom boomboom boom\n<n><n> <n><n> <n>\nss ss s\ntrue\n",
    );
}

#[test]
fn checks_report_what_was_found_and_what_was_wanted() {
    let source = r#"package main
import "std/errors"
import "std/testing"

func show(err error) {
    if err == nil {
        println("ok")
    } else {
        println(errors.Message(err))
    }
}

type Code int

func main() {
    show(testing.Equal(2 + 2, 4))
    show(testing.Equal(2 + 2, 5))
    show(testing.Equal("left", "right"))
    show(testing.Equal(Code(3), Code(4)))
    show(testing.NotEqual(1, 2))
    show(testing.NotEqual("same", "same"))
    show(testing.EqualSlices(Array<int>{1, 2}[:], Array<int>{1, 2}[:]))
    show(testing.EqualSlices(Array<int>{1, 2}[:], Array<int>{1, 2, 3}[:]))
    show(testing.EqualSlices(Array<string>{"a", "b", "c"}[:], Array<string>{"a", "x", "y"}[:]))
    show(testing.EqualSlices(Array<int>{}[:], Array<int>{}[:]))
    show(testing.Near(0.1 + 0.2, 0.3, 0.000001))
    show(testing.Near(1.0, 1.5, 0.25))
    show(testing.Near(-2.0, -2.5, 0.5))
    show(testing.True(1 < 2, "never shown"))
    show(testing.True(false, "flag should be set"))
    show(testing.NoError(nil))
    show(testing.NoError(error("disk full")))
    let missing = error("missing")
    show(testing.ErrorIs(errors.Wrap(missing, "load"), missing))
    show(testing.ErrorIs(nil, missing))
    show(testing.ErrorIs(error("other"), missing))
    show(testing.ErrorContains(error("bad input at line 3"), "line 3"))
    show(testing.ErrorContains(nil, "line"))
    show(testing.ErrorContains(error("bad input"), "timeout"))
}
"#;
    prints(
        source,
        "ok\ngot 4, want 5\ngot left, want right\ngot 3, want 4\nok\ngot same, want any other value\nok\ngot length 2, want 3\nat index 1: got b, want x\nok\nok\ngot 1.0, want 1.5 within 0.25\nok\nok\nflag should be set\nok\nunexpected error: disk full\nok\ngot no error, want \"missing\"\ngot error \"other\", want \"missing\"\nok\ngot no error, want one containing \"line\"\ngot error \"bad input\", want one containing \"timeout\"\n",
    );
}

#[test]
fn a_test_context_records_failures_and_runs_cleanups_newest_first_once() {
    let source = r#"package main
import "std/os"
import "std/testing"

func main() {
    var t = testing.NewT("TestDemo")
    println(t.Name())
    println(t.Failed())
    t.Cleanup(func() { println("cleanup 1") })
    t.Cleanup(func() { println("cleanup 2") })
    let dir, err = t.TempDir()
    println(err == nil)
    let _, statErr = os.Stat(dir)
    println(statErr == nil)
    t.Check(nil)
    println(t.Failed())
    t.Check(error("first"))
    t.Fail("second")
    let failures = t.Failures()
    println(failures.len())
    println(failures[0] + "," + failures[1])
    t.Log("logged now")
    println(t.Finish(error("returned")))
    let _, gone = os.Stat(dir)
    println(gone != nil)
    var clean = testing.NewT("TestClean")
    clean.Cleanup(func() { println("clean cleanup") })
    println(clean.Finish(nil))
    var dropped = testing.NewT("TestDropped")
    dropped.Cleanup(func() { println("dropped cleanup") })
    drop(dropped)
    println("end")
}
"#;
    prints(
        source,
        "TestDemo\nfalse\ntrue\ntrue\nfalse\n2\nfirst,second\nlogged now\ncleanup 2\ncleanup 1\nfirst\nsecond\nreturned\n1\ntrue\nclean cleanup\n0\ndropped cleanup\nend\n",
    );
}

#[test]
fn cleanups_run_while_a_panic_unwinds() {
    let source = r#"package main
import "std/testing"

func main() {
    var t = testing.NewT("TestPanic")
    t.Cleanup(func() { println("first registered") })
    t.Cleanup(func() { println("last registered") })
    println("before")
    panic("test panic")
}
"#;
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "before\nlast registered\nfirst registered\n"
    );
    assert!(
        stderr(&output).contains("test panic"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("leak:"), "{}", stderr(&output));
}

#[test]
fn a_benchmark_measures_until_its_goal_and_reports_its_rate() {
    let source = r#"package main
import "std/testing"
import "std/time"

func main() {
    var b = testing.NewB("BenchmarkCount", 20 * time.Millisecond)
    println(b.Name())
    var total = 0
    for b.Loop() {
        total += 1
    }
    println(b.Iterations() == total && total > 1)
    println(b.Elapsed() >= 20 * time.Millisecond)
    println(b.NanosecondsPerIteration() > 0.0)
    println(b.Loop())
    println(b.Iterations() == total)
    var slow = testing.NewB("BenchmarkSlow", 5 * time.Millisecond)
    var runs = 0
    for slow.Loop() {
        runs += 1
        time.Sleep(10 * time.Millisecond)
    }
    println(runs)
    var never = testing.NewB("BenchmarkNever", time.Millisecond)
    println(never.NanosecondsPerIteration() == 0.0)
    println(never.Finish(nil))
    var failed = testing.NewB("BenchmarkFailed", time.Millisecond)
    println(failed.Finish(error("setup failed")))
}
"#;
    prints(
        source,
        "BenchmarkCount\ntrue\ntrue\ntrue\nfalse\ntrue\n1\ntrue\nbenchmark BenchmarkNever did not run its Loop to the end\n1\nsetup failed\n1\n",
    );
}

#[test]
fn a_benchmark_needs_a_positive_goal() {
    for goal in ["0", "-1"] {
        let source = format!(
            "package main\nimport \"std/testing\"\nimport \"std/time\"\nfunc main() {{\n    println(\"start\")\n    var b = testing.NewB(\"BenchmarkBad\", time.Duration({goal}))\n    _ = b.Loop()\n}}\n"
        );
        panics(&source, "testing.NewB: goal must be positive", "start\n");
    }
}
