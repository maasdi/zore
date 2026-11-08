use super::*;
use std::time::Duration;

const HELPER: &str = r#"
use std::io::{Read, Write};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "environment" => {
            print!("{}|{}|{}", std::env::var("ZORE_EXEC_PRESENT").unwrap_or("absent".into()), std::env::var("ZORE_EXEC_EMPTY").unwrap_or("absent".into()), std::env::var("ZORE_EXEC_PARENT").unwrap_or("absent".into()));
        }
        "echo" => {
            std::io::stdout().write_all(&[0, 255, 128]).unwrap();
            let mut data = Vec::new();
            std::io::stdin().read_to_end(&mut data).unwrap();
            std::io::stdout().write_all(&data).unwrap();
            std::io::stderr().write_all(b"diagnostic").unwrap();
        }
        "blocked" => {
            std::io::stdout().write_all(b"ready").unwrap();
            std::io::stdout().flush().unwrap();
            let mut byte = [0];
            std::io::stdin().read_exact(&mut byte).unwrap();
            std::process::exit(0);
        }
        "flood" => loop { if std::io::stdout().write_all(&[b'x'; 8192]).is_err() { break; } },
        "both" => {
            std::io::stdout().write_all(b"out").unwrap();
            std::io::stdout().flush().unwrap();
            std::io::stderr().write_all(b"err").unwrap();
        }
        "status" => std::process::exit(7),
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(30)),
        "touch" => std::fs::write(&args[2], b"started").unwrap(),
        _ => panic!("unknown action"),
    }
}
"#;

fn helper(directory: &TempDir) -> std::path::PathBuf {
    let source = directory.path().join("helper.rs");
    let executable = directory.executable("helper with spaces");
    std::fs::write(&source, HELPER).unwrap();
    let output = Command::new("rustc")
        .arg("--edition=2024")
        .arg("--crate-name=zore_exec_helper")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    executable
}

fn run_program(directory: &TempDir, source: &str) -> Output {
    let executable = directory.executable("parent");
    let mut source = source.to_string();
    for package in ["os", "io", "context", "time"] {
        let import = format!("import \"std/{package}\"\n");
        let without_import = source.replace(&import, "");
        if !without_import.contains(&format!("{package}.")) {
            source = without_import;
        }
    }
    if source.contains("waitHelpers(") {
        controlled_runtime::build_with_helper_capacity(&source, &executable, 2);
    } else if source.contains("waitIdle(") {
        controlled_runtime::build(&source, &executable);
    } else {
        build_file(&source, &executable).unwrap_or_else(|error| panic!("build: {error:?}"));
    }
    bounded_process::run(
        Command::new(executable)
            .env("ZORE_CHECK_LEAKS", "1")
            .env("ZORE_EXEC_PRESENT", "parent")
            .env("ZORE_EXEC_EMPTY", "")
            .env("ZORE_EXEC_PARENT", "inherited"),
        Duration::from_secs(20),
    )
    .unwrap()
}

const PRELUDE: &str = r#"package main
import "std/os"
import "std/os/exec"
import "std/io"
import "std/strings"
import "std/context"
import "std/time"
func check(ok bool, label string) { if !ok { panic(label) } }
func text(data []byte) string { let value, err = strings.FromBytes(data); check(err == nil, "utf8"); return value }
"#;

