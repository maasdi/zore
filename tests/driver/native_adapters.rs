use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use zore::build::{TempDir, compiler};

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    output
}

fn failure(output: Output, message: &str) {
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{output:?}"
    );
}

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("native projects café with spaces");
        for (path, text) in [
            (
                "app/zore.toml",
                include_str!("../../examples/native-library/app/zore.toml"),
            ),
            (
                "app/main.ore",
                include_str!("../../examples/native-library/app/main.ore"),
            ),
            (
                "adler32/zore.toml",
                include_str!("../../examples/native-library/adler32/zore.toml"),
            ),
            (
                "adler32/adler32.ore",
                include_str!("../../examples/native-library/adler32/adler32.ore"),
            ),
            (
                "adler32/adler32_test.ore",
                include_str!("../../examples/native-library/adler32/adler32_test.ore"),
            ),
        ] {
            write(&root, path, text);
        }
        Self {
            _temporary: temporary,
            root,
        }
    }

    fn command(&self, action: &str, target: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zore"));
        command
            .arg(action)
            .arg("--offline")
            .arg(self.root.join(target))
            .current_dir(&self.root)
            .env("ZORE_CACHE_DIR", self.root.join("cache"))
            .env("ZORE_CHECK_LEAKS", "1");
        command
    }

    fn invoke(&self, action: &str, target: &str) -> Output {
        self.command(action, target).output().unwrap()
    }

    fn install(&self) {
        success(self.invoke("install", "app"));
    }
}

#[test]
fn local_package_checks_builds_runs_and_tests_with_static_native_linking() {
    let fixture = Fixture::new();
    let original = fs::read(fixture.root.join("app/main.ore")).unwrap();
    failure(
        fixture.invoke("check", "app/main.ore"),
        "missing or mismatched lockfile",
    );
    fixture.install();
    let lock = fs::read(fixture.root.join("app/zore.lock")).unwrap();
    success(
        fixture
            .command("check", "app/main.ore")
            .arg("--locked")
            .env("ZORE_CC", "missing-native-compiler")
            .env("ZORE_RUSTC", "missing-rust-compiler")
            .output()
            .unwrap(),
    );
    success(
        fixture
            .command("build", "app/main.ore")
            .arg("--locked")
            .output()
            .unwrap(),
    );
    let executable = fixture
        .root
        .join(format!("main{}", std::env::consts::EXE_SUFFIX));
    assert_eq!(
        success(
            Command::new(&executable)
                .env("ZORE_CHECK_LEAKS", "1")
                .output()
                .unwrap()
        )
        .stdout,
        b"300286872\n"
    );
    assert_eq!(
        success(fixture.invoke("run", "app/main.ore")).stdout,
        b"300286872\n"
    );
    success(fixture.invoke("install", "adler32"));
    let tests = success(
        fixture
            .command("test", "adler32")
            .arg("--locked")
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8_lossy(&tests.stdout).contains("Tests: 5 passed, 0 failed"));
    assert_eq!(fs::read(fixture.root.join("app/zore.lock")).unwrap(), lock);
    assert_eq!(
        fs::read(fixture.root.join("app/main.ore")).unwrap(),
        original
    );
    write(
        &fixture.root,
        "plain/main.ore",
        "package main\nfunc main() { println(7) }\n",
    );
    assert_eq!(
        success(fixture.invoke("run", "plain/main.ore")).stdout,
        b"7\n"
    );
    assert_eq!(
        success(fixture.invoke("run", "app/main.ore")).stdout,
        b"300286872\n"
    );
    let runtimes = fs::read_dir(fixture.root.join("cache/zore"))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .as_encoded_bytes()
                .starts_with(b"runtime-")
        })
        .count();
    assert_eq!(
        runtimes, 2,
        "native selections must have distinct runtime cache keys"
    );
}

