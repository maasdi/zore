use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use zore::build::TempDir;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn git_command(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
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
        .unwrap()
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = git_command(root, args);
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn failure(output: Output, message: &str) -> String {
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains(message), "{error}");
    error
}

fn url(path: &Path) -> String {
    let path = path.to_str().unwrap().replace('\\', "/");
    format!(
        "file://{}{path}",
        if path.starts_with('/') { "" } else { "/" }
    )
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if path.is_dir() {
            files.insert(path.strip_prefix(root).unwrap().into(), Vec::new());
            for entry in fs::read_dir(path).unwrap() {
                walk(root, &entry.unwrap().path(), files);
            }
        } else {
            files.insert(
                path.strip_prefix(root).unwrap().into(),
                fs::read(path).unwrap(),
            );
        }
    }
    let mut files = BTreeMap::new();
    if root.exists() {
        walk(root, root, &mut files);
    }
    files
}

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
    app: PathBuf,
    cache: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("Release workflow café with spaces");
        let app = root.join("app");
        write(&app, "main.ore", "package main\nfunc main() {}\n");
        Self {
            _temporary: temporary,
            cache: root.join("cache"),
            root,
            app,
        }
    }

    fn author(&self, name: &str) -> PathBuf {
        self.root.join(format!("author-{name}"))
    }
    fn remote(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.git"))
    }
    fn requirement(&self, name: &str, version: &str) -> String {
        format!(
            "{name} = {{ git = '{}', version = '{version}' }}\n",
            url(&self.remote(name))
        )
    }
    fn app(&self, dependencies: &str) {
        write(
            &self.app,
            "zore.toml",
            &format!("name = 'app'\n[dependencies]\n{dependencies}"),
        );
    }
    fn command(&self, action: &str, flags: &[&str], target: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zore"));
        command
            .arg(action)
            .args(flags)
            .arg(target)
            .env("ZORE_CACHE_DIR", &self.cache)
            .env("ZORE_CHECK_LEAKS", "1")
            .current_dir(&self.root);
        command
    }
    fn invoke(&self, action: &str, flags: &[&str]) -> Output {
        self.command(action, flags, &self.app).output().unwrap()
    }
    fn release(
        &self,
        name: &str,
        version: &str,
        dependencies: &str,
        source: &str,
        preflight: bool,
    ) -> String {
        let author = self.author(name);
        if !author.exists() {
            fs::create_dir_all(&author).unwrap();
            git(
                &self.root,
                &[
                    "init",
                    "--bare",
                    "--initial-branch=main",
                    self.remote(name).to_str().unwrap(),
                ],
            );
            git(&author, &["init", "-q", "--initial-branch=main"]);
            git(
                &author,
                &["remote", "add", "origin", &url(&self.remote(name))],
            );
        }
        write(
            &author,
            "zore.toml",
            &format!(
                "name = '{name}'\nversion = '{version}'\nlicense = 'MIT'\n[dependencies]\n{dependencies}"
            ),
        );
        write(&author, &format!("{name}.ore"), source);
        write(
            &author,
            &format!("{name}_test.ore"),
            &format!("package {name}\nfunc TestSmoke() {{}}\n"),
        );
        let tag = format!("{name}/v{version}");
        if preflight {
            let action = if author.join("zore.lock").exists() {
                "lock"
            } else {
                "install"
            };
            success(self.command(action, &[], &author).output().unwrap());
            success(
                self.command("check", &["--release", "--tag", &tag], &author)
                    .output()
                    .unwrap(),
            );
            assert!(
                success(
                    self.command("test", &["--offline"], &author)
                        .output()
                        .unwrap()
                )
                .contains("Tests: 1 passed")
            );
        }
        git(&author, &["add", "."]);
        git(&author, &["commit", "-qm", &format!("Release {version}")]);
        git(&author, &["tag", "-a", &tag, "-m", &tag]);
        git(&author, &["push", "origin", &format!("refs/tags/{tag}")]);
        git(&author, &["rev-parse", "HEAD"])
    }
    fn leaf(&self, version: &str) -> String {
        self.release(
            "shared",
            version,
            "",
            "package shared\nfunc Value() int { return 42 }\n",
            false,
        )
    }
    fn lock(&self) -> String {
        fs::read_to_string(self.app.join("zore.lock")).unwrap()
    }
    fn readonly_report(&self, flags: &[&str]) -> String {
        let app = snapshot(&self.app);
        let cache = snapshot(&self.cache);
        let output = self
            .command("outdated", flags, &self.app)
            .env("ZORE_CC", "missing-clang")
            .env("RUSTC", "missing-rustc")
            .output()
            .unwrap();
        let report = success(output);
        assert_eq!(snapshot(&self.app), app);
        assert_eq!(snapshot(&self.cache), cache);
        report
    }
}

