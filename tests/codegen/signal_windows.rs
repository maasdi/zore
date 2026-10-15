use super::*;
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
    fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
}

extern "system" fn ignore_console_event(_: u32) -> i32 {
    1
}

pub(super) fn isolated_console() -> bool {
    let thread = std::thread::current();
    let name = thread.name().expect("named test thread");
    if std::env::var("ZORE_TEST_CONSOLE").as_deref() == Ok(name) {
        // SAFETY: the static callback protects only the isolated test controller.
        assert_ne!(
            unsafe { SetConsoleCtrlHandler(Some(ignore_console_event), 1) },
            0
        );
        // SAFETY: enable Ctrl+C delivery in this isolated console and its children.
        assert_ne!(unsafe { SetConsoleCtrlHandler(None, 0) }, 0);
        return true;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("ZORE_TEST_CONSOLE", name)
        .creation_flags(0x00000010)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait(&mut child, Duration::from_secs(180));
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        stdout(&output),
        stderr(&output)
    );
    false
}

pub(super) fn send_signal(_: &Child, event: i32) {
    assert!(matches!(event, 0 | 1));
    // SAFETY: group zero targets only this test's isolated console.
    assert_ne!(unsafe { GenerateConsoleCtrlEvent(event as u32, 0) }, 0);
}

fn wait(child: &mut Child, timeout: Duration) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("console subprocess did not finish");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn console_interrupts_notify_overlapping_subscriptions_and_restore_default_exit() {
    if !isolated_console() {
        return;
    }
    let source = r#"package main
import "std/context"
import "std/os/signal"
import "std/time"
func run() error {
    let interrupts = [signal.Signal; 1]{signal.Interrupt}
    let terminations = [signal.Signal; 1]{signal.Terminate}
    let first, stopFirst = signal.NotifyContext(context.Background(), interrupts[:])?
    let second, stopSecond = signal.NotifyContext(context.Background(), interrupts[:])?
    let peer, stopPeer = signal.NotifyContext(context.Background(), interrupts[:])?
    let other, stopOther = signal.NotifyContext(context.Background(), terminations[:])?
    stopFirst()
    stopFirst()
    if first.Err() == nil || second.Err() != nil { panic("subscription isolation") }
    println("ready")
    let _, _ = second.Done().receive()
    let _, _ = peer.Done().receive()
    if second.Err() == nil || peer.Err() == nil || other.Err() != nil { panic("wrong signal mapping") }
    stopPeer()
    stopSecond()
    let fresh, stopFresh = signal.NotifyContext(context.Background(), interrupts[:])?
    if fresh.Err() != nil { panic("stale notification") }
    stopFresh()
    stopOther()
    println("restored")
    time.Sleep(10 * time.Second)
    return nil
}
func main() { let err = run(); if err != nil { panic("signal failure") } }
"#;
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("console-signals");
    build_file(source, &executable).unwrap();
    for event in [0, 1] {
        let mut child = Command::new(&executable)
            .env("ZORE_CHECK_LEAKS", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert_eq!(line, "ready\n");
        send_signal(&child, event);
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line, "restored\n");
        send_signal(&child, event);
        assert_eq!(
            wait(&mut child, Duration::from_secs(5)).code(),
            Some(0xC000013Au32 as i32)
        );
    }
}
