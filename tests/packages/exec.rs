use std::path::Path;

use zore::async_lowering::{self, NativeWait, Suspension};
use zore::check::check_project;
use zore::driver::project::{Memory, load_project};
use zore::mir;
use zore::source::SourceMap;

#[test]
fn subprocess_handles_compose_with_io_without_borrowing_the_command() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/context"
import "std/io"
import "std/os"
import "std/os/exec"
func start() (exec.Process, error) {
    var options = exec.DefaultOptions()
    options.Env = os.Environ()
    options.Env.push("MODE=worker")
    options.StdoutPipe = true
    return exec.Command("helper", Array<string>{}[:]).StartContext(context.Background(), options)
}
func read(reader mut io.Reader) error {
    let _, err = io.ReadAtMost(reader, 8)
    return err
}
func write(writer mut io.Writer) error {
    let _, err = writer.Write(Array<byte>{0,255}[:])
    return err
}
func run() error {
    let value, found = os.LookupEnv("MODE")
    println(value)
    println(found)
    let child = start()?
    var output = child.StdoutPipe()?
    var input = child.StdinPipe()?
    write(input)?
    input.Close()?
    read(output)?
    output.Close()?
    let status, err = child.Wait()
    println(status.Success())
    println(status.ExitCode())
    println(status.Signal())
    _ = err
    return child.Close()
}
func main() {}
"#,
    )]);
}

#[test]
fn subprocess_handles_are_private_move_resources() {
    for (body, expected) in [
        (
            "let child = exec.Command(\"helper\", Array<string>{}[:]).Start()?; let moved = child; child.Kill()?; return moved.Close()",
            "moved",
        ),
        (
            "let child = exec.Command(\"helper\", Array<string>{}[:]).Start()?; child.Close()?; let _, err = child.Wait(); return err",
            "moved",
        ),
        (
            "let child = exec.Command(\"helper\", Array<string>{}[:]).Start()?; let input = child.StdinPipe()?; input.Close()?; let _, err = input.Write(Array<byte>{1}[:]); return err",
            "moved",
        ),
        (
            "let child = exec.Command(\"helper\", Array<string>{}[:]).Start()?; let output = child.StdoutPipe()?; let moved = output; var buffer = Array<byte>{0}; let _, err = output.Read(buffer[:]); _ = err; return moved.Close()",
            "moved",
        ),
        (
            "let child = exec.Command(\"helper\", Array<string>{}[:]).Start()?; println(child.id); return nil",
            "not exported",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/os/exec\"\nfunc run() error {{ {body} }}\nfunc main() {{}}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn subprocess_waits_suspend_in_async_callers() {
    let source = r#"package main
import "std/os/exec"
async func collect() error {
    let child = exec.Command("helper", Array<string>{}[:]).Start()?
    let _, err = child.Wait()
    return err
}
func plain() error {
    return exec.Command("helper", Array<string>{}[:]).Run()
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
    let names: Vec<_> = plan
        .machines
        .keys()
        .map(|id| package.function(*id).name.as_str())
        .collect();
    assert!(names.contains(&"collect"), "{names:?}");
    assert!(!names.contains(&"plain"), "{names:?}");
    let (_, wait) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == "std/os/exec.Process.Wait")
        .unwrap();
    assert!(
        wait.suspensions
            .iter()
            .any(|(_, suspension)| matches!(suspension, Suspension::Channel))
    );
    for name in ["pipeRead", "pipeWrite", "run"] {
        let name = format!("std/os/exec.{name}");
        let function = package
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        assert_eq!(
            async_lowering::native_wait(function),
            Some(NativeWait::Helper)
        );
    }
    for name in [
        "start",
        "pollExited",
        "waitFields",
        "kill",
        "cancel",
        "beginClose",
        "processClose",
        "pipeClose",
        "pipeDrop",
    ] {
        let name = format!("std/os/exec.{name}");
        let function = package
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        assert_eq!(async_lowering::native_wait(function), None);
    }
}
