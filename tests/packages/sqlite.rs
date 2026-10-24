use std::path::Path;
use zore::async_lowering::{self, Suspension};
use zore::check::check_project;
use zore::driver::project::{Memory, load_project};
use zore::mir;
use zore::source::SourceMap;

fn files(main: &str) -> Vec<(&str, &str)> {
    vec![
        (
            "/app/zore.toml",
            "name = 'app'\n[dependencies]\nsqlite = { path = '../sqlite' }\n",
        ),
        ("/app/main.ore", main),
        (
            "/sqlite/zore.toml",
            include_str!("../../examples/sqlite-library/sqlite/zore.toml"),
        ),
        (
            "/sqlite/sqlite.ore",
            include_str!("../../examples/sqlite-library/sqlite/sqlite.ore"),
        ),
        (
            "/sqlite/native/bindings.abi",
            include_str!("../../examples/sqlite-library/sqlite/native/bindings.abi"),
        ),
        (
            "/sqlite/native/sqlite.c",
            include_str!("../../examples/sqlite-library/sqlite/native/sqlite.c"),
        ),
        (
            "/sqlite/native/sqlite.h",
            include_str!("../../examples/sqlite-library/sqlite/native/sqlite.h"),
        ),
        (
            "/sqlite/native/bridge.rs",
            include_str!("../../examples/sqlite-library/sqlite/native/bridge.rs"),
        ),
    ]
}

#[test]
fn sqlite_wrappers_suspend_through_helpers_and_plain_user_helpers_stay_synchronous() {
    let files = files(
        r#"package main
import "sqlite"
func plain(db sqlite.Database) error { var statement = db.Prepare("select 1")?; statement.Step()?; return nil }
async func query(db own sqlite.Database) error { var statement = db.Prepare("select 1")?; statement.Step()?; return nil }
func main() {}
"#,
    );
    let mut memory = Memory::new();
    for (path, text) in files {
        memory = memory.with(path, text);
    }
    let mut sources = SourceMap::new();
    let project = load_project(&mut sources, &memory, Path::new("/app/main.ore"))
        .unwrap_or_else(|_| panic!("valid project"));
    let checked = check_project(&project, &sources);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let package = checked.package.unwrap();
    let program = mir::lower::lower(&package);
    let plan = async_lowering::lower(&package, &program);
    let names: Vec<_> = plan
        .machines
        .keys()
        .map(|id| package.function(*id).name.as_str())
        .collect();
    assert!(names.contains(&"query"));
    assert!(!names.contains(&"plain"));
    assert!(
        names
            .iter()
            .any(|name| name.starts_with("sqlite.") && name.ends_with(".Prepare")),
        "{names:?}"
    );
    assert!(plan.machines.values().any(|machine| {
        machine
            .suspensions
            .iter()
            .any(|(_, suspension)| matches!(suspension, Suspension::Io(_)))
    }));
    let wait = |name: &str| {
        async_lowering::native_wait(
            package
                .functions
                .iter()
                .find(|function| function.name == name)
                .unwrap(),
        )
    };
    assert_eq!(
        wait("sqlite.nativeStep"),
        Some(async_lowering::NativeWait::Helper)
    );
    assert_eq!(wait("sqlite.nativeDropDatabase"), None);
}

#[test]
fn sqlite_resources_preserve_ordinary_move_and_visibility_rules() {
    for (main, error) in [
        (
            "package main\nimport \"sqlite\"\nfunc f() error { let db = sqlite.Open(\":memory:\")?; var tx = db.Begin()?; return nil }; func main() {}",
            "cannot pass immutable binding `db`",
        ),
        (
            "package main\nimport \"sqlite\"\nfunc f() error { var db = sqlite.Open(\":memory:\")?; let tx = db.Begin()?; let finished = tx.Commit()?; return nil }; func main() {}",
            "cannot pass immutable binding `tx`",
        ),
        (
            "package main\nimport \"sqlite\"\nfunc f() error { var db = sqlite.Open(\":memory:\")?; var tx = db.Begin()?; let s = tx.Prepare(\"select 1\")?; let moved = s; let count = s.ColumnCount()?; return nil }; func main() {}",
            "moved",
        ),
        (
            "package main\nimport \"sqlite\"\nfunc main() { let token, failure = sqlite.nativeOpen(\":memory:\"); _ = failure }",
            "not exported",
        ),
        (
            "package main\nimport \"sqlite\"\nfunc f() error { let db = sqlite.Open(\":memory:\")?; println(db.handle); return nil }; func main() {}",
            "not exported",
        ),
    ] {
        super::local_dependencies::rejects_tree(&files(main), error);
    }
}

#[test]
fn an_unregistered_sqlite_package_keeps_plain_functions_synchronous() {
    let memory = Memory::new()
        .with("/app/zore.toml", "name = 'app'\n[dependencies]\nsqlite = { path = '../sqlite' }\n")
        .with("/app/main.ore", "package main\nimport \"sqlite\"\nasync func run(ch channel<int>) int { return sqlite.Indirect(ch) }; func main() {}")
        .with("/sqlite/zore.toml", "name = 'sqlite'\n")
        .with("/sqlite/sqlite.ore", "package sqlite\nfunc nativeStep() int { return 3 }; func Helper(ch channel<int>) int { let n, _ = ch.receive(); return n + nativeStep() }; func Indirect(ch channel<int>) int { return Helper(ch) }");
    let mut sources = SourceMap::new();
    let project = load_project(&mut sources, &memory, Path::new("/app/main.ore"))
        .unwrap_or_else(|_| panic!("valid project"));
    let checked = check_project(&project, &sources);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let package = checked.package.unwrap();
    let program = mir::lower::lower(&package);
    let plan = async_lowering::lower(&package, &program);
    let names: Vec<_> = plan
        .machines
        .keys()
        .map(|id| package.function(*id).name.as_str())
        .collect();
    assert!(names.contains(&"run"));
    assert!(
        !names.iter().any(|name| name.starts_with("sqlite.")),
        "{names:?}"
    );
}
