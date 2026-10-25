use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use zore::build::TempDir;

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

fn success(mut command: Command) -> Output {
    let directory = TempDir::new().unwrap();
    let output_path = directory.path().join("stdout");
    let error_path = directory.path().join("stderr");
    let mut child = command
        .stdout(Stdio::from(fs::File::create(&output_path).unwrap()))
        .stderr(Stdio::from(fs::File::create(&error_path).unwrap()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("HTTPS client subprocess exceeded 180 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = Output {
        status,
        stdout: fs::read(output_path).unwrap(),
        stderr: fs::read(error_path).unwrap(),
    };
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    output
}

#[test]
fn offline_https_do_verifies_peer_and_preserves_fragmented_binary_bytes() {
    let directory = TempDir::new().unwrap();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let app = directory.path().join("https-client");
    copy_tree(&examples.join("https-client"), &app);
    copy_tree(&examples.join("tls/tls"), &directory.path().join("tls/tls"));
    let compiler = directory
        .path()
        .join(format!("distributed-zore{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_zore"), &compiler).unwrap();
    let command = |action: &str, target: &Path| {
        let mut command = Command::new(&compiler);
        command
            .arg(action)
            .arg("--offline")
            .arg(target)
            .current_dir(directory.path())
            .env("ZORE_CACHE_DIR", directory.path().join("cache"))
            .env("ZORE_CHECK_LEAKS", "1");
        command
    };
    success(command("install", &app));
    let mut check = command("check", &app.join("main.ore"));
    check
        .arg("--locked")
        .env("ZORE_CC", "missing-native-compiler")
        .env("ZORE_RUSTC", "missing-rust-compiler");
    success(check);
    let mut tests = command("test", &app);
    tests.arg("--locked");
    let output = success(tests);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Tests: 5 passed, 0 failed"),
        "{output:?}"
    );
}
