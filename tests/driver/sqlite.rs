use super::{failure, git, success, write};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use zore::build::TempDir;

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("SQLite projects café with spaces");
        for (path, text) in [
            (
                "app/zore.toml",
                include_str!("../../examples/sqlite-library/app/zore.toml"),
            ),
            (
                "app/main.ore",
                include_str!("../../examples/sqlite-library/app/main.ore"),
            ),
            (
                "sqlite/zore.toml",
                include_str!("../../examples/sqlite-library/sqlite/zore.toml"),
            ),
            (
                "sqlite/sqlite.ore",
                include_str!("../../examples/sqlite-library/sqlite/sqlite.ore"),
            ),
            (
                "sqlite/sqlite_test.ore",
                include_str!("../../examples/sqlite-library/sqlite/sqlite_test.ore"),
            ),
            (
                "sqlite/native/bindings.abi",
                include_str!("../../examples/sqlite-library/sqlite/native/bindings.abi"),
            ),
            (
                "sqlite/native/sqlite.c",
                include_str!("../../examples/sqlite-library/sqlite/native/sqlite.c"),
            ),
            (
                "sqlite/native/sqlite.h",
                include_str!("../../examples/sqlite-library/sqlite/native/sqlite.h"),
            ),
            (
                "sqlite/native/bridge.rs",
                include_str!("../../examples/sqlite-library/sqlite/native/bridge.rs"),
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
        command.arg(action);
        if action != "install" {
            command.arg("--locked");
        }
        command
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
fn sqlite_resource_contract_and_tagged_offline_application() {
    let fixture = Fixture::new();
    let library = fixture.root.join("sqlite");
    git(&library, &["init", "-q"]);
    git(&library, &["add", "."]);
    git(&library, &["commit", "-qm", "reviewed SQLite fixture"]);
    git(&library, &["tag", "sqlite/v0.1.0"]);
    let revision = git(&library, &["rev-parse", "HEAD"]);
    let path = library.to_str().unwrap().replace('\\', "/");
    let url = format!(
        "file://{}{path}",
        if path.starts_with('/') { "" } else { "/" }
    );
    write(
        &fixture.root,
        "app/zore.toml",
        &format!(
            "name = 'notes'\n[dependencies]\nsqlite = {{ git = '{url}', version = '0.1.0' }}\n"
        ),
    );
    write(
        &fixture.root,
        "app/main_test.ore",
        "package main\nfunc TestPersistence() error { return persist() }\n",
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
    assert!(lock.contains(&revision));
    assert!(lock.contains("sqlite-v1"));
    fs::rename(&library, fixture.root.join("unavailable")).unwrap();
    success(
        fixture
            .command("install", "app")
            .arg("--locked")
            .output()
            .unwrap(),
    );
    success(
        fixture
            .command("check", "app/main.ore")
            .env("ZORE_CC", "missing-c-compiler")
            .env("ZORE_RUSTC", "missing-rust-compiler")
            .output()
            .unwrap(),
    );
    success(fixture.invoke("build", "app/main.ore"));
    assert_eq!(
        success(
            Command::new(
                fixture
                    .root
                    .join(format!("main{}", std::env::consts::EXE_SUFFIX))
            )
            .current_dir(&fixture.root)
            .env("ZORE_CHECK_LEAKS", "1")
            .output()
            .unwrap()
        )
        .stdout,
        b"hello from SQLite\n"
    );
    assert!(
        String::from_utf8_lossy(&success(fixture.invoke("test", "app")).stdout)
            .contains("Tests: 1 passed, 0 failed")
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("app/zore.lock")).unwrap(),
        lock
    );
    fs::rename(fixture.root.join("unavailable"), &library).unwrap();
    success(fixture.invoke("install", "sqlite"));
    let tests = success(fixture.invoke("test", "sqlite"));
    assert!(
        String::from_utf8_lossy(&tests.stdout).contains("Tests: 23 passed, 0 failed"),
        "{tests:?}"
    );
    cleanup_modes(&fixture);
    let main = fs::read_to_string(fixture.root.join("app/main.ore")).unwrap();
    write(
        &fixture.root,
        "app/main.ore",
        "package main\nimport \"sqlite\"\nfunc bad() error { let db = sqlite.Open(\":memory:\")?; let tx = db.Begin()?; return nil }; func main() {}\n",
    );
    failure(fixture.invoke("check", "app/main.ore"), "immutable binding");
    write(&fixture.root, "app/main.ore", &main);
    let source = library.join("native/bridge.rs");
    let original = fs::read_to_string(&source).unwrap();
    fs::write(&source, format!("{original}\n")).unwrap();
    failure(
        fixture.invoke("install", "sqlite"),
        "native sources do not match reviewed adapter",
    );
}

fn cleanup_modes(fixture: &Fixture) {
    write(
        &fixture.root,
        "app/main.ore",
        r#"package main
import "sqlite"
import "std/errors"
import "std/os"
import "std/time"
func runSQL(db sqlite.Database, sql string) error {
    var statement = db.Prepare(sql)?
    statement.Step()?
    return nil
}
async func transaction(db own sqlite.Database, mode string) error {
    var source = db
    var tx = source.Begin()?
    var statement = tx.Prepare("insert into cleanup values (1)")?
    statement.Step()?
    time.Sleep(time.Millisecond)
    if mode == "error" { return error("expected transaction error") }
    if mode == "panic" { panic("expected SQLite task panic") }
    statement.Close()?
    let finished = tx.Commit()?
    return nil
}
func callbackPanic() error {
    let db = sqlite.Open("cleanup.db")?
    db.Transaction(func(tx mut sqlite.Transaction) error {
        var statement = tx.Prepare("delete from cleanup")?
        statement.Step()?
        panic("expected SQLite callback panic")
    })?
    return nil
}
func panickingChild(db sqlite.Database, ready channel<bool>) error {
    return db.Transaction(func(tx mut sqlite.Transaction) error {
        ready.send(true)
        time.Sleep(300 * time.Millisecond)
        panic("expected scoped callback panic")
    })
}
func contendingChild(db sqlite.Database, ready channel<bool>) error {
    ready.receive()
    var failures = 0
    for var attempt = 0; attempt < 100; attempt += 1 {
        let result = db.Transaction(func(tx mut sqlite.Transaction) error { return nil })
        if result == nil {
            if failures == 0 { return error("peer began while the panicking transaction was open") }
            return nil
        }
        failures += 1
        time.Sleep(20 * time.Millisecond)
    }
    return error("peer never began after the panic")
}
func scopedPanic() error {
    let db = sqlite.Open(":memory:")?
    let ready = channel<bool>(1)
    scope {
        let panicking = go panickingChild(db, ready)
        let peer = go contendingChild(db, ready)
        let failure = peer.wait()
        if failure != nil { panic("peer failed: " + errors.Message(failure)) }
        println("peer ok")
    }
    return nil
}
func execute(mode string) error {
    if mode == "callback-panic" { return callbackPanic() }
    if mode == "scoped-panic" { return scopedPanic() }
    if mode != "inspect" {
        let db = sqlite.Open("cleanup.db")?
        runSQL(db, "create table if not exists cleanup(value integer)")?
        runSQL(db, "delete from cleanup")?
        let task = go transaction(db, mode)
        let failure = task.wait()
        if failure != nil && mode != "error" { return failure }
        if failure == nil && mode == "error" { panic("missing expected error") }
    }
    let db = sqlite.Open("cleanup.db")?
    var query = db.Prepare("select count(*) from cleanup")?
    query.Step()?
    println(query.Int(0)?)
    return nil
}
func main() {
    let args = os.Args()
    let failure = execute(args[1])
    if failure != nil { panic("SQLite cleanup failure") }
}
"#,
    );
    success(fixture.invoke("build", "app/main.ore"));
    let executable = fixture
        .root
        .join(format!("main{}", std::env::consts::EXE_SUFFIX));
    let run = |mode: &str| {
        Command::new(&executable)
            .arg(mode)
            .current_dir(&fixture.root)
            .env("ZORE_CHECK_LEAKS", "1")
            .output()
            .unwrap()
    };
    assert_eq!(success(run("normal")).stdout, b"1\n");
    assert_eq!(success(run("error")).stdout, b"0\n");
    let panic = run("panic");
    assert_eq!(panic.status.code(), Some(2), "{panic:?}");
    let stderr = String::from_utf8_lossy(&panic.stderr);
    assert!(stderr.contains("expected SQLite task panic"), "{panic:?}");
    assert!(!stderr.contains("leak:"), "{panic:?}");
    assert_eq!(success(run("inspect")).stdout, b"0\n");
    assert_eq!(success(run("normal")).stdout, b"1\n");
    let callback_panic = run("callback-panic");
    assert_eq!(callback_panic.status.code(), Some(2), "{callback_panic:?}");
    let stderr = String::from_utf8_lossy(&callback_panic.stderr);
    assert!(
        stderr.contains("expected SQLite callback panic"),
        "{callback_panic:?}"
    );
    assert!(!stderr.contains("leak:"), "{callback_panic:?}");
    assert_eq!(success(run("inspect")).stdout, b"1\n");
    let scoped_panic = run("scoped-panic");
    assert_eq!(scoped_panic.status.code(), Some(2), "{scoped_panic:?}");
    assert_eq!(scoped_panic.stdout, b"peer ok\n", "{scoped_panic:?}");
    let stderr = String::from_utf8_lossy(&scoped_panic.stderr);
    assert!(
        stderr.contains("expected scoped callback panic"),
        "{scoped_panic:?}"
    );
    assert!(!stderr.contains("peer failed"), "{scoped_panic:?}");
    assert!(!stderr.contains("leak:"), "{scoped_panic:?}");
}
