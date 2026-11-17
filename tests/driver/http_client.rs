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
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let app = directory.path().join("examples/https-client");
    copy_tree(&repository.join("examples/https-client"), &app);
    copy_tree(
        &repository.join("packages/tls"),
        &directory.path().join("packages/tls"),
    );
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
    tests.arg("--locked").env_remove("SSL_CERT_FILE");
    let output = success(tests);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Tests: 13 passed, 0 failed"),
        "{output:?}"
    );
}

struct NodeServer {
    child: std::process::Child,
    port: String,
}

impl Drop for NodeServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn node_server(script: &Path, tls: &Path, mode: &str) -> NodeServer {
    let mut child = Command::new("node")
        .arg(script)
        .arg(tls.join("testdata/server.pem"))
        .arg(tls.join("testdata/server-key.pem"))
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| {
            panic!("the HTTP/2 interoperability test needs Node.js (`node`) on PATH: {e}")
        });
    let mut stdout = child.stdout.take().unwrap();
    let mut port = String::new();
    let mut byte = [0];
    while std::io::Read::read(&mut stdout, &mut byte).unwrap() == 1 && byte[0] != b'\n' {
        port.push(byte[0] as char);
    }
    assert!(
        !port.trim().is_empty(),
        "Node.js server did not report a port"
    );
    NodeServer {
        child,
        port: port.trim().to_owned(),
    }
}

#[test]
fn http2_auto_client_interoperates_with_node_and_falls_back_to_http1() {
    let directory = TempDir::new().unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let app = directory.path().join("examples/http2-interop");
    copy_tree(
        &repository.join("tests/driver/fixtures/http2-interop"),
        &app,
    );
    let tls = directory.path().join("packages/tls");
    copy_tree(&repository.join("packages/tls"), &tls);
    let compiler = directory
        .path()
        .join(format!("distributed-zore{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_zore"), &compiler).unwrap();
    let zore = |action: &str| {
        let mut command = Command::new(&compiler);
        command
            .arg(action)
            .arg("--offline")
            .current_dir(&app)
            .env("ZORE_CACHE_DIR", directory.path().join("cache"));
        command
    };
    let mut install = zore("install");
    install.arg(".");
    success(install);
    let mut build = zore("build");
    build.arg("main.ore");
    success(build);
    let program = app.join(format!("main{}", std::env::consts::EXE_SUFFIX));
    let script = app.join("server.js");
    let roots = tls.join("testdata/roots.pem");
    for (mode, expected) in [
        (
            "h2",
            "concurrent 01234\npeak 200 5\necho 200 2.0 POST 150000\nbig 200 300000 bytes\n\
             header 200 abc\nmissing 404 missing\nopen 200 300000 bytes in several pieces\nsessions 200 1\nconnections 1\n",
        ),
        (
            "http1",
            "echo 200 1.1 POST 0\nopen 200 300000 bytes in several pieces\nconnections 0\n",
        ),
    ] {
        let server = node_server(&script, &tls, mode);
        let mut run = Command::new(&program);
        run.arg(&server.port)
            .arg(&roots)
            .arg(mode)
            .env("ZORE_CHECK_LEAKS", "1");
        let output = success(run);
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{mode}");
        drop(server);
    }
}