#[test]
fn online_discovery_filters_tags_orders_numeric_lines_and_never_writes_or_fetches() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    fixture.app(&fixture.requirement("shared", "1.2.3"));
    success(fixture.invoke("install", &[]));
    let author = fixture.author("shared");
    for tag in [
        "shared/v1.9.0",
        "shared/v1.10.0",
        "shared/v2.1.0",
        "shared/v2.3.0",
        "shared/v10.0.0",
        "shared/v99999999999999999999999999999.0.0",
        "shared/v1.010.0",
        "shared/v1.99.0-beta",
        "shared/v1.99.0+build",
        "shared/V1.99.0",
        "other/v99.0.0",
        "shared/v1.99",
    ] {
        git(&author, &["tag", tag]);
        git(&author, &["push", "origin", &format!("refs/tags/{tag}")]);
    }
    let report = fixture.readonly_report(&[]);
    assert!(report.contains("source metadata is unverified"), "{report}");
    assert!(
        report
            .contains("shared\t1.2.3\t1.10.0\t2.3.0, 10.0.0, 99999999999999999999999999999.0.0\n"),
        "{report}"
    );
    assert!(!report.contains("1.99"), "{report}");
    fixture.app(&fixture.requirement("shared", "1.10.0"));
    failure(fixture.invoke("lock", &[]), "does not match");
    assert!(fixture.lock().contains("version = \"1.2.3\""));
}

#[test]
fn pre_one_lines_and_noncommit_advertisements_require_explicit_validation() {
    let fixture = Fixture::new();
    fixture.leaf("0.3.1");
    fixture.app(&fixture.requirement("shared", "0.3.1"));
    success(fixture.invoke("install", &[]));
    let author = fixture.author("shared");
    for tag in [
        "shared/v0.2.99",
        "shared/v0.3.2",
        "shared/v0.4.1",
        "shared/v0.4.9",
        "shared/v0.10.0",
        "shared/v1.0.0",
    ] {
        git(&author, &["tag", tag]);
    }
    let blob = git(&author, &["rev-parse", "HEAD:zore.toml"]);
    git(&author, &["tag", "shared/v0.3.99", &blob]);
    git(&author, &["push", "origin", "--tags"]);
    let report = fixture.readonly_report(&[]);
    assert!(
        report.contains("shared\t0.3.1\t0.3.99\t0.4.9, 0.10.0, 1.0.0\n"),
        "{report}"
    );
    fixture.app(&fixture.requirement("shared", "0.3.99"));
    failure(fixture.invoke("lock", &[]), "must resolve to a commit");
}

#[test]
fn offline_reports_use_lock_history_and_shared_bindings_without_network_or_writes() {
    let fixture = Fixture::new();
    for version in ["1.2.3", "1.4.0", "2.0.0"] {
        fixture.leaf(version);
    }
    fixture.app(&fixture.requirement("shared", "1.2.3"));
    success(fixture.invoke("install", &[]));
    let other = fixture.root.join("other-app");
    for version in ["1.4.0", "2.0.0"] {
        write(
            &other,
            "zore.toml",
            &format!(
                "name = 'other'\n[dependencies]\n{}",
                fixture.requirement("shared", version)
            ),
        );
        success(fixture.command("lock", &[], &other).output().unwrap());
    }
    fs::rename(
        fixture.remote("shared"),
        fixture.root.join("unavailable.git"),
    )
    .unwrap();
    let report = fixture.readonly_report(&["--offline"]);
    assert!(
        report.starts_with("Stale and incomplete cached releases"),
        "{report}"
    );
    assert!(report.contains("shared\t1.2.3\t1.4.0\t2.0.0\n"), "{report}");
    failure(fixture.invoke("outdated", &[]), "Git tag listing failed");
    fs::remove_dir_all(fixture.cache.join("releases")).unwrap();
    let report = fixture.readonly_report(&["--offline"]);
    assert!(report.contains("shared\t1.2.3\t1.2.3\t-\n"), "{report}");
    assert!(!fixture.cache.join("releases").exists());
}

