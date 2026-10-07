use std::ffi::OsStr;
use std::process::{Command, Output};

fn invoke(args: &[impl AsRef<OsStr>]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zore"))
        .args(args)
        .output()
        .expect("run zore binary")
}

fn failure(output: &Output, code: i32, message: &str) {
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{output:?}"
    );
}

#[test]
fn help_is_successful_and_honest_about_support() {
    for flag in ["--help", "-h"] {
        let output = invoke(&[flag]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Usage: zore"));
        assert!(stdout.contains("check <file.ore>"));
        assert!(stdout.contains("Not implemented yet"));
        assert!(stdout.contains("build <file.ore>"));
        assert!(stdout.contains("run <file.ore>"));
    }
}

#[test]
fn version_comes_from_package_metadata() {
    for flag in ["--version", "-V"] {
        let output = invoke(&[flag]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(
            output.stdout,
            format!("zore {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
        );
    }
}

#[test]
fn no_command_is_a_usage_error() {
    failure(&invoke(&[] as &[&str]), 2, "expected a command");
}

#[test]
fn unknown_commands_and_options_are_usage_errors() {
    for args in [
        vec!["unknown"],
        vec!["--unknown"],
        vec!["check", "--unknown"],
        vec!["check", "main.ore", "--unknown"],
    ] {
        failure(&invoke(&args), 2, "unknown");
    }
}

#[test]
fn informational_flags_do_not_hide_extra_arguments() {
    for args in [
        vec!["--help", "check"],
        vec!["--version", "main.ore"],
        vec!["check", "--help", "main.ore"],
        vec!["check", "main.ore", "--help"],
    ] {
        failure(&invoke(&args), 2, "zore:");
    }
}

#[test]
fn commands_validate_target_count_and_offer_help() {
    for action in ["check", "build", "run", "fmt", "test"] {
        for tail in [vec![], vec!["--"], vec!["one.ore", "two.ore"]] {
            let mut args = vec![action];
            args.extend(tail);
            failure(&invoke(&args), 2, "requires exactly one target");
        }
        failure(&invoke(&[action, ""]), 2, "must not be empty");
        for flag in ["--help", "-h"] {
            let output = invoke(&[action, flag]);
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains(&format!("Usage: zore {action}")));
            match action {
                "check" => assert!(stdout.contains("Only part of the")),
                "build" | "run" => assert!(stdout.contains("Requires clang")),
                _ => assert!(stdout.contains("not implemented yet")),
            }
        }
    }
}

#[test]
fn unimplemented_commands_never_claim_success() {
    for action in ["fmt", "test"] {
        for target in [".", "missing.ore", "a path with spaces.ore"] {
            failure(&invoke(&[action, target]), 1, "not implemented yet");
        }
    }
    for action in ["build", "run"] {
        failure(&invoke(&[action, "missing.ore"]), 1, "zore: missing.ore:");
    }
}

#[test]
fn check_reports_unreadable_targets() {
    failure(&invoke(&["check", "missing.ore"]), 1, "zore: missing.ore:");
    failure(
        &invoke(&["check", "a path with spaces.ore"]),
        1,
        "a path with spaces.ore",
    );
    failure(&invoke(&["check", "."]), 1, "zore: .:");
}

#[test]
fn check_accepts_supported_examples_silently() {
    let root = format!("{}/../examples", env!("CARGO_MANIFEST_DIR"));
    let examples: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("main.ore").is_file())
        .map(|path| path.join("main.ore").to_string_lossy().into_owned())
        .collect();
    assert!(examples.len() >= 13, "{examples:?}");
    for path in examples {
        let output = invoke(&["check", &path]);
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stdout.is_empty() && output.stderr.is_empty(),
            "{output:?}"
        );
    }
}

