use super::*;

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn local_dependency_example_builds_runs_and_tests_each_selected_project() {
    let directory = TempDir::new().unwrap();
    let root = directory.path().join("projects café with spaces");
    for (path, text) in [
        (
            "app/zore.toml",
            include_str!("../../examples/local-dependencies/app/zore.toml"),
        ),
        (
            "app/main.ore",
            include_str!("../../examples/local-dependencies/app/main.ore"),
        ),
        (
            "app/main_test.ore",
            include_str!("../../examples/local-dependencies/app/main_test.ore"),
        ),
        (
            "greeting/zore.toml",
            include_str!("../../examples/local-dependencies/greeting/zore.toml"),
        ),
        (
            "greeting/greeting.ore",
            include_str!("../../examples/local-dependencies/greeting/greeting.ore"),
        ),
        (
            "greeting/greeting_test.ore",
            include_str!("../../examples/local-dependencies/greeting/greeting_test.ore"),
        ),
        (
            "greeting/text/text.ore",
            include_str!("../../examples/local-dependencies/greeting/text/text.ore"),
        ),
        (
            "audit/zore.toml",
            include_str!("../../examples/local-dependencies/audit/zore.toml"),
        ),
        (
            "audit/audit.ore",
            include_str!("../../examples/local-dependencies/audit/audit.ore"),
        ),
        (
            "shared/zore.toml",
            include_str!("../../examples/local-dependencies/shared/zore.toml"),
        ),
        (
            "shared/shared.ore",
            include_str!("../../examples/local-dependencies/shared/shared.ore"),
        ),
    ] {
        write(&root, path, text);
    }
    for command in ["check", "build", "run"] {
        let output = zore()
            .arg(command)
            .arg(root.join("app/main.ore"))
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{command}: {}", stderr(&output));
        assert!(output.stderr.is_empty(), "{}", stderr(&output));
        assert_eq!(
            stdout(&output),
            if command == "run" {
                "Hello, Ada!\nperson:Ada\n"
            } else {
                ""
            }
        );
    }
    let executable = directory.executable("main");
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Hello, Ada!\nperson:Ada\n");
    for target in ["app", "greeting"] {
        let output = zore().arg("test").arg(root.join(target)).output().unwrap();
        assert!(output.status.success(), "{target}: {}", stderr(&output));
        assert!(
            stdout(&output).contains("Tests: 1 passed, 0 failed"),
            "{}",
            stdout(&output)
        );
    }
    write(
        &root,
        "greeting/greeting_test.ore",
        "invalid source in an unselected dependency test\n",
    );
    let output = zore().arg("test").arg(root.join("app")).output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Tests: 1 passed, 0 failed"));
    let output = zore()
        .arg("test")
        .arg(root.join("greeting"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("greeting_test.ore"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn dependency_diamonds_initialize_once_and_preserve_async_error_and_panic_cleanup() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    for (path, text) in [
        (
            "app/zore.toml",
            "name = 'app'\n[dependencies]\nleft = { path = '../left' }\nright = { path = '../right' }\n",
        ),
        (
            "left/zore.toml",
            "name = 'left'\n[dependencies]\nshared = { path = '../shared' }\n",
        ),
        (
            "right/zore.toml",
            "name = 'right'\n[dependencies]\nshared = { path = '.././shared' }\n",
        ),
        ("shared/zore.toml", "name = 'shared'\n"),
        (
            "shared/shared.ore",
            r#"package shared
let Seed = initialize()
func initialize() int { println("init:shared"); return 42 }
type Resource struct { Name string }
func (r mut Resource) drop() { println("drop:" + r.Name) }
func New(name string) Resource { return Resource{Name: name} }
func Identity<T any>(value own T) T { return value }
"#,
        ),
        (
            "left/left.ore",
            r#"package left
import "shared"
func Seed() int { return shared.Seed }
func Make(name string) shared.Resource { return shared.Identity(shared.New(name)) }
"#,
        ),
        (
            "right/right.ore",
            r#"package right
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
"#,
        ),
        (
            "app/main.ore",
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
        ),
    ] {
        write(root, path, text);
    }
    let executable = directory.executable("main");
    let output = zore()
        .arg("build")
        .arg(root.join("app/main.ore"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
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
        assert_eq!(
            output.status.code(),
            Some(code),
            "{mode}: {}",
            stderr(&output)
        );
        assert_eq!(stdout(&output), expected, "{mode}");
        if mode == "panic" {
            assert!(stderr(&output).contains("dependency panic"));
        } else {
            assert!(output.stderr.is_empty(), "{}", stderr(&output));
        }
    }
}
