use super::*;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const TOOL: &str = r#"
use std::{env, fs, process, thread};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let executable = env::current_exe().unwrap();
    let clang = executable.file_stem().unwrap().to_string_lossy().starts_with("clang");
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    let stage = if clang && arguments.iter().any(|a| a == OsStr::new("-c"))
        && arguments.iter().any(|a| a.to_string_lossy().ends_with(".ll")) {
        Some(("object", "runtime"))
    } else if !clang && arguments.iter().any(|a| a == OsStr::new("--crate-type=rlib")) {
        Some(("runtime", "object"))
    } else { None };
    let markers = PathBuf::from(env::var_os("ZORE_TEST_BUILD_MARKERS").unwrap());
    if let Some((name, other)) = stage {
        fs::write(markers.join(format!("{name}.started")), b"").unwrap();
        if env::var_os("ZORE_TEST_BUILD_OVERLAP").is_some() {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !markers.join(format!("{other}.started")).is_file() {
                if Instant::now() >= deadline {
                    eprintln!("build stages did not overlap");
                    process::exit(71);
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
        let failure = env::var("ZORE_TEST_BUILD_FAILURE").unwrap_or_default();
        if failure == name || failure == "both" {
            fs::write(markers.join(format!("{name}.finished")), b"").unwrap();
            eprintln!("forced {name} failure");
            process::exit(72);
        }
    }
    let real = env::var_os(if clang { "ZORE_TEST_REAL_CC" } else { "ZORE_TEST_REAL_RUSTC" }).unwrap();
    let status = process::Command::new(real).args(arguments).status().unwrap();
    if let Some((name, _)) = stage {
        fs::write(markers.join(format!("{name}.finished")), b"").unwrap();
    }
    process::exit(status.code().unwrap_or(1));
}
"#;

fn tools(directory: &TempDir) -> (PathBuf, PathBuf) {
    let source = directory.path().join("tool.rs");
    let clang = directory.executable("clang gate");
    let rustc = directory.executable("rustc gate");
    fs::write(&source, TOOL).unwrap();
    let output = bounded_process::run(
        Command::new(std::env::var_os("ZORE_RUSTC").unwrap_or_else(|| "rustc".into()))
            .args(["--edition=2024", "--crate-name=build_gate"])
            .arg(&source)
            .arg("-o")
            .arg(&clang),
        Duration::from_secs(60),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    fs::copy(&clang, &rustc).unwrap();
    (clang, rustc)
}

fn build_command(folder: &Path, clang: &Path, rustc: &Path, cache: &Path) -> Command {
    fs::create_dir_all(folder).unwrap();
    fs::write(folder.join("main.ore"), main_body("println(42)")).unwrap();
    let mut command = zore();
    command
        .arg("build")
        .arg(folder.join("main.ore"))
        .current_dir(folder)
        .env("ZORE_CC", clang)
        .env("ZORE_RUSTC", rustc)
        .env("ZORE_CACHE_DIR", cache)
        .env("ZORE_TEST_BUILD_MARKERS", folder)
        .env("ZORE_TEST_REAL_CC", zore::build::compiler())
        .env(
            "ZORE_TEST_REAL_RUSTC",
            std::env::var_os("ZORE_RUSTC").unwrap_or_else(|| "rustc".into()),
        );
    command
}

#[test]
fn cold_build_overlaps_native_compilers_and_warm_build_reuses_runtime() {
    let directory = TempDir::new().unwrap();
    let (clang, rustc) = tools(&directory);
    let cache = directory.path().join("cache");
    let cold = directory.path().join("cold");
    let output = bounded_process::run(
        build_command(&cold, &clang, &rustc, &cache).env("ZORE_TEST_BUILD_OVERLAP", "1"),
        Duration::from_secs(120),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(cold.join("object.finished").is_file());
    assert!(cold.join("runtime.finished").is_file());
    let warm = directory.path().join("warm");
    let output = bounded_process::run(
        &mut build_command(&warm, &clang, &rustc, &cache),
        Duration::from_secs(120),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(warm.join("object.finished").is_file());
    assert!(!warm.join("runtime.started").exists());
    for folder in [cold, warm] {
        let output = Command::new(folder.join(format!("main{}", std::env::consts::EXE_SUFFIX)))
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(stdout(&output), "42\n");
    }
}

#[test]
fn concurrent_compiler_failures_join_both_stages_and_preserve_error_priority() {
    let directory = TempDir::new().unwrap();
    let (clang, rustc) = tools(&directory);
    for failure in ["object", "runtime", "both"] {
        let folder = directory.path().join(failure);
        let output = bounded_process::run(
            build_command(&folder, &clang, &rustc, &folder.join("cache"))
                .env("ZORE_TEST_BUILD_OVERLAP", "1")
                .env("ZORE_TEST_BUILD_FAILURE", failure),
            Duration::from_secs(120),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        assert!(folder.join("object.finished").is_file());
        assert!(folder.join("runtime.finished").is_file());
        let expected = if failure == "runtime" {
            "runtime"
        } else {
            "object"
        };
        assert!(
            stderr(&output).contains(&format!("forced {expected} failure")),
            "{}",
            stderr(&output)
        );
        assert!(
            !folder
                .join(format!("main{}", std::env::consts::EXE_SUFFIX))
                .exists()
        );
    }
}
