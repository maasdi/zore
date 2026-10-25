use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use zore::build::TempDir;

const SUBPROCESS_LIMIT: Duration = Duration::from_secs(180);

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let to = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}

fn run(mut command: Command) -> Output {
    let stdout = TempDir::new().unwrap();
    let output_path = stdout.path().join("output");
    let error_path = stdout.path().join("error");
    let mut child = command
        .stdout(Stdio::from(fs::File::create(&output_path).unwrap()))
        .stderr(Stdio::from(fs::File::create(&error_path).unwrap()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + SUBPROCESS_LIMIT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "TLS subprocess exceeded {} seconds: stdout={} stderr={}",
                SUBPROCESS_LIMIT.as_secs(),
                String::from_utf8_lossy(&fs::read(&output_path).unwrap()),
                String::from_utf8_lossy(&fs::read(&error_path).unwrap())
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: fs::read(output_path).unwrap(),
        stderr: fs::read(error_path).unwrap(),
    }
}

fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    output
}

struct Fixture {
    _directory: TempDir,
    root: PathBuf,
    compiler: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let root = directory.path().join("TLS offline café");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/tls/tls"),
            &root,
        );
        let compiler = directory
            .path()
            .join(format!("distributed-zore{}", std::env::consts::EXE_SUFFIX));
        fs::copy(env!("CARGO_BIN_EXE_zore"), &compiler).unwrap();
        Self {
            _directory: directory,
            root,
            compiler,
        }
    }
    fn command(&self, action: &str) -> Command {
        self.target(action, &self.root)
    }
    fn target(&self, action: &str, target: &Path) -> Command {
        let mut command = Command::new(&self.compiler);
        command
            .arg(action)
            .arg("--offline")
            .arg(target)
            .current_dir(self._directory.path())
            .env("ZORE_CACHE_DIR", self._directory.path().join("cache"))
            .env("ZORE_CHECK_LEAKS", "1");
        command
    }
}

#[test]
fn offline_distributed_compiler_runs_verified_tls_conformance() {
    let fixture = Fixture::new();
    success(run(fixture.command("install")));
    let mut check = Command::new(&fixture.compiler);
    check
        .arg("check")
        .arg("--offline")
        .arg(fixture.root.join("tls.ore"));
    check
        .arg("--locked")
        .env("ZORE_CC", "missing-native-compiler")
        .env("ZORE_RUSTC", "missing-rust-compiler");
    success(run(check));
    let app = fixture._directory.path().join("consumer");
    fs::create_dir_all(&app).unwrap();
    fs::write(
        app.join("zore.toml"),
        "name = \"consumer\"\n[dependencies]\ntls = { path = \"../TLS offline café\" }\n",
    )
    .unwrap();
    fs::write(
        app.join("main.ore"),
        r#"package main
import "tls"
func consume(config own tls.ClientConfig) {}
func invalid(config own tls.ClientConfig) { consume(config); consume(config) }
func main() {}
"#,
    )
    .unwrap();
    success(run(fixture.target("install", &app)));
    let output = run(fixture.target("check", &app.join("main.ore")));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("moved"),
        "{output:?}"
    );
    for example in [
        include_str!("../../examples/tls/client/main.ore"),
        include_str!("../../examples/tls/server/main.ore"),
    ] {
        fs::write(app.join("main.ore"), example).unwrap();
        success(run(fixture.target("check", &app.join("main.ore"))));
    }
    let mut tests = fixture.command("test");
    tests.arg("--locked").env_remove("SSL_CERT_FILE");
    let output = success(run(tests));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Tests: 28 passed, 0 failed"));
    let cached = fs::read_dir(fixture._directory.path().join("cache/zore"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("mbedtls-")
        })
        .collect::<Vec<_>>();
    assert_eq!(cached.len(), 1, "{cached:?}");
    assert!(cached[0].join("library/ssl_tls.o").is_file(), "{cached:?}");
    assert!(
        cached[0].join("library/ssl_tls13_client.o").is_file(),
        "{cached:?}"
    );
    let environment_test = fixture.root.join("system_roots_env_test.ore");
    fs::write(
        &environment_test,
        r#"package tls
import "std/os"
func TestSystemRootsHonorsCertificateFileVariable() error {
    let roots = SystemRoots()?
    let expected = nativeSystemRoots("testdata/roots.pem")?
    check(roots.len() > 0 && roots.len() == expected.len())
    let config = DefaultClientConfig()?
    let cert = os.ReadFile("testdata/server.pem")?
    let key = os.ReadFile("testdata/server-key.pem")?
    let serverConfig = NewServerConfig(cert[:], key[:])?
    let client = ClientConfig{handle: nativeSession(config.handle, "localhost")?}
    let server = ClientConfig{handle: nativeSession(serverConfig.handle, "")?}
    return memoryHandshake(client.handle, server.handle)
}
"#,
    )
    .unwrap();
    let mut environment = fixture.target("test", &environment_test);
    environment
        .arg("--locked")
        .env("SSL_CERT_FILE", "testdata/roots.pem");
    success(run(environment));
    let panic_test = fixture.root.join("panic_test.ore");
    fs::write(
        &panic_test,
        r#"package tls
func TestPanicCleanup() error {
    let config, serverConfig = configs()?
    let session = ClientConfig{handle: nativeSession(config.handle, "localhost")?}
    panic("TLS cleanup panic")
}
"#,
    )
    .unwrap();
    let output = run(fixture.target("test", &panic_test));
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("TLS cleanup panic"), "{output:?}");
    assert!(!diagnostic.contains("leak:"), "{output:?}");
    let path = fixture.root.join("native/tls.c");
    let contents = fs::read_to_string(&path).unwrap();
    fs::write(path, contents + "\n").unwrap();
    let output = run(fixture.command("install"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("reviewed adapter"));
}
