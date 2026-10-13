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