#[test]
fn discovery_requires_matching_locks_and_complete_caches_and_rejects_usage_errors() {
    let fixture = Fixture::new();
    fixture.leaf("1.0.0");
    fixture.app(&fixture.requirement("shared", "1.0.0"));
    failure(
        fixture.invoke("outdated", &[]),
        "requires a matching zore.lock",
    );
    assert!(!fixture.cache.exists());
    success(fixture.invoke("install", &[]));
    for flags in [
        vec!["--locked"],
        vec!["--offline", "--offline"],
        vec!["--release"],
        vec!["--tag", "shared/v1.0.0"],
    ] {
        let output = fixture.invoke("outdated", &flags);
        assert_eq!(output.status.code(), Some(2));
        failure(output, "unknown option");
    }
    fixture.app(&fixture.requirement("shared", "1.0.1"));
    failure(
        fixture.invoke("outdated", &[]),
        "mismatched release provenance",
    );
    fixture.app(&fixture.requirement("shared", "1.0.0"));
    fs::rename(&fixture.cache, fixture.root.join("unavailable-cache")).unwrap();
    failure(fixture.invoke("outdated", &[]), "missing from the cache");
    assert!(!fixture.cache.exists());
}

#[test]
fn removed_tags_do_not_revoke_locks_and_absent_or_older_advertisements_are_reported() {
    let fixture = Fixture::new();
    fixture.leaf("1.0.0");
    fixture.leaf("1.2.0");
    fixture.app(&fixture.requirement("shared", "1.2.0"));
    success(fixture.invoke("install", &[]));
    let remote = fixture.remote("shared");
    git(&remote, &["tag", "-d", "shared/v1.2.0"]);
    let report = fixture.readonly_report(&[]);
    assert!(report.contains("shared\t1.2.0\t1.0.0\t-\n"), "{report}");
    git(&remote, &["tag", "-d", "shared/v1.0.0"]);
    assert!(
        fixture
            .readonly_report(&[])
            .contains("shared\t1.2.0\t-\t-\n")
    );
    success(fixture.invoke("install", &["--locked", "--offline"]));
}

#[test]
fn local_and_revision_pins_have_no_inferred_release_line() {
    let fixture = Fixture::new();
    let revision = fixture.leaf("1.0.0");
    fixture.app(&format!(
        "shared = {{ git = '{}', rev = '{revision}' }}\n",
        url(&fixture.remote("shared"))
    ));
    success(fixture.invoke("install", &[]));
    assert_eq!(
        fixture.readonly_report(&[]),
        "No tagged dependencies are selected.\n"
    );
    fixture.app("");
    success(fixture.invoke("lock", &[]));
    assert_eq!(
        fixture.readonly_report(&["--offline"]),
        "No tagged dependencies are selected.\n"
    );
}

#[test]
fn corrupt_binding_indexes_fail_without_repair_or_partial_output() {
    let fixture = Fixture::new();
    fixture.leaf("1.0.0");
    fixture.app(&fixture.requirement("shared", "1.0.0"));
    success(fixture.invoke("install", &[]));
    write(
        &fixture.cache,
        "releases/not-an-identity/binding",
        "not a binding\n",
    );
    let before = snapshot(&fixture.cache);
    failure(
        fixture.invoke("outdated", &["--offline"]),
        "corrupt release binding cache",
    );
    assert_eq!(before, snapshot(&fixture.cache));
    success(fixture.invoke("outdated", &[]));
}

#[test]
fn ordinary_tag_pushes_reject_replacement_allow_retry_and_preserve_known_identities() {
    let fixture = Fixture::new();
    let first = fixture.leaf("1.0.0");
    fixture.app(&fixture.requirement("shared", "1.0.0"));
    success(fixture.invoke("install", &[]));
    let author = fixture.author("shared");
    git(&author, &["push", "origin", "refs/tags/shared/v1.0.0"]);
    write(
        &author,
        "shared.ore",
        "package shared\nfunc Value() int { return 43 }\n",
    );
    git(&author, &["commit", "-qam", "Correction"]);
    git(&author, &["tag", "-f", "shared/v1.0.0"]);
    assert!(
        !git_command(&author, &["push", "origin", "refs/tags/shared/v1.0.0"])
            .status
            .success()
    );
    assert_eq!(
        git(
            &fixture.remote("shared"),
            &["rev-parse", "refs/tags/shared/v1.0.0^{commit}"]
        ),
        first
    );
    git(
        &author,
        &["push", "--force", "origin", "refs/tags/shared/v1.0.0"],
    );
    let lock = fixture.lock();
    failure(fixture.invoke("lock", &[]), "moved from its known commit");
    assert_eq!(fixture.lock(), lock);
    success(fixture.invoke("install", &["--locked", "--offline"]));
    fixture.leaf("1.0.1");
    fixture.app(&fixture.requirement("shared", "1.0.1"));
    success(fixture.invoke("lock", &[]));
    assert!(fixture.lock().contains("version = \"1.0.1\""));
}