#[test]
fn native_recipe_lock_drift_is_explicit_and_never_silently_rewritten() {
    let fixture = Fixture::new();
    fixture.install();
    let lock_path = fixture.root.join("app/zore.lock");
    let expected = fs::read_to_string(&lock_path).unwrap();
    assert!(expected.contains("adapter = \"adler32-v1\", recipe = \""));
    let corrupt = expected.replacen("recipe = \"", "recipe = \"changed", 1);
    fs::write(&lock_path, &corrupt).unwrap();
    failure(
        fixture
            .command("check", "app/main.ore")
            .arg("--locked")
            .output()
            .unwrap(),
        "mismatched lockfile",
    );
    failure(
        fixture
            .command("install", "app")
            .arg("--locked")
            .output()
            .unwrap(),
        "mismatched lockfile",
    );
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), corrupt);
    success(fixture.invoke("lock", "app"));
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), expected);
    write(
        &fixture.root,
        "adler32/zore.toml",
        "name = 'adler32'\n[native]\nadapter = 'unregistered-v2'\n",
    );
    failure(
        fixture.invoke("check", "app/main.ore"),
        "unknown native adapter",
    );
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), expected);
}

const ASYNC: &str = r#"package main
import "adler32"
import "std/time"
import "std/os"
import "std/errors"

async func consume(value own adler32.Hasher, mode string) error {
    var hasher = value
    time.Sleep(time.Millisecond)
    if mode == "panic" { panic("native task panic") }
    if mode == "error" {
        var oversized = Array<uint8>{}
        for var i = 0; i < 65537; i += 1 { oversized.push(0) }
        hasher.Write(oversized[:])?
    }
    let data = Array<uint8>{0, 255, 128, 65}
    hasher.Write(data[:])?
    println(hasher.Sum()?)
    return nil
}
func execute(mode string) error {
    let parent = adler32.New()?
    let child = adler32.New()?
    let task = go consume(child, mode)
    task.wait()?
    println(parent.Sum()?)
    return nil
}
func main() {
    let args = os.Args()
    let err = execute(args[1])
    if err != nil { println(errors.Message(err)) }
}
"#;

