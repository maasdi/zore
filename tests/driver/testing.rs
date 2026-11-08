use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use zore::build::TempDir;

fn project(files: &[(&str, &str)]) -> TempDir {
    let directory = TempDir::new().unwrap();
    for (path, text) in files {
        let path = directory.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    directory
}

fn command(action: &str, target: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zore"));
    command
        .env("ZORE_CHECK_LEAKS", "1")
        .env_remove("ZORE_TEST_DEMO_FAILURES")
        .args([OsStr::new(action), target.as_os_str()]);
    command
}

fn run(target: &Path) -> Output {
    command("test", target).output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn success(output: &Output, passed: usize) {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(
        stdout(output).ends_with(&format!("Tests: {passed} passed, 0 failed\n")),
        "{output:?}"
    );
}

fn example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/application-tests")
}

#[test]
fn sample_runs_both_signatures_and_cleans_up_on_return_error_and_panic() {
    let normal = run(&example());
    success(&normal, 4);
    let text = stdout(&normal);
    for message in [
        "cleanup: normal return",
        "cleanup: error return",
        "cleanup: panic or normal return",
    ] {
        assert_eq!(text.matches(message).count(), 1, "{text}");
    }
    assert!(!text.contains("\n5\n"), "application main ran: {text}");
    let failure = command("test", &example())
        .env("ZORE_TEST_DEMO_FAILURES", "1")
        .output()
        .unwrap();
    assert_eq!(failure.status.code(), Some(1), "{failure:?}");
    let text = stdout(&failure);
    assert!(text.ends_with("Tests: 2 passed, 2 failed\n"), "{text}");
    assert!(
        stderr(&failure).contains("intentional panic"),
        "{failure:?}"
    );
    assert!(!stderr(&failure).contains("leak:"), "{failure:?}");
    for message in [
        "cleanup: normal return",
        "cleanup: error return",
        "cleanup: panic or normal return",
        "intentional returned error",
    ] {
        assert_eq!(text.matches(message).count(), 1, "{text}");
    }
    assert!(
        text.contains("::TestReturnedError (exit status: 1)"),
        "{text}"
    );
    assert!(text.contains("::TestPanic (exit status: 2)"), "{text}");
    assert!(
        text.rfind("PASS ").unwrap() > text.rfind("FAIL ").unwrap(),
        "{text}"
    );
}

#[test]
fn file_selection_checks_the_whole_package_but_runs_only_selected_tests() {
    success(&run(&example().join("main.ore")), 4);
    let selected = run(&example().join("calculator_test.ore"));
    success(&selected, 2);
    assert!(!stdout(&selected).contains("TestReturnedError"));
    let directory = project(&[
        (
            "selected_test.ore",
            "package main\nfunc TestSelected() {}\n",
        ),
        (
            "other_test.ore",
            "package main\nfunc helper() { println(missing) }\n",
        ),
    ]);
    let rejected = run(&directory.path().join("selected_test.ore"));
    assert_eq!(rejected.status.code(), Some(1));
    assert!(rejected.stdout.is_empty());
    assert!(
        stderr(&rejected).contains("other_test.ore:2:"),
        "{rejected:?}"
    );
}

#[test]
fn discovery_uses_private_helpers_and_hygienic_imports_with_fresh_initialization() {
    let directory = project(&[
        (
            "library.ore",
            "package library\nlet initialized = initialize()\nfunc initialize() int { println(\"initialized\"); return 7 }\nfunc os() int { return initialized }\nfunc errors() {}\nfunc runner() {}\nfunc main() { panic(\"application main ran\") }\nfunc TestOrdinaryFile() { panic(\"ordinary file test ran\") }\n",
        ),
        (
            "b_test.ore",
            "package library\nfunc TestZed() { if os() != 7 { panic(\"initialization\") } }\nfunc Testhelper() { panic(\"helper ran\") }\nfunc Test() { panic(\"bare Test ran\") }\ntype Helper struct {}\nfunc (h Helper) TestMethod() { panic(\"method ran\") }\n",
        ),
        (
            "a_test.ore",
            "package library\nasync func double(value int) int { return value * 2 }\nfunc TestAsync() error { let task = go double(3); if task.wait() != 6 { return error(\"task result\") }; return nil }\nfunc TestAlpha() { if os() != 7 { panic(\"private helper\") } }\n",
        ),
    ]);
    let output = run(directory.path());
    success(&output, 3);
    let text = stdout(&output);
    assert_eq!(text.matches("initialized\n").count(), 3, "{text}");
    let names: Vec<_> = text
        .lines()
        .filter(|line| line.starts_with("RUN "))
        .map(|line| line.rsplit("::").next().unwrap())
        .collect();
    assert_eq!(names, ["TestAlpha", "TestAsync", "TestZed"]);
}

#[test]
fn invalid_test_signatures_are_diagnostics_before_native_building() {
    for declaration in [
        "async func TestInvalid() {}",
        "func TestInvalid<T any>() {}",
        "func TestInvalid(value int) {}",
        "func TestInvalid() int { return 1 }",
        "func TestInvalid() (int, error) { return 1, nil }",
    ] {
        let directory = project(&[(
            "invalid_test.ore",
            &format!("package main\n{declaration}\n"),
        )]);
        let output = command("test", directory.path())
            .env("ZORE_CC", "missing-compiler-for-test")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(
            stderr(&output).contains("a test must be synchronous"),
            "{output:?}"
        );
        assert!(
            stderr(&output).contains("invalid_test.ore:2:"),
            "{output:?}"
        );
    }
}

#[test]
fn tests_reuse_ordinary_ownership_and_error_use_checking() {
    for (body, expected) in [
        (
            "let values = Array<int>{1}; let moved = values; println(values.len())",
            "moved",
        ),
        ("let err = error(\"ignored\")", "error"),
    ] {
        let directory = project(&[(
            "bad_test.ore",
            &format!("package main\nfunc TestBad() {{ {body} }}\n"),
        )]);
        let output = run(directory.path());
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(stderr(&output).contains(expected), "{output:?}");
        assert!(stderr(&output).contains("bad_test.ore:2:"), "{output:?}");
    }
}

#[test]
fn imported_and_ordinary_build_packages_exclude_test_files() {
    let directory = project(&[
        ("zore.toml", "name = \"app\"\n"),
        (
            "main.ore",
            "package main\nimport \"app/lib\"\nfunc main() { println(lib.Value()) }\n",
        ),
        (
            "main_test.ore",
            "package main\nimport \"app/lib\"\nfunc TestImport() { if lib.Value() != 7 { panic(\"import result\") } }\n",
        ),
        (
            "lib/library.ore",
            "package lib\nfunc Value() int { return 7 }\n",
        ),
        ("lib/invalid_test.ore", "not valid source"),
    ]);
    success(&run(&directory.path().join("main.ore")), 1);
    std::fs::write(directory.path().join("main_test.ore"), "not valid source").unwrap();
    for action in ["check", "run"] {
        let output = command(action, &directory.path().join("main.ore"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }
    for action in ["check", "build", "run"] {
        let output = command(action, &directory.path().join("main_test.ore"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(stderr(&output).contains("use `zore test`"), "{output:?}");
    }
}

#[test]
fn every_package_builds_before_any_test_is_executed() {
    let directory = project(&[
        (
            "a/pass_test.ore",
            "package main\nfunc TestPass() { println(\"must not run\") }\n",
        ),
        (
            "z/bad_test.ore",
            "package main\nfunc TestBad() { println(missing) }\n",
        ),
    ]);
    let output = run(directory.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(stderr(&output).contains("bad_test.ore:2:"), "{output:?}");
}

#[test]
fn no_tests_checks_packages_without_a_native_toolchain() {
    let directory = project(&[("library.ore", "package library\nfunc F() {}\n")]);
    let output = command("test", directory.path())
        .env("ZORE_CC", "missing-compiler-for-test")
        .env("ZORE_RUSTC", "missing-rustc-for-test")
        .output()
        .unwrap();
    success(&output, 0);
    std::fs::write(
        directory.path().join("library.ore"),
        "package library\nfunc F() { println(missing) }\n",
    )
    .unwrap();
    let output = run(directory.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let empty = TempDir::new().unwrap();
    success(&run(empty.path()), 0);
}

#[test]
fn a_test_only_main_package_works_but_an_invalid_application_main_is_rejected() {
    let directory = project(&[("only_test.ore", "package main\nfunc TestOnly() {}\n")]);
    success(&run(directory.path()), 1);
    std::fs::write(
        directory.path().join("main.ore"),
        "package main\nfunc main(value int) {}\n",
    )
    .unwrap();
    let output = run(directory.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        stderr(&output).contains("entry point `main` takes no parameters"),
        "{output:?}"
    );
}

#[test]
fn explicit_nonzero_exit_fails_and_later_tests_continue() {
    let directory = project(&[(
        "exit_test.ore",
        "package main\nimport \"std/os\"\nfunc TestExit() { os.Exit(3) }\nfunc TestPass() {}\n",
    )]);
    let output = run(directory.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("TestExit (exit status: 3)"), "{text}");
    assert!(text.ends_with("Tests: 1 passed, 1 failed\n"), "{text}");
}

#[test]
fn temporary_build_files_are_removed_after_success_and_failure() {
    let directory = project(&[("pass_test.ore", "package main\nfunc TestPass() {}\n")]);
    let temporary = directory.path().join("scratch with spaces");
    std::fs::create_dir(&temporary).unwrap();
    for fail in [false, true] {
        if fail {
            std::fs::write(
                directory.path().join("pass_test.ore"),
                "package main\nfunc TestFail() error { return error(\"failure\") }\n",
            )
            .unwrap();
        }
        let output = command("test", &directory.path().join("pass_test.ore"))
            .env("TMPDIR", &temporary)
            .env("TMP", &temporary)
            .env("TEMP", &temporary)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), !fail, "{output:?}");
        assert_eq!(
            std::fs::read_dir(&temporary).unwrap().count(),
            0,
            "temporary build files remained"
        );
    }
    let output = command("test", &directory.path().join("pass_test.ore"))
        .env("TMPDIR", &temporary)
        .env("TMP", &temporary)
        .env("TEMP", &temporary)
        .env("ZORE_CC", "missing-compiler-for-test")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(
        stderr(&output).contains("could not run `missing-compiler-for-test`"),
        "{output:?}"
    );
    assert_eq!(std::fs::read_dir(&temporary).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn a_signal_terminated_test_fails_and_later_tests_continue() {
    let directory = project(&[(
        "signal_test.ore",
        "package main\nimport \"std/os/exec\"\nfunc TestSignal() error { let args = Array<string>{\"-c\", \"kill -TERM $PPID\"}; let child = exec.Command(\"/bin/sh\", args[:]); return child.Run() }\nfunc TestSuccess() {}\n",
    )]);
    let output = run(directory.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("::TestSignal (signal:"), "{text}");
    assert!(text.ends_with("Tests: 1 passed, 1 failed\n"), "{text}");
}

#[cfg(unix)]
#[test]
fn recursive_targets_run_in_path_order_and_skip_symlinks_and_generated_folders() {
    use std::os::unix::fs::symlink;
    let directory = project(&[
        ("z/pass_test.ore", "package main\nfunc TestPass() {}\n"),
        ("a/pass_test.ore", "package main\nfunc TestPass() {}\n"),
        (".hidden/bad.ore", "invalid"),
        ("target/bad.ore", "invalid"),
        ("node_modules/bad.ore", "invalid"),
    ]);
    symlink(".", directory.path().join("cycle")).unwrap();
    symlink(
        "../target/bad.ore",
        directory.path().join("a/linked_test.ore"),
    )
    .unwrap();
    let output = run(directory.path());
    success(&output, 2);
    let text = stdout(&output);
    assert!(
        text.find("/a::TestPass").unwrap() < text.find("/z::TestPass").unwrap(),
        "{text}"
    );
    let output = run(&directory.path().join("cycle"));
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("symbolic links"));
}

const CONTEXT_TESTS: &str = r#"package calc

import "std/os"
import "std/testing"

func TestPlainStillRuns() {
    if add(1, 1) != 2 { panic("bad sum") }
}

func TestRecordsAndContinues(t mut testing.T) {
    t.Check(testing.Equal(add(2, 2), 5))
    t.Log("after the first failure")
    t.Check(testing.EqualSlices(Array<int>{1, 2}[:], Array<int>{1, 3}[:]))
}

func TestStopsAtQuestionMark(t mut testing.T) error {
    t.Cleanup(func() { println("cleanup: stopped test") })
    testing.Equal(add(1, 2), 4)?
    t.Fail("not reached")
    return nil
}

func TestTempDirIsRemoved(t mut testing.T) {
    let dir, err = t.TempDir()
    t.Check(err)
    t.Check(os.WriteFile(dir + "/note.txt", Array<byte>{104, 105}[:], 0o600))
    t.Cleanup(func() {
        let _, gone = os.Stat(dir + "/note.txt")
        println("note exists during cleanup: " + boolText(gone == nil))
    })
    println("dir: " + dir)
}

func TestPanicRunsCleanups(t mut testing.T) {
    t.Cleanup(func() { println("cleanup: first registered") })
    t.Cleanup(func() { println("cleanup: last registered") })
    panic("intentional context panic")
}

func boolText(value bool) string {
    if value { return "yes" }
    return "no"
}
"#;

const SUM: &str = "package calc\nfunc add(a int, b int) int { return a + b }\n";

#[test]
fn context_tests_record_failures_stop_at_errors_and_clean_up_on_every_path() {
    let directory = project(&[
        ("zore.toml", "name = \"calc\"\n"),
        ("calc.ore", SUM),
        ("calc_test.ore", CONTEXT_TESTS),
    ]);
    let output = run(directory.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output)
        .replace(&format!("{}::", directory.path().display()), ".::")
        .replace(&format!("{}/", directory.path().display()), "");
    assert!(text.ends_with("Tests: 2 passed, 3 failed\n"), "{text}");
    let block = |name: &str| {
        let start = text
            .find(&format!("RUN .::{name}\n"))
            .unwrap_or_else(|| panic!("{text}"));
        let rest = &text[start..];
        let end = rest
            .find("\nPASS ")
            .into_iter()
            .chain(rest.find("\nFAIL "))
            .min()
            .unwrap();
        let line_end = rest[end + 1..].find('\n').unwrap() + end + 1;
        rest[..line_end].to_string()
    };
    assert_eq!(
        block("TestRecordsAndContinues"),
        "RUN .::TestRecordsAndContinues\ncalc_test.ore:12:5: after the first failure\ncalc_test.ore:11:13: got 4, want 5\ncalc_test.ore:13:13: at index 1: got 2, want 3\nFAIL .::TestRecordsAndContinues (exit status: 1)"
    );
    assert_eq!(
        block("TestStopsAtQuestionMark"),
        "RUN .::TestStopsAtQuestionMark\ncleanup: stopped test\ncalc_test.ore:18:5: got 3, want 4\nFAIL .::TestStopsAtQuestionMark (exit status: 1)"
    );
    assert_eq!(
        block("TestPanicRunsCleanups"),
        "RUN .::TestPanicRunsCleanups\ncleanup: last registered\ncleanup: first registered\nFAIL .::TestPanicRunsCleanups (exit status: 2)"
    );
    assert!(
        stderr(&output).contains("intentional context panic"),
        "{output:?}"
    );
    assert!(!stderr(&output).contains("leak:"), "{output:?}");
    assert!(text.contains("PASS .::TestPlainStillRuns\n"), "{text}");
    let temp = block("TestTempDirIsRemoved");
    assert!(
        temp.ends_with("note exists during cleanup: yes\nPASS .::TestTempDirIsRemoved"),
        "{temp}"
    );
    let dir = temp
        .lines()
        .find_map(|line| line.strip_prefix("dir: "))
        .unwrap_or_else(|| panic!("{temp}"));
    assert!(dir.contains("zore-TestTempDirIsRemoved-"), "{dir}");
    assert!(!Path::new(dir).exists(), "{dir} was not removed");
}

#[test]
fn bench_runs_only_benchmarks_with_a_metadata_header() {
    let directory = project(&[
        ("zore.toml", "name = \"calc\"\n"),
        ("calc.ore", SUM),
        (
            "calc_test.ore",
            r#"package calc

import "std/testing"

func TestNotRunWithBench() {
    panic("tests must not run with --bench")
}

func BenchmarkAdd(b mut testing.B) {
    var total = 0
    for b.Loop() { total = add(total, 1) }
}

func BenchmarkStopsEarly(b mut testing.B) error {
    return error("setup failed")
}

func BenchmarkNeverLoops(b mut testing.B) {}
"#,
        ),
    ]);
    let tests = run(directory.path());
    assert_eq!(tests.status.code(), Some(1), "{tests:?}");
    assert!(
        stdout(&tests).ends_with("Tests: 0 passed, 1 failed\n"),
        "{tests:?}"
    );
    assert!(!stdout(&tests).contains("Benchmark"), "{tests:?}");
    let output = Command::new(env!("CARGO_BIN_EXE_zore"))
        .args(["test", "--bench"])
        .arg(directory.path())
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output).replace(&format!("{}::", directory.path().display()), ".::");
    let mut lines = text.lines();
    assert_eq!(
        lines.next().unwrap(),
        format!(
            "Benchmarks: zore {}, {} {}, clang -O2, at least 1s per benchmark in its own process",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    );
    assert_eq!(lines.next().unwrap(), "RUN .::BenchmarkAdd");
    let measured = lines.next().unwrap();
    let (iterations, rest) = measured
        .split_once(" iterations, ")
        .unwrap_or_else(|| panic!("{text}"));
    assert!(iterations.parse::<u64>().unwrap() > 1, "{measured}");
    assert!(
        rest.contains(" ns/op, ") && rest.ends_with(" total"),
        "{measured}"
    );
    assert_eq!(
        lines.collect::<Vec<_>>(),
        [
            "PASS .::BenchmarkAdd",
            "RUN .::BenchmarkNeverLoops",
            "benchmark BenchmarkNeverLoops did not run its Loop to the end",
            "FAIL .::BenchmarkNeverLoops (exit status: 1)",
            "RUN .::BenchmarkStopsEarly",
            "setup failed",
            "FAIL .::BenchmarkStopsEarly (exit status: 1)",
            "Benchmarks: 1 passed, 2 failed",
        ]
    );
    assert!(!stderr(&output).contains("must not run"), "{output:?}");
}

#[test]
fn context_and_benchmark_signatures_are_checked_before_building() {
    for (body, message) in [
        (
            "import \"std/testing\"\nfunc TestShared(t testing.T) {}",
            "a test must be synchronous, non-generic, take no parameters or one `t mut testing.T`",
        ),
        (
            "import \"std/testing\"\nfunc TestTwo(t mut testing.T, u mut testing.T) {}",
            "a test must be synchronous",
        ),
        (
            "import \"std/testing\"\nfunc TestWrongContext(t mut testing.B) {}",
            "a test must be synchronous",
        ),
        (
            "import \"std/testing\"\nfunc BenchmarkPlain() {}",
            "a benchmark must be synchronous, non-generic, take one `b mut testing.B`",
        ),
        (
            "import \"std/testing\"\nfunc BenchmarkInt(b mut testing.B) int { return 0 }",
            "a benchmark must be synchronous",
        ),
        (
            "type T struct {}\nfunc TestLocal(t mut T) {}",
            "a test must be synchronous",
        ),
    ] {
        let directory = project(&[
            ("zore.toml", "name = \"calc\"\n"),
            ("calc.ore", SUM),
            ("calc_test.ore", &format!("package calc\n{body}\n")),
        ]);
        for bench in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_zore"));
            command.arg("test");
            if bench {
                command.arg("--bench");
            }
            let output = command
                .arg(directory.path())
                .env("ZORE_CC", "missing-c-compiler")
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{body}: {output:?}");
            assert!(stderr(&output).contains(message), "{body}: {output:?}");
        }
    }
}

#[test]
fn the_testing_helpers_example_passes_its_tests_and_benchmark() {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/testing-helpers");
    let tests = run(&example);
    success(&tests, 5);
    assert!(stdout(&tests).contains("finished\n"), "{tests:?}");
    let bench = Command::new(env!("CARGO_BIN_EXE_zore"))
        .args(["test", "--bench"])
        .arg(&example)
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .unwrap();
    assert!(bench.status.success(), "{bench:?}");
    assert!(
        stdout(&bench).ends_with("Benchmarks: 1 passed, 0 failed\n"),
        "{bench:?}"
    );
}

#[test]
fn filter_bench_time_and_count_select_and_repeat_runs() {
    let directory = project(&[
        ("zore.toml", "name = \"calc\"\n"),
        ("calc.ore", SUM),
        (
            "calc_test.ore",
            r#"package calc

import "std/testing"

func TestAddSmall() {}

func TestAddLarge() {}

func TestOther() {}

func BenchmarkAdd(b mut testing.B) {
    var total = 0
    for b.Loop() { total = add(total, 1) }
}

func BenchmarkOther(b mut testing.B) {
    for b.Loop() {}
}
"#,
        ),
    ]);
    let zore = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_zore"))
            .args(args)
            .arg(directory.path())
            .env("ZORE_CHECK_LEAKS", "1")
            .output()
            .unwrap()
    };
    let label = format!("{}::", directory.path().display());
    let filtered = zore(&["test", "--filter", "Add"]);
    success(&filtered, 2);
    let text = stdout(&filtered).replace(&label, ".::");
    assert!(
        text.contains("PASS .::TestAddLarge\n") && text.contains("PASS .::TestAddSmall\n"),
        "{text}"
    );
    assert!(!text.contains("TestOther"), "{text}");
    let bench = zore(&[
        "test",
        "--bench",
        "--filter",
        "Add",
        "--bench-time",
        "50ms",
        "--count",
        "2",
    ]);
    assert!(bench.status.success(), "{bench:?}");
    let text = stdout(&bench).replace(&label, ".::");
    assert!(
        text.starts_with(&format!(
            "Benchmarks: zore {}, {} {}, clang -O2, at least 50ms per benchmark in its own process, 2 runs each\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        )),
        "{text}"
    );
    assert_eq!(text.matches("RUN .::BenchmarkAdd\n").count(), 2, "{text}");
    assert!(!text.contains("BenchmarkOther"), "{text}");
    assert!(text.ends_with("Benchmarks: 2 passed, 0 failed\n"), "{text}");
    for (args, message) in [
        (
            &["test", "--count", "2"][..],
            "--bench-time and --count apply only with --bench",
        ),
        (
            &["test", "--bench", "--bench-time", "5"][..],
            "--bench-time requires a duration",
        ),
        (
            &["test", "--bench", "--bench-time", "0ms"][..],
            "--bench-time requires a duration",
        ),
        (
            &["test", "--bench", "--count", "0"][..],
            "--count requires a whole number from 1 to 100",
        ),
        (
            &["test", "--bench", "--count", "101"][..],
            "--count requires a whole number from 1 to 100",
        ),
        (
            &["test", "--filter", ""][..],
            "--filter requires a non-empty",
        ),
    ] {
        let output = zore(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(stderr(&output).contains(message), "{args:?}: {output:?}");
    }
}