#[test]
fn environment_presence_snapshots_and_child_overrides_are_independent() {
    let directory = TempDir::new().unwrap();
    let helper = helper(&directory);
    let source = format!(
        r#"{PRELUDE}
func parent() string {{
    let value, _ = os.Getenv("ZORE_EXEC_PRESENT")
    return value
}}
func environment(command exec.Cmd, clear bool) string {{
    var options = exec.DefaultOptions()
    options.ClearEnv = clear
    options.Stdout = exec.Pipe
    options.Env.push("ZORE_EXEC_PRESENT=first")
    options.SetEnv("ZORE_EXEC_PRESENT", "last")
    options.SetEnv("ZORE_EXEC_EMPTY", "")
    let child, err = command.Start(context.Background(), options)
    check(err == nil, "start")
    var output, pipeErr = child.StdoutPipe()
    check(pipeErr == nil, "pipe")
    let data, readErr = io.ReadAll(output)
    check(readErr == nil, "read")
    let status, waitErr = child.Wait()
    check(waitErr == nil && status.Success(), "wait")
    return text(data[:])
}}
func main() {{
    let empty, exists = os.Getenv("ZORE_EXEC_EMPTY")
    check(empty == "" && exists, "present empty")
    let _, absent = os.Getenv("ZORE_EXEC_MISSING_220")
    check(!absent, "missing")
    for key in Array<string>{{"", "bad=key", "nul\u0000key"}} {{
        let _, present = os.Getenv(key)
        check(!present, "invalid key")
    }}
    var snapshot = os.Environ()
    snapshot.push("ZORE_EXEC_PRESENT=modified")
    check(parent() == "parent", "snapshot independence")
    let missing, missingErr = exec.Command("zore-nonexistent-program-issue-220", Array<string>{{}}[:]).Start(context.Background(), exec.DefaultOptions())
    check(missingErr != nil, "spawn failure")
    let _, missingWait = missing.Wait()
    check(missingWait != nil, "spawn failure closed handle")
    let command = exec.Command({helper:?}, Array<string>{{"environment"}}[:])
    check(environment(command, false) == "last||inherited", "overlay")
    check(environment(command, true) == "last||absent", "clear")
    check(parent() == "parent", "parent unchanged")
    var bad = exec.DefaultOptions()
    bad.Env.push("invalid")
    let failed, invalid = command.Start(context.Background(), bad)
    check(invalid == error("exec: invalid environment entry"), "invalid environment")
    let _, closed = failed.Wait()
    check(closed != nil, "failed handle")
    println("ok")
}}
"#,
        helper = helper.to_str().unwrap()
    );
    let output = run_program(&directory, &source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn streaming_pipes_roundtrip_bytes_close_stdin_and_cache_exit_status() {
    let directory = TempDir::new().unwrap();
    let helper = helper(&directory);
    let source = format!(
        r#"{PRELUDE}
async func drain(reader own exec.PipeReader) Array<byte> {{
    var source = reader
    let data, err = io.ReadAll(source)
    check(err == nil, "drain")
    return data
}}
func main() {{
    var options = exec.DefaultOptions()
    options.Stdin = exec.Pipe
    options.Stdout = exec.Pipe
    options.Stderr = exec.Pipe
    let child, err = exec.Command({helper:?}, Array<string>{{"echo"}}[:]).Start(context.Background(), options)
    check(err == nil, "start")
    var input, inErr = child.StdinPipe()
    let output, outErr = child.StdoutPipe()
    let diagnostic, diagErr = child.StderrPipe()
    check(inErr == nil, "input pipe")
    check(outErr == nil, "output pipe")
    check(diagErr == nil, "diagnostic pipe")
    let _, twice = child.StdoutPipe()
    check(twice != nil, "one take")
    let outTask = go drain(output)
    let errTask = go drain(diagnostic)
    let count, writeErr = io.WriteString(input, "literal $HOME")
    check(writeErr == nil && count == 13, "write")
    check(input.Close() == nil, "stdin EOF")
    let bytes = outTask.wait()
    let errors = errTask.wait()
    check(bytes.len() == 16 && bytes[0] == 0 && bytes[1] == 255 && bytes[2] == 128, "binary prefix")
    check(text(bytes[3:]) == "literal $HOME", "literal bytes")
    check(text(errors[:]) == "diagnostic", "stderr")
    let status, waitErr = child.Wait()
    let again, againErr = child.Wait()
    check(waitErr == nil && status.Success() && status.ExitCode() == 0 && status.Signal() == 0, "success")
    check(againErr == nil && again.Success(), "cached wait")
    check(child.Kill() == nil, "kill exited")
    let failed, startErr = exec.Command({helper:?}, Array<string>{{"status"}}[:]).Start(context.Background(), exec.DefaultOptions())
    check(startErr == nil, "nonzero start")
    let nonzero, failure = failed.Wait()
    check(failure == error("exit status 7") && nonzero.ExitCode() == 7 && !nonzero.Success(), "nonzero status")
    check(child.Close() == nil, "close")
    println("ok")
}}
"#,
        helper = helper.to_str().unwrap()
    );
    let output = run_program(&directory, &source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn cancellation_deadlines_and_drop_reap_a_gated_child() {
    let directory = TempDir::new().unwrap();
    let helper = helper(&directory);
    let marker = directory.path().join("must not exist");
    let source = format!(
        r#"{PRELUDE}
func ready(child exec.Process) {{
    var output, err = child.StdoutPipe()
    check(err == nil, "stdout")
    var bytes = Array<byte>{{0, 0, 0, 0, 0}}
    let count, readErr = io.ReadFull(output, bytes[:])
    check(readErr == nil && count == 5 && text(bytes[:]) == "ready", "gate")
}}
func blocked(ctx context.Context) (exec.Process, error) {{
    var options = exec.DefaultOptions()
    options.Stdin = exec.Pipe
    options.Stdout = exec.Pipe
    return exec.Command({helper:?}, Array<string>{{"blocked"}}[:]).Start(ctx, options)
}}
func main() {{
    let canceled, cancel = context.WithCancel(context.Background())
    cancel()
    let never, preErr = exec.Command({helper:?}, Array<string>{{"touch", {marker:?}}}[:]).Start(canceled, exec.DefaultOptions())
    check(preErr == error("context canceled"), "pre-canceled")
    let _, neverErr = never.Wait()
    check(neverErr != nil, "no process")
    let ctx, cancelChild = context.WithCancel(context.Background())
    let child, err = blocked(ctx)
    check(err == nil, "start")
    ready(child)
    cancelChild()
    let status, failure = child.Wait()
    check(!status.Success() && failure == error("context canceled"), "cancellation wins")
    let _, repeated = child.Wait()
    check(repeated == failure, "cached cancellation")
    let timeout, stop = context.WithTimeout(context.Background(), 2 * time.Second)
    let timed, timeoutErr = blocked(timeout)
    check(timeoutErr == nil, "timeout start")
    ready(timed)
    let _, expired = timed.Wait()
    stop()
    check(expired == error("context deadline exceeded"), "deadline")
    let owned, ownedErr = blocked(context.Background())
    check(ownedErr == nil, "owned start")
    ready(owned)
    check(owned.Close() == nil, "close running child")
    let dropped, dropErr = blocked(context.Background())
    check(dropErr == nil, "drop start")
    ready(dropped)
    drop(dropped)
    let naturalCtx, lateCancel = context.WithCancel(context.Background())
    let natural, naturalErr = exec.Command({helper:?}, Array<string>{{"both"}}[:]).Start(naturalCtx, exec.DefaultOptions())
    check(naturalErr == nil, "natural start")
    let _, exitErr = natural.Wait()
    lateCancel()
    let _, lateErr = natural.Wait()
    check(exitErr == nil, "natural exit")
    check(lateErr == nil, "natural exit wins")
    println("ok")
}}
"#,
        helper = helper.to_str().unwrap(),
        marker = marker.to_str().unwrap()
    );
    let output = run_program(&directory, &source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(!marker.exists());
}

#[test]
fn capture_limits_bound_floods_and_preserve_combined_order() {
    let directory = TempDir::new().unwrap();
    let helper = helper(&directory);
    let source = format!(
        r#"{PRELUDE}
func limited(limit int) exec.Options {{
    var options = exec.DefaultOptions()
    options.OutputLimit = limit
    return options
}}
func main() {{
    let flood = exec.Command({helper:?}, Array<string>{{"flood"}}[:])
    for limit in Array<int>{{0, 1, 8192, 9000, 65536, 70000}} {{
        let bytes, err = flood.OutputWith(context.Background(), limited(limit))
        check(err == error("exec: output limit exceeded") && bytes.len() == limit, "bounded flood")
        for value in bytes {{ check(value == 120, "prefix") }}
    }}
    let both = exec.Command({helper:?}, Array<string>{{"both"}}[:])
    let exact, exactErr = both.CombinedOutputWith(context.Background(), limited(6))
    check(exactErr == nil && text(exact[:]) == "outerr", "combined exact")
    let prefix, overflow = both.CombinedOutputWith(context.Background(), limited(4))
    check(overflow == error("exec: output limit exceeded") && text(prefix[:]) == "oute", "combined prefix")
    let empty, invalid = flood.OutputWith(context.Background(), limited(-1))
    check(invalid == error("exec: invalid output limit") && empty.len() == 0, "negative")
    let _, huge = flood.OutputWith(context.Background(), limited(1073741825))
    check(huge == error("exec: invalid output limit"), "max")
    println("ok")
}}
"#,
        helper = helper.to_str().unwrap()
    );
    let output = run_program(&directory, &source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn natural_completion_reaps_without_wait_and_releases_cancellation_monitors() {
    let directory = TempDir::new().unwrap();
    let helper = helper(&directory);
    let source = format!(
        r#"{PRELUDE}
func main() {{
    let ctx, cancel = context.WithCancel(context.Background())
    let child, err = exec.Command({helper:?}, Array<string>{{"both"}}[:]).Start(ctx, exec.DefaultOptions())
    check(err == nil, "start")
    waitIdle(0)
    let status, waitErr = child.Wait()
    check(waitErr == nil && status.Success(), "reaped without explicit wait")
    cancel()
    let background, backgroundErr = exec.Command({helper:?}, Array<string>{{"both"}}[:]).Start(context.Background(), exec.DefaultOptions())
    check(backgroundErr == nil, "background start")
    waitIdle(0)
    let _, failure = background.Wait()
    check(failure == nil, "background completion")
    println("ok")
}}
"#,
        helper = helper.to_str().unwrap()
    );
    let output = run_program(&directory, &source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn saturated_pipe_helpers_cannot_starve_child_control_or_many_waiters() {
    let directory = TempDir::new().unwrap();
    let helper = helper(&directory);
    let source = format!(
        r#"{PRELUDE}
type Running struct {{
    child exec.Process
    output exec.PipeReader
}}
func startGated(ctx context.Context) Running {{
    var options = exec.DefaultOptions()
    options.Stdin = exec.Pipe
    options.Stdout = exec.Pipe
    let child, err = exec.Command({helper:?}, Array<string>{{"blocked"}}[:]).Start(ctx, options)
    check(err == nil, "start gated")
    var output, pipeErr = child.StdoutPipe()
    check(pipeErr == nil, "take output")
    var ready = Array<byte>{{0, 0, 0, 0, 0}}
    let count, readErr = io.ReadFull(output, ready[:])
    check(readErr == nil && count == 5 && text(ready[:]) == "ready", "ready gate")
    return Running{{child: child, output: output}}
}}
async func finish(child own exec.Process, results channel<bool>) {{
    let _, err = child.Wait()
    results.send(err == error("context canceled"))
}}
async func readBlocked(reader own exec.PipeReader, results channel<bool>) {{
    var input = reader
    let data, err = io.ReadAll(input)
    results.send(err == nil && data.len() == 0)
}}
func extra() exec.Process {{
    var options = exec.DefaultOptions()
    options.Stdin = exec.Pipe
    let child, err = exec.Command({helper:?}, Array<string>{{"blocked"}}[:]).Start(context.Background(), options)
    check(err == nil, "start while saturated")
    return child
}}
func main() {{
    let ctx, cancel = context.WithCancel(context.Background())
    let results = channel<bool>(8)
    let reads = channel<bool>(2)
    var children = Array<Running>{{}}
    for var i = 0; i < 8; i += 1 {{ children.push(startGated(ctx)) }}
    for var i = 0; i < 8; i += 1 {{
        let present, running = children.pop()
        check(present, "owned child")
        if i < 2 {{ go readBlocked(running.output, reads) }}
        go finish(running.child, results)
    }}
    waitHelpers(2)
    let closed = extra()
    check(closed.Close() == nil, "close while saturated")
    let dropped = extra()
    drop(dropped)
    let killed = extra()
    check(killed.Kill() == nil, "kill while saturated")
    let status, killedErr = killed.Wait()
    check(killedErr != nil && !status.Success(), "reap while saturated")
    waitHelpers(2)
    cancel()
    for var i = 0; i < 8; i += 1 {{
        let canceled, _ = results.receive()
        check(canceled, "all waiters canceled")
    }}
    for var i = 0; i < 2; i += 1 {{
        let drained, _ = reads.receive()
        check(drained, "blocked reads released")
    }}
    waitHelpers(0)
    waitIdle(0)
    println("ok")
}}
"#,
        helper = helper.to_str().unwrap()
    );
    let output = run_program(&directory, &source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn helpers_take_options_and_contexts_and_streams_can_share_the_parent_terminal() {
    let directory = TempDir::new().unwrap();
    let helper = helper(&directory);
    let source = format!(
        r#"{PRELUDE}
func helper(action string) exec.Cmd {{
    return exec.Command({helper:?}, Array<string>{{action}}[:])
}}
func piped(stream int) exec.Options {{
    var options = exec.DefaultOptions()
    if stream == 0 {{ options.Stdin = exec.Pipe }}
    if stream == 1 {{ options.Stdout = exec.Pipe }}
    if stream == 2 {{ options.Stderr = exec.Pipe }}
    return options
}}
func main() {{
    var shared = exec.DefaultOptions()
    shared.Stdout = exec.Inherit
    shared.Stderr = exec.Inherit
    check(helper("both").RunWith(context.Background(), shared) == nil, "inherited streams")
    println("")
    var env = exec.DefaultOptions()
    env.SetEnv("ZORE_EXEC_PRESENT", "child")
    let data, err = helper("environment").OutputWith(context.Background(), env)
    check(err == nil && text(data[:]) == "child||inherited", "output with environment")
    var loud = exec.DefaultOptions()
    loud.Stdout = exec.Inherit
    loud.Stderr = exec.Inherit
    let out, outErr = helper("both").OutputWith(context.Background(), loud)
    check(outErr == nil && text(out[:]) == "out", "capture takes stdout, stderr inherited")
    let ctx, stop = context.WithTimeout(context.Background(), 100 * time.Millisecond)
    let slow = helper("sleep").RunWith(ctx, exec.DefaultOptions())
    stop()
    check(slow == error("context deadline exceeded"), "RunWith deadline")
    let late, stopLate = context.WithTimeout(context.Background(), 100 * time.Millisecond)
    let partial, lateErr = helper("sleep").CombinedOutputWith(late, exec.DefaultOptions())
    stopLate()
    check(lateErr == error("context deadline exceeded") && partial.len() == 0, "capture deadline")
    for stream in Array<int>{{0, 1, 2}} {{
        check(helper("both").RunWith(context.Background(), piped(stream)) == error("exec: RunWith cannot use Pipe; use Start"), "RunWith pipe")
    }}
    let rejected = error("exec: output capture cannot use Pipe for other streams; use Start")
    let _, stdinPipe = helper("both").OutputWith(context.Background(), piped(0))
    check(stdinPipe == rejected, "capture stdin pipe")
    let _, stderrPipe = helper("both").OutputWith(context.Background(), piped(2))
    check(stderrPipe == rejected, "capture stderr pipe")
    let merged, mergedErr = helper("both").CombinedOutputWith(context.Background(), piped(2))
    check(mergedErr == nil && text(merged[:]) == "outerr", "combined takes stderr")
    var bad = exec.DefaultOptions()
    bad.Stdout = exec.Stream(3)
    let failed, invalid = helper("both").Start(context.Background(), bad)
    check(invalid == error("exec: invalid stream") && failed.Pid() == 0, "invalid stream")
    let child, startErr = helper("both").Start(context.Background(), exec.DefaultOptions())
    check(startErr == nil && child.Pid() > 0, "pid")
    let _, waitErr = child.Wait()
    check(waitErr == nil, "wait")
    println("ok")
}}
"#,
        helper = helper.to_str().unwrap()
    );
    let output = run_program(&directory, &source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "out\nok\n");
    assert_eq!(stderr(&output), "errerr");
}

#[test]
fn set_env_rejects_keys_that_cannot_name_a_variable() {
    for key in ["", "A=B", "NUL\\u0000KEY"] {
        let source = format!(
            "package main\nimport \"std/os/exec\"\nfunc main() {{\n    var options = exec.DefaultOptions()\n    options.SetEnv(\"GOOD\", \"value\")\n    println(options.Env.len())\n    options.SetEnv(\"{key}\", \"value\")\n}}\n"
        );
        panics(&source, "exec: invalid environment key", "1\n");
    }
}