#[test]
fn check_renders_diagnostics_and_fails() {
    let dir = std::env::temp_dir().join(format!("zore-cli-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.ore");
    std::fs::write(
        &path,
        "package main\n\nfunc main() {\n    let x uint8 = 300\n    println(y)\n}\n",
    )
    .unwrap();
    let output = invoke(&[OsStr::new("check"), path.as_os_str()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("error: integer constant `300` does not fit in `uint8`"),
        "{stderr}"
    );
    assert!(stderr.contains("bad.ore:4:19"), "{stderr}");
    assert!(
        stderr.contains("error: cannot find `y` in this scope"),
        "{stderr}"
    );
    assert!(
        stderr.ends_with("zore: check failed with 2 errors\n"),
        "{stderr}"
    );
}

#[test]
fn delimiter_allows_option_shaped_targets() {
    for target in ["-source.ore", "--help", "--version", "--"] {
        failure(
            &invoke(&["check", "--", target]),
            1,
            &format!("zore: {target}:"),
        );
        failure(&invoke(&["fmt", "--", target]), 1, "was not processed");
    }
    failure(&invoke(&["check", "-source.ore"]), 2, "unknown option");
    failure(
        &invoke(&["check", "--", "one.ore", "two.ore"]),
        2,
        "requires exactly one target",
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_do_not_panic() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let path = OsString::from_vec(b"source-\xff.ore".to_vec());
    failure(
        &invoke(&[OsString::from("check"), path.clone()]),
        1,
        "zore: source-",
    );
    failure(
        &invoke(&[OsString::from("fmt"), path.clone()]),
        1,
        "was not processed",
    );
    failure(&invoke(&[path]), 2, "unknown command or option");
}

/// Writes a project into a fresh folder and returns it.
fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("zore-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    dir
}

#[test]
fn check_follows_imports_and_names_the_file_with_the_error() {
    let dir = project(
        "imports",
        &[
            ("zore.toml", "name = \"app\"\n"),
            (
                "main.ore",
                "package main\nimport \"app/lib\"\nfunc main() { lib.F() }\n",
            ),
            (
                "lib/lib.ore",
                "package lib\nfunc F() { println(missing) }\n",
            ),
        ],
    );
    let output = invoke(&[OsStr::new("check"), dir.join("main.ore").as_os_str()]);
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("cannot find `missing`"), "{stderr}");
    assert!(stderr.contains("lib.ore:2:"), "{stderr}");

    std::fs::write(dir.join("lib/lib.ore"), "package lib\nfunc F() {}\n").unwrap();
    let output = invoke(&[OsStr::new("check"), dir.join("main.ore").as_os_str()]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "{output:?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn import_problems_are_reported_without_a_panic() {
    let dir = project(
        "bad-imports",
        &[
            ("zore.toml", "name = \"app\"\n"),
            (
                "main.ore",
                "package main\nimport \"app/a\"\nimport \"other/b\"\nimport \"zore/nothing\"\nfunc main() {}\n",
            ),
            ("a/a.ore", "package a\nimport \"app/a\"\n"),
        ],
    );
    let output = invoke(&[OsStr::new("check"), dir.join("main.ore").as_os_str()]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    for message in [
        "import cycle: app/a -> app/a",
        "package `other/b` was not found",
        "no standard package `zore/nothing`",
    ] {
        assert!(stderr.contains(message), "{message}: {stderr}");
    }
    assert!(
        stderr.ends_with("zore: check failed with 3 errors\n"),
        "{stderr}"
    );
}

#[test]
fn only_a_main_package_runs_but_any_package_checks() {
    let dir = project(
        "library",
        &[(
            "lib.ore",
            "package library\nfunc Hello() string { return \"hi\" }\n",
        )],
    );
    let path = dir.join("lib.ore");
    assert!(
        invoke(&[OsStr::new("check"), path.as_os_str()])
            .status
            .success()
    );
    let output = invoke(&[OsStr::new("run"), path.as_os_str()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is not executable"),
        "{output:?}"
    );
}

#[test]
fn a_file_is_built_together_with_the_rest_of_its_folder() {
    let dir = project(
        "folder",
        &[
            ("main.ore", "package main\nfunc main() { helper() }\n"),
            ("helper.ore", "package main\nfunc helper() { println(1) }\n"),
        ],
    );
    let output = invoke(&[OsStr::new("check"), dir.join("main.ore").as_os_str()]);
    assert!(output.status.success(), "{output:?}");
    std::fs::write(dir.join("helper.ore"), "package main\nfunc helper( {\n").unwrap();
    let output = invoke(&[OsStr::new("check"), dir.join("main.ore").as_os_str()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("helper.ore:2:"),
        "{output:?}"
    );
}
