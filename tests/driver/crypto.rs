use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use zore::build::TempDir;

fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    output
}

fn write(root: &Path, name: &str, contents: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

struct Fixture {
    _directory: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let root = directory.path().join("crypto café with spaces");
        for (name, contents) in [
            (
                "crypto/zore.toml",
                include_str!("../../examples/crypto/crypto/zore.toml"),
            ),
            (
                "crypto/LICENSE",
                include_str!("../../examples/crypto/crypto/LICENSE"),
            ),
            (
                "crypto/crypto.ore",
                include_str!("../../examples/crypto/crypto/crypto.ore"),
            ),
            (
                "crypto/crypto_test.ore",
                include_str!("../../examples/crypto/crypto/crypto_test.ore"),
            ),
            (
                "crypto/native/crypto.c",
                include_str!("../../examples/crypto/crypto/native/crypto.c"),
            ),
            (
                "crypto/native/crypto.h",
                include_str!("../../examples/crypto/crypto/native/crypto.h"),
            ),
            (
                "crypto/native/bridge.rs",
                include_str!("../../examples/crypto/crypto/native/bridge.rs"),
            ),
            (
                "crypto/native/bindings.abi",
                include_str!("../../examples/crypto/crypto/native/bindings.abi"),
            ),
            (
                "app/zore.toml",
                include_str!("../../examples/crypto/app/zore.toml"),
            ),
            (
                "app/main.ore",
                include_str!("../../examples/crypto/app/main.ore"),
            ),
        ] {
            write(&root, name, contents);
        }
        fs::copy(
            env!("CARGO_BIN_EXE_zore"),
            root.join(format!("compiler{}", std::env::consts::EXE_SUFFIX)),
        )
        .unwrap();
        Self {
            _directory: directory,
            root,
        }
    }

    fn command(&self, action: &str, target: &str) -> Command {
        let mut command = Command::new(
            self.root
                .join(format!("compiler{}", std::env::consts::EXE_SUFFIX)),
        );
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
}

#[test]
fn reviewed_crypto_runs_known_answers_random_and_concurrency_offline() {
    let fixture = Fixture::new();
    success(fixture.invoke("install", "app"));
    let lock = fs::read_to_string(fixture.root.join("app/zore.lock")).unwrap();
    assert!(lock.contains("adapter = \"crypto-v1\""));
    success(
        fixture
            .command("check", "app/main.ore")
            .arg("--locked")
            .env("ZORE_CC", "missing-native-toolchain")
            .env("ZORE_RUSTC", "missing-runtime-toolchain")
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
        b"true\n32\n"
    );
    success(fixture.invoke("install", "crypto"));
    let output = success(
        fixture
            .command("test", "crypto")
            .arg("--locked")
            .output()
            .unwrap(),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Tests: 7 passed, 0 failed"),
        "{output:?}"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("app/zore.lock")).unwrap(),
        lock
    );
    for name in ["crypto.c", "crypto.h", "bridge.rs", "bindings.abi"] {
        let path = fixture.root.join("crypto/native").join(name);
        let original = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("{original}\n")).unwrap();
        let output = fixture.invoke("lock", "app");
        assert!(!output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("native sources do not match reviewed adapter"),
            "{output:?}"
        );
        fs::write(path, original).unwrap();
    }
    write(
        &fixture.root,
        "app/main.ore",
        "package main\nimport \"crypto\"\nfunc main() { let bytes, err = crypto.nativeRandomBytes(1); println(bytes.len()) }\n",
    );
    let output = fixture.invoke("check", "app/main.ore");
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not exported"),
        "{output:?}"
    );
    for (name, contents) in [
        (
            "zore.toml",
            include_str!("../../examples/native-library/adler32/zore.toml"),
        ),
        (
            "adler32.ore",
            include_str!("../../examples/native-library/adler32/adler32.ore"),
        ),
        (
            "native/adler32.c",
            include_str!("../../examples/native-library/adler32/native/adler32.c"),
        ),
        (
            "native/adler32.h",
            include_str!("../../examples/native-library/adler32/native/adler32.h"),
        ),
        (
            "native/bindings.abi",
            include_str!("../../examples/native-library/adler32/native/bindings.abi"),
        ),
        (
            "native/bridge.rs",
            include_str!("../../examples/native-library/adler32/native/bridge.rs"),
        ),
    ] {
        write(&fixture.root, &format!("adler32/{name}"), contents);
    }
    write(
        &fixture.root,
        "app/zore.toml",
        "name = 'cryptoapp'\n[dependencies]\ncrypto = { path = '../crypto' }\nadler32 = { path = '../adler32' }\n",
    );
    write(
        &fixture.root,
        "app/main.ore",
        r#"package main
import "crypto"
import "adler32"
func example() error {
    let data = crypto.RandomBytes(32)?
    let digest = crypto.SHA256(data[:])?
    var checksum = adler32.New()?
    checksum.Write(digest[:])?
    let sum = checksum.Sum()?
    if digest.len() != 32 || sum == 1 { panic("mixed providers") }
    println("mixed providers")
    return nil
}
func main() { let err = example(); if err != nil { panic("mixed provider error") } }
"#,
    );
    success(fixture.invoke("lock", "app"));
    success(fixture.invoke("install", "app"));
    assert_eq!(
        success(
            fixture
                .command("run", "app/main.ore")
                .arg("--locked")
                .output()
                .unwrap()
        )
        .stdout,
        b"mixed providers\n"
    );
}