#[test]
fn ordinary_author_workflow_preserves_diamond_type_identity_and_all_cleanup_exits_offline() {
    let fixture = Fixture::new();
    let shared = r#"package shared
let Seed = initialize()
func initialize() int { println("init:shared"); return 42 }
type Resource struct { Name string }
func (r mut Resource) drop() { println("drop:" + r.Name) }
func New(name string) Resource { return Resource{Name: name} }
func Identity<T any>(value own T) T { return value }
"#;
    for version in ["1.2.3", "1.2.4"] {
        fixture.release("shared", version, "", shared, true);
    }
    fixture.release(
        "left",
        "1.0.0",
        &fixture.requirement("shared", "1.2.3"),
        r#"package left
import "shared"
func Seed() int { return shared.Seed }
func Make(name string) shared.Resource { return shared.Identity(shared.New(name)) }
"#,
        true,
    );
    fixture.release("right", "1.0.0", &fixture.requirement("shared", "1.2.4"), r#"package right
import "shared"
import "std/time"
func Seed() int { return shared.Seed }
func check(mode string) error { if mode == "error" { return error("expected failure") }; return nil }
async func Consume(resource own shared.Resource, mode string) error {
    time.Sleep(time.Millisecond)
    if mode == "panic" { panic("dependency panic") }
    check(mode)?
    println("handled:" + resource.Name)
    return nil
}
"#, true);
    fixture.app(&(fixture.requirement("left", "1.0.0") + &fixture.requirement("right", "1.0.0")));
    write(
        &fixture.app,
        "main.ore",
        r#"package main
import "left"
import "right"
import "std/os"
func main() {
    println(left.Seed())
    println(right.Seed())
    let args = os.Args()
    let parent = left.Make("parent")
    let resource = left.Make("child")
    let task = go right.Consume(resource, args[1])
    let err = task.wait()
    if err != nil { println("returned:error") }
    println("done")
}
"#,
    );
    success(fixture.invoke("install", &[]));
    assert!(
        fixture
            .lock()
            .lines()
            .find(|line| line.starts_with("shared = "))
            .unwrap()
            .contains("version = \"1.2.4\"")
    );
    let report = fixture.readonly_report(&[]);
    assert!(report.contains("shared\t1.2.4\t1.2.4\t-\n"), "{report}");
    for name in ["left", "right", "shared"] {
        fs::rename(
            fixture.remote(name),
            fixture.root.join(format!("unavailable-{name}.git")),
        )
        .unwrap();
    }
    let lock = fixture.lock();
    let source = fixture.app.join("main.ore");
    success(
        fixture
            .command("check", &["--locked", "--offline"], &source)
            .output()
            .unwrap(),
    );
    success(
        fixture
            .command("build", &["--locked", "--offline"], &source)
            .output()
            .unwrap(),
    );
    let executable = fixture
        .root
        .join(format!("main{}", std::env::consts::EXE_SUFFIX));
    for (mode, expected, code) in [
        (
            "normal",
            "init:shared\n42\n42\nhandled:child\ndrop:child\ndone\ndrop:parent\n",
            0,
        ),
        (
            "error",
            "init:shared\n42\n42\ndrop:child\nreturned:error\ndone\ndrop:parent\n",
            0,
        ),
        ("panic", "init:shared\n42\n42\ndrop:child\ndrop:parent\n", 2),
    ] {
        let output = Command::new(&executable)
            .arg(mode)
            .env("ZORE_CHECK_LEAKS", "1")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        if mode == "panic" {
            assert!(String::from_utf8_lossy(&output.stderr).contains("dependency panic"));
        } else {
            assert!(output.stderr.is_empty(), "{output:?}");
        }
    }
    write(
        &fixture.app,
        "main_test.ore",
        "package main\nimport \"left\"\nimport \"right\"\nfunc TestIdentity() { if left.Seed() != right.Seed() { panic(\"identity\") } }\n",
    );
    assert!(
        success(fixture.invoke("test", &["--locked", "--offline"])).contains("Tests: 1 passed")
    );
    fixture.readonly_report(&["--offline"]);
    let main = fs::read_to_string(&source)
        .unwrap()
        .replace("args[1]", "\"normal\"");
    fs::write(&source, main).unwrap();
    let output = success(
        fixture
            .command("run", &["--locked", "--offline"], &source)
            .output()
            .unwrap(),
    );
    assert_eq!(
        output,
        "init:shared\n42\n42\nhandled:child\ndrop:child\ndone\ndrop:parent\n"
    );
    assert_eq!(fixture.lock(), lock);
}

#[test]
fn reported_transitive_updates_require_a_direct_minimum_and_explicit_lock() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    fixture.leaf("1.4.0");
    fixture.release(
        "left",
        "1.0.0",
        &fixture.requirement("shared", "1.2.3"),
        "package left\nimport \"shared\"\nfunc Value() int { return shared.Value() }\n",
        false,
    );
    let left = fixture.requirement("left", "1.0.0");
    fixture.app(&left);
    success(fixture.invoke("install", &[]));
    let lock = fixture.lock();
    assert!(
        fixture
            .readonly_report(&[])
            .contains("shared\t1.2.3\t1.4.0\t-\n")
    );
    assert_eq!(fixture.lock(), lock);
    fixture.app(&(left + &fixture.requirement("shared", "1.4.0")));
    failure(
        fixture.invoke("install", &[]),
        "mismatched release provenance",
    );
    assert_eq!(fixture.lock(), lock);
    success(fixture.invoke("lock", &[]));
    assert!(
        fixture
            .lock()
            .lines()
            .find(|line| line.starts_with("shared = "))
            .unwrap()
            .contains("version = \"1.4.0\"")
    );
    assert!(
        fixture
            .readonly_report(&[])
            .contains("shared\t1.4.0\t1.4.0\t-\n")
    );
    success(
        fixture
            .command(
                "check",
                &["--locked", "--offline"],
                &fixture.app.join("main.ore"),
            )
            .output()
            .unwrap(),
    );
}

