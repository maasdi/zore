use std::path::Path;

use zore::async_lowering::{self, NativeWait};
use zore::check::check_project;
use zore::driver::project::{Memory, load_project};
use zore::mir;
use zore::source::SourceMap;

#[test]
fn regexp_results_are_owned_offsets_and_handles_can_be_shared_explicitly() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/regexp"
func compiled() (regexp.Regexp, error) {
    let pattern = "(a+)"
    return regexp.Compile(pattern)
}
func inspect() error {
    let re = compiled()?
    let shared = re.Share()
    let found, present, err = shared.Find("aaa")
    if err != nil { return err }
    if present { println(found.Start); println(found.Group(1).Matched()) }
    let results = shared.FindAll("aaa", 10)?
    println(results.len())
    let replaced = shared.ReplaceAll("aaa", "$1")?
    println(replaced)
    return nil
}
func main() {}
"#,
    )]);
}

#[test]
fn regexp_handles_and_capture_arrays_follow_move_and_visibility_rules() {
    for (body, expected) in [
        (
            "let re = regexp.Compile(\"a\")?; let moved = re; let _, err = re.Matches(\"a\"); _ = err; drop(moved); return nil",
            "moved",
        ),
        (
            "let re = regexp.Compile(\"a\")?; println(re.id); return nil",
            "not exported",
        ),
        (
            "let re = regexp.Regexp{id: 1, groups: 0}; drop(re); return nil",
            "not exported",
        ),
        (
            "let re = regexp.Compile(\"a\")?; let all = re.FindAll(\"a\", 1)?; let moved = all; println(all.len()); println(moved.len()); return nil",
            "moved",
        ),
        (
            "let re = regexp.Compile(\"a\")?; let matches, present, err = re.Find(\"a\"); _ = err; let moved = matches; println(matches.Start); println(moved.Start); return nil",
            "moved",
        ),
        (
            "let re = regexp.Compile(\"a\")?; let _, err = re.Matches(12); return err",
            "mismatched types",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/regexp\"\nfunc run() error {{ {body} }}\nfunc main() {{}}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn regexp_async_wrappers_suspend_through_helpers_without_holding_table_locks() {
    let source = r#"package main
import "std/regexp"
async func run() error {
    let re = regexp.Compile("(a+)")?
    let result = re.ReplaceAll("aaa", "$1")?
    println(result)
    let _, err = re.Matches("a")
    return err
}
func main() {}
"#;
    let memory = Memory::new()
        .with("/p/zore.toml", super::MANIFEST)
        .with("/p/main.ore", source);
    let mut sources = SourceMap::new();
    let project = load_project(&mut sources, &memory, Path::new("/p/main.ore"))
        .unwrap_or_else(|_| panic!("valid project"));
    let checked = check_project(&project, &sources);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let package = checked.package.unwrap();
    let program = mir::lower::lower(&package);
    let plan = async_lowering::lower(&package, &program);
    assert!(
        plan.machines
            .keys()
            .any(|&id| package.function(id).name == "run")
    );
    for name in ["compile", "search", "replace"] {
        let function = package
            .functions
            .iter()
            .find(|f| f.name == format!("std/regexp.{name}"))
            .unwrap();
        assert_eq!(
            async_lowering::native_wait(function),
            Some(NativeWait::Helper)
        );
    }
    for name in ["share", "release", "groupCount"] {
        let function = package
            .functions
            .iter()
            .find(|f| f.name == format!("std/regexp.{name}"))
            .unwrap();
        assert_eq!(async_lowering::native_wait(function), None);
    }
}
