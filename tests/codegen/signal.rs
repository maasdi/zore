use super::*;
use std::io::{BufRead, BufReader};
use std::os::unix::process::ExitStatusExt;
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

#[test]
fn signal_subscriptions_overlap_and_restore_default_dispositions() {
    for selected in [2, 15] {
        let source = format!(
            r#"package main
import "std/context"
import "std/os/signal"
import "std/time"
func run() error {{
    let chosen = [signal.Signal; 1]{{signal.Signal({selector})}}
    let first, stopFirst = signal.NotifyContext(context.Background(), chosen[:])?
    let second, stopSecond = signal.NotifyContext(context.Background(), chosen[:])?
    stopFirst()
    stopFirst()
    if first.Err() == nil || second.Err() != nil {{ panic("subscription isolation") }}
    println("ready")
    let _, _ = second.Done().receive()
    if second.Err() == nil {{ panic("missing signal") }}
    stopSecond()
    println("restored")
    time.Sleep(10 * time.Second)
    return nil
}}
func main() {{ let err = run(); if err != nil {{ panic("subscription failure") }} }}
"#,
            selector = if selected == 2 { 1 } else { 2 }
        );
        let dir = TempDir::new().unwrap();
        let executable = dir.path().join("signal");
        build_file(&source, &executable).unwrap();
        let mut child = Command::new(executable)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert_eq!(line, "ready\n");
        // SAFETY: the ready child has installed the selected supported signal.
        assert_eq!(unsafe { kill(child.id() as i32, selected) }, 0);
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line, "restored\n");
        // SAFETY: the live child has restored its initial signal disposition.
        assert_eq!(unsafe { kill(child.id() as i32, selected) }, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                panic!("default signal disposition not restored");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.signal(), Some(selected));
    }
}

#[test]
fn signal_parent_cancellation_and_invalid_selection_clean_up() {
    prints(
        r#"package main
import "std/context"
import "std/os/signal"
func exercise() error {
    let parent, cancel = context.WithCancel(context.Background())
    let chosen = [signal.Signal; 3]{signal.Interrupt, signal.Interrupt, signal.Terminate}
    let child, stop = signal.NotifyContext(parent, chosen[:])?
    cancel()
    let _, _ = child.Done().receive()
    stop()
    stop()
    println(child.Err() == error("context canceled"))
    let empty = Array<signal.Signal>{}
    let _, noop, err = signal.NotifyContext(context.Background(), empty[:])
    noop()
    println(err != nil)
    let invalid = [signal.Signal; 2]{signal.Interrupt, signal.Signal(99)}
    let _, noop2, err2 = signal.NotifyContext(context.Background(), invalid[:])
    noop2()
    println(err2 != nil)
    let ctx, stopAgain = signal.NotifyContext(context.Background(), chosen[:])?
    stopAgain()
    println(ctx.Err() == error("context canceled"))
    return nil
}
func main() { let err = exercise(); if err != nil { panic("signal cleanup failure") } }
"#,
        "true\ntrue\ntrue\ntrue\n",
    );
}
