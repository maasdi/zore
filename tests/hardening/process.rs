use std::io::Read;
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 256 * 1024;

#[cfg(windows)]
#[path = "windows_job.rs"]
mod windows_job;

struct Reap {
    child: Child,
    terminated: bool,
    #[cfg(windows)]
    job: Option<windows_job::Job>,
}

impl Reap {
    fn terminate(&mut self) {
        if self.terminated {
            return;
        }
        self.terminated = true;
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        terminate_tree(&mut self.child);
    }
}

#[cfg(unix)]
fn terminate_tree(child: &mut Child) {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    if let Ok(pid) = i32::try_from(child.id()) {
        unsafe { kill(-pid, 9) };
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn terminate_tree(child: &mut Child) {
    let _ = child.kill();
}

impl Drop for Reap {
    fn drop(&mut self) {
        self.terminate();
        let _ = self.child.wait();
    }
}

fn capture(mut pipe: impl Read, exceeded: Arc<AtomicBool>) -> std::io::Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = match pipe.read(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if count == 0 {
            return Ok(captured);
        }
        let remaining = OUTPUT_LIMIT - captured.len();
        captured.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            exceeded.store(true, Ordering::Release);
        }
    }
}

pub fn run(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = Reap {
        child: command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("spawn {command:?}: {error}"))?,
        terminated: false,
        #[cfg(windows)]
        job: None,
    };
    #[cfg(windows)]
    {
        child.job = Some(windows_job::Job::attach(&child.child)?);
    }
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = child.child.stdout.take().unwrap();
    let stderr = child.child.stderr.take().unwrap();
    let out_flag = exceeded.clone();
    let err_flag = exceeded.clone();
    let (out_sender, out_receiver) = std::sync::mpsc::channel();
    let (err_sender, err_receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = out_sender.send(capture(stdout, out_flag));
    });
    thread::spawn(move || {
        let _ = err_sender.send(capture(stderr, err_flag));
    });
    let deadline = Instant::now() + timeout;
    let mut failure = None;
    let status = loop {
        if exceeded.load(Ordering::Acquire) || Instant::now() >= deadline {
            failure = Some(if exceeded.load(Ordering::Acquire) {
                "output limit"
            } else {
                "time limit"
            });
            child.terminate();
            break child.child.wait().expect("reap bounded child");
        }
        if let Some(status) = child.child.try_wait().expect("poll bounded child") {
            break status;
        }
        thread::sleep(Duration::from_millis(5));
    };
    child.terminate();
    let drain_timeout = Duration::from_secs(2);
    let output = Output {
        status,
        stdout: out_receiver
            .recv_timeout(drain_timeout)
            .map_err(|_| "stdout pipe held by descendant")?
            .map_err(|error| format!("read stdout: {error}"))?,
        stderr: err_receiver
            .recv_timeout(drain_timeout)
            .map_err(|_| "stderr pipe held by descendant")?
            .map_err(|error| format!("read stderr: {error}"))?,
    };
    if exceeded.load(Ordering::Acquire) {
        failure = Some("output limit");
    }
    if let Some(reason) = failure {
        return Err(format!(
            "{reason}; stdout tail: {}; stderr tail: {}",
            String::from_utf8_lossy(&output.stdout[output.stdout.len().saturating_sub(2048)..]),
            String::from_utf8_lossy(&output.stderr[output.stderr.len().saturating_sub(2048)..]),
        ));
    }
    Ok(output)
}

#[test]
#[allow(clippy::zombie_processes)]
fn bounded_child_fixture() {
    let Ok(mode) = std::env::var("ZORE_BOUNDED_CHILD") else {
        return;
    };
    match mode.as_str() {
        "quiet" => thread::sleep(Duration::from_secs(10)),
        "output" => loop {
            println!("{}", "x".repeat(4096));
        },
        "descendant" | "descendant-timeout" => {
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "bounded_process::bounded_child_fixture",
                    "--nocapture",
                ])
                .env("ZORE_BOUNDED_CHILD", "quiet")
                .spawn()
                .unwrap();
            println!("parent exited");
            if mode == "descendant-timeout" {
                loop {
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
        _ => panic!("unexpected fixture mode"),
    }
}

fn fixture(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "bounded_process::bounded_child_fixture",
            "--nocapture",
        ])
        .env("ZORE_BOUNDED_CHILD", mode);
    command
}

#[test]
fn time_and_output_limits_stop_children() {
    let started = Instant::now();
    let error = run(&mut fixture("quiet"), Duration::from_millis(100)).unwrap_err();
    assert!(error.contains("time limit"), "{error}");
    let error = run(&mut fixture("output"), Duration::from_secs(5)).unwrap_err();
    assert!(error.contains("output limit"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn successful_parent_does_not_hang_on_pipe_holding_descendant() {
    let started = Instant::now();
    let result = run(&mut fixture("descendant"), Duration::from_secs(5));
    #[cfg(unix)]
    assert!(result.unwrap().status.success());
    #[cfg(windows)]
    match result {
        Ok(output) => assert!(output.status.success()),
        Err(error) => assert!(!error.is_empty()),
    }
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn timeout_stops_pipe_holding_descendant() {
    let started = Instant::now();
    let error = run(&mut fixture("descendant-timeout"), Duration::from_secs(1)).unwrap_err();
    assert!(error.contains("time limit"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
}