#[test]
fn owned_native_resources_survive_async_waits_and_clean_up_on_error_and_panic() {
    let fixture = Fixture::new();
    write(&fixture.root, "app/main.ore", ASYNC);
    fixture.install();
    success(fixture.invoke("build", "app/main.ore"));
    let executable = fixture
        .root
        .join(format!("main{}", std::env::consts::EXE_SUFFIX));
    for mode in ["normal", "error", "panic"] {
        let output = Command::new(&executable)
            .arg(mode)
            .env("ZORE_CHECK_LEAKS", "1")
            .output()
            .unwrap();
        match mode {
            "normal" => assert_eq!(success(output).stdout, b"71434689\n1\n"),
            "error" => assert_eq!(
                success(output).stdout,
                b"native adler32: write requires at most 65536 readable bytes\n"
            ),
            "panic" => {
                assert_eq!(output.status.code(), Some(2), "{output:?}");
                assert!(output.stdout.is_empty(), "{output:?}");
                assert!(String::from_utf8_lossy(&output.stderr).contains("native task panic"));
                assert!(!String::from_utf8_lossy(&output.stderr).contains("leak:"));
            }
            _ => unreachable!(),
        }
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "core.autocrlf=false",
            "-c",
            "user.name=Zore tests",
            "-c",
            "user.email=zore@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
fn pinned_git_native_provider_is_shared_by_a_transitive_offline_diamond() {
    let fixture = Fixture::new();
    let library = fixture.root.join("adler32");
    git(&library, &["init", "-q"]);
    git(&library, &["add", "."]);
    git(&library, &["commit", "-qm", "native fixture"]);
    let revision = git(&library, &["rev-parse", "HEAD"]);
    let path = library.to_str().unwrap().replace('\\', "/");
    let url = format!(
        "file://{}{path}",
        if path.starts_with('/') { "" } else { "/" }
    );
    let manifest = format!("[dependencies]\nadler32 = {{ git = '{url}', rev = '{revision}' }}\n");
    write(
        &fixture.root,
        "left/zore.toml",
        &format!("name = 'left'\n{manifest}"),
    );
    write(
        &fixture.root,
        "right/zore.toml",
        &format!("name = 'right'\n{manifest}"),
    );
    write(
        &fixture.root,
        "left/left.ore",
        "package left\nimport \"adler32\"\nimport \"std/strings\"\nfunc New() (adler32.Hasher, error) { var h = adler32.New()?; let bytes = strings.Bytes(\"Wikipedia\"); h.Write(bytes[:])?; return h, nil }\n",
    );
    write(
        &fixture.root,
        "right/right.ore",
        "package right\nimport \"adler32\"\nfunc Sum(h adler32.Hasher) (int, error) { return h.Sum() }\n",
    );
    write(
        &fixture.root,
        "app/zore.toml",
        "name = 'checksumapp'\n[dependencies]\nleft = { path = '../left' }\nright = { path = '../right' }\n",
    );
    write(
        &fixture.root,
        "app/main.ore",
        "package main\nimport \"left\"\nimport \"right\"\nfunc checksum() (int, error) { let h = left.New()?; return right.Sum(h) }\nfunc main() { let value, err = checksum(); if err != nil { panic(\"checksum\") }; println(value) }\n",
    );
    write(
        &fixture.root,
        "app/main_test.ore",
        "package main\nfunc TestChecksum() error { let value = checksum()?; if value != 300286872 { panic(\"checksum\") }; return nil }\n",
    );
    success(
        Command::new(env!("CARGO_BIN_EXE_zore"))
            .arg("install")
            .arg(fixture.root.join("app"))
            .env("ZORE_CACHE_DIR", fixture.root.join("cache"))
            .output()
            .unwrap(),
    );
    let lock = fs::read_to_string(fixture.root.join("app/zore.lock")).unwrap();
    assert_eq!(lock.matches("adapter = \"adler32-v1\"").count(), 1);
    assert!(lock.contains(&revision));
    fs::rename(&library, fixture.root.join("unavailable")).unwrap();
    success(
        fixture
            .command("check", "app/main.ore")
            .arg("--locked")
            .output()
            .unwrap(),
    );
    assert_eq!(
        success(
            fixture
                .command("run", "app/main.ore")
                .arg("--locked")
                .output()
                .unwrap()
        )
        .stdout,
        b"300286872\n"
    );
    let tests = success(
        fixture
            .command("test", "app")
            .arg("--locked")
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8_lossy(&tests.stdout).contains("Tests: 1 passed, 0 failed"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("app/zore.lock")).unwrap(),
        lock
    );
}

#[test]
fn curated_c_provider_validates_lengths_and_preserves_state_on_failure() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    write(
        root,
        "adler32.c",
        include_str!("../../runtime/native/adler32.c"),
    );
    write(
        root,
        "adler32.h",
        include_str!("../../runtime/native/adler32.h"),
    );
    write(
        root,
        "test.c",
        r#"#include "adler32.h"
#include <stddef.h>
int main(void) {
    void *state = zore_adler32_create();
    if (state == NULL || zore_adler32_sum(state) != 1) return 1;
    const uint8_t bytes[] = {0, 255, 128, 65};
    if (zore_adler32_write(state, NULL, 0) != 0) return 2;
    if (zore_adler32_write(state, NULL, 1) == 0) return 3;
    if (zore_adler32_write(state, bytes, 65537) == 0) return 4;
    if (zore_adler32_sum(state) != 1) return 5;
    if (zore_adler32_write(state, bytes, 4) != 0) return 6;
    if (zore_adler32_sum(state) != 71434689) return 7;
    if (zore_adler32_write(NULL, bytes, 4) == 0) return 8;
    zore_adler32_free(state);
    zore_adler32_free(NULL);
    return 0;
}
"#,
    );
    let executable = directory.executable("c-boundary");
    let mut command = Command::new(compiler());
    command.args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"]);
    if cfg!(windows) {
        command.arg("--target=x86_64-pc-windows-msvc");
    }
    command
        .arg(root.join("adler32.c"))
        .arg(root.join("test.c"))
        .arg("-o")
        .arg(&executable);
    success(command.output().unwrap());
    success(Command::new(&executable).output().unwrap());
}

#[test]
fn scoped_unobserved_native_results_are_destroyed_on_normal_and_parent_panic_exits() {
    let fixture = Fixture::new();
    write(
        &fixture.root,
        "app/main.ore",
        r#"package main
import "adler32"
import "std/time"
import "std/os"
async func make() (adler32.Hasher, error) {
    time.Sleep(time.Millisecond)
    return adler32.New()
}
func main() {
    let args = os.Args()
    scope {
        let child = go make()
        if args[1] == "panic" { panic("parent native panic") }
    }
    println("joined")
}
"#,
    );
    fixture.install();
    success(fixture.invoke("build", "app/main.ore"));
    let executable = fixture
        .root
        .join(format!("main{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new(&executable)
        .arg("normal")
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .unwrap();
    assert_eq!(success(output).stdout, b"joined\n");
    let output = Command::new(&executable)
        .arg("panic")
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("parent native panic"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("leak:"));
}