#[test]
fn owned_crypto_buffers_clean_up_on_error_panic_and_unobserved_async_results() {
    let fixture = Fixture::new();
    write(
        &fixture.root,
        "app/main.ore",
        r#"package main
import "crypto"
import "std/os"
import "std/time"
async func make() (Array<byte>, error) {
    let bytes = crypto.RandomBytes(64)?
    time.Sleep(time.Millisecond)
    return crypto.SHA256(bytes[:])
}
async func consume(mode string) error {
    let bytes = crypto.RandomBytes(64)?
    let hash = crypto.SHA256(bytes[:])?
    time.Sleep(time.Millisecond)
    if mode == "panic" { panic("crypto task panic") }
    if mode == "error" { let invalid = crypto.RandomBytes(-1)? }
    if hash.len() != 32 { panic("digest size") }
    return nil
}
func execute(mode string) error {
    let secret = crypto.RandomBytes(32)?
    scope {
        let unused = go make()
        let child = go consume(mode)
        child.wait()?
    }
    println(secret.len())
    return nil
}
func main() {
    let args = os.Args()
    let err = execute(args[1])
    if err != nil { println("expected error") }
}
"#,
    );
    success(fixture.invoke("install", "app"));
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
            "normal" => assert_eq!(success(output).stdout, b"32\n"),
            "error" => assert_eq!(success(output).stdout, b"expected error\n"),
            "panic" => {
                assert_eq!(output.status.code(), Some(2), "{output:?}");
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(stderr.contains("crypto task panic"), "{output:?}");
                assert!(!stderr.contains("leak:"), "{output:?}");
            }
            _ => unreachable!(),
        }
    }
}

#[path = "../../compiler/src/driver/mbedtls.rs"]
mod mbedtls;

#[test]
fn real_provider_failure_paths_clear_partial_output_and_delegate_comparison() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    assert_eq!(mbedtls::FINGERPRINT.len(), 64);
    assert!(mbedtls::RECIPE.starts_with("mbedtls-3.6.7-"));
    for &(name, contents) in mbedtls::SOURCES {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    write(
        root,
        "crypto.c",
        include_str!("../../examples/crypto/crypto/native/crypto.c"),
    );
    write(
        root,
        "crypto.h",
        include_str!("../../examples/crypto/crypto/native/crypto.h"),
    );
    write(
        root,
        "provider.c",
        include_str!("../fixtures/crypto/provider.c"),
    );
    let executable = directory.executable("provider");
    let mut command = Command::new(zore::build::compiler());
    command.args([
        "-std=c11",
        "-O2",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-DMBEDTLS_CONFIG_FILE=\"zore_config.h\"",
        "-D_DEFAULT_SOURCE",
    ]);
    if cfg!(windows) {
        command.arg("--target=x86_64-pc-windows-msvc");
    }
    for include in ["include", "library", ""] {
        command.arg("-I").arg(root.join(include));
    }
    for &(name, _) in mbedtls::SOURCES {
        if name.ends_with(".c") {
            command.arg(root.join(name));
        }
    }
    command
        .arg(root.join("provider.c"))
        .arg("-o")
        .arg(&executable);
    if cfg!(windows) {
        command.arg("-l").arg("bcrypt");
    }
    success(command.output().unwrap());
    success(Command::new(executable).output().unwrap());
}

#[test]
fn rust_bridge_translates_provider_failures_and_rejects_invalid_abi_lengths() {
    let directory = TempDir::new().unwrap();
    write(
        directory.path(),
        "bridge.rs",
        include_str!("../../examples/crypto/crypto/native/bridge.rs"),
    );
    write(
        directory.path(),
        "harness.rs",
        include_str!("../fixtures/crypto/bridge.rs"),
    );
    let executable = directory.executable("bridge-tests");
    success(
        Command::new(std::env::var_os("ZORE_RUSTC").unwrap_or_else(|| "rustc".into()))
            .args(["--edition=2024", "--test", "-Dwarnings"])
            .arg(directory.path().join("harness.rs"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    success(Command::new(executable).output().unwrap());
}