#[test]
#[ignore = "invoked only as a Git SSH subprocess fixture"]
fn ssh_failure_fixture() {
    let marker = std::env::var_os("ZORE_TEST_SSH_FAILURE").expect("SSH fixture marker");
    fs::write(marker, "ran").unwrap();
    eprintln!("TOP_SECRET");
    std::process::exit(1);
}

#[test]
fn git_listing_failures_redact_remote_diagnostics_and_embedded_credentials() {
    let fixture = Fixture::new();
    fixture.leaf("1.0.0");
    fixture.app(&fixture.requirement("shared", "1.0.0"));
    success(fixture.invoke("install", &[]));
    let marker = fixture.root.join("ssh-ran");
    let executable = std::env::current_exe().unwrap();
    let output = fixture
        .command("outdated", &[], &fixture.app)
        .env("GIT_CONFIG_COUNT", "1")
        .env(
            "GIT_CONFIG_KEY_0",
            "url.ssh://git@example.invalid/library.insteadOf",
        )
        .env("GIT_CONFIG_VALUE_0", url(&fixture.remote("shared")))
        .env(
            "GIT_SSH_COMMAND",
            format!(
                "\"{}\" --ignored --exact ssh_failure_fixture --nocapture --",
                executable.to_str().unwrap().replace('\\', "/")
            ),
        )
        .env("ZORE_TEST_SSH_FAILURE", &marker)
        .env("GIT_SSH_VARIANT", "ssh")
        .output()
        .unwrap();
    let error = failure(output, "Git tag listing failed");
    assert!(marker.exists(), "SSH failure fixture was not invoked");
    assert!(!error.contains("TOP_SECRET"), "{error}");
    fixture.app(
        "shared = { git = 'https://token:TOP_SECRET@example.invalid/repo', version = '1.0.0' }\n",
    );
    let error = failure(
        fixture.invoke("outdated", &[]),
        "without embedded credentials",
    );
    assert!(!error.contains("TOP_SECRET"), "{error}");
}
