use super::*;

#[test]
fn internal_linkage_keeps_referenced_initializers_callbacks_generics_and_async_cleanup() {
    prints(
        r#"package main
import "std/time"
let initial = seed()
func seed() int { println("init"); return 7 }
func unused() int { return 99 }
func identity<T copyable>(value T) T { return value }
type Reader interface { Read() int }
type Counter struct { N int }
func (c Counter) Read() int { return c.N }
func read(r Reader) int { return r.Read() }
type Resource struct { Values Array<int> }
func (r mut Resource) drop() { println("drop") }
func job(value own Resource) async func() int {
    return async func() int { time.Sleep(time.Millisecond); return value.Values[0] }
}
func main() {
    let callback = func(n int) int { return n + initial }
    println(callback(identity(3)))
    println(read(Counter{N: 11}))
    let work = job(Resource{Values: Array<int>{12}})
    let task = go work()
    println(task.wait())
}
"#,
        "init\n10\n11\ndrop\n12\n",
    );
}

#[test]
fn internal_linkage_exposes_only_the_entry_shim_and_removes_unreferenced_bodies() {
    let source = "package main\nimport \"std/time\"\nfunc unused() int { return 99 }\nfunc main() { println(time.Now() > 0) }\n";
    let mut sources = SourceMap::new();
    let file = sources.add("test.ore", source.into()).unwrap();
    let llvm = emit_llvm(sources.file(file).unwrap()).unwrap();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("program.ll");
    std::fs::write(&path, llvm).unwrap();
    let output = Command::new(zore::build::compiler())
        .args([
            "-Wno-override-module",
            "-O2",
            "-S",
            "-emit-llvm",
            "-x",
            "ir",
        ])
        .arg(path)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let optimized = stdout(&output);
    let definitions: Vec<_> = optimized
        .lines()
        .filter(|line| line.starts_with("define "))
        .collect();
    assert!(definitions.iter().any(|line| line.contains("@zore_entry(")));
    for definition in definitions {
        assert!(
            definition.contains(" internal ")
                || definition.contains(" private ")
                || definition.contains("@zore_entry("),
            "{definition}"
        );
    }
    assert!(!optimized.contains("@\"main.unused\""));
    assert!(!optimized.contains("@\"main.std/time.Sleep\""));
    assert!(optimized.contains("@zore_native_time_now("));
}

#[test]
fn unused_source_keeps_full_type_and_ownership_diagnostics() {
    for source in [
        "package main\nfunc unused() int { return true }\nfunc main() {}\n",
        "package main\nfunc unused() { let values = Array<int>{1}; let moved = values; println(values.len()); println(moved.len()) }\nfunc main() {}\n",
    ] {
        let mut sources = SourceMap::new();
        let file = sources.add("test.ore", source.into()).unwrap();
        assert!(
            matches!(emit_llvm(sources.file(file).unwrap()), Err(BuildError::Diagnostics(errors)) if !errors.is_empty())
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn canceled_context_deadlines_unregister_timers_before_the_original_deadline() {
    let output = run_controlled(
        r#"package main
import "std/context"
import "std/time"
func exercise() {
    let ctx, cancel = context.WithTimeout(context.Background(), time.Hour)
    waitPending(1)
    checkTimers(1)
    cancel()
    cancel()
    waitIdle(0)
    checkTimers(0)
    if ctx.Err() != error("context canceled") { panic("cancellation reason") }
}
func main() {
    for var i = 0; i < 32; i += 1 { exercise() }
    waitIdle(0)
    println("clean")
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "clean\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn context_deadline_expiration_and_parent_cancellation_release_every_timer() {
    let output = run_controlled(
        r#"package main
import "std/context"
import "std/time"
func expire() {
    let ctx, cancel = context.WithTimeout(context.Background(), time.Hour)
    waitPending(1)
    checkTimers(1)
    expireTimers(1)
    let _, _ = ctx.Done().receive()
    waitIdle(0)
    checkTimers(0)
    cancel()
    if ctx.Err() != error("context deadline exceeded") { panic("deadline reason") }
}
func parent() {
    let root, stopRoot = context.WithTimeout(context.Background(), time.Hour)
    let child, stopChild = context.WithTimeout(root, time.Minute)
    waitPending(3)
    checkTimers(2)
    stopRoot()
    waitIdle(0)
    checkTimers(0)
    stopChild()
    if child.Err() != error("context canceled") { panic("parent reason") }
}
func main() { expire(); parent(); waitIdle(0); println("clean") }
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "clean\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}
