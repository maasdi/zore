#[path = "common.rs"]
mod common;

use std::io::Write;
use std::process::Command;
use std::time::Duration;

use zore::build::TempDir;

fn fixture(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "oracle_subprocess_fixture",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ZORE_ORACLE_TEST_MODE", mode);
    command
}

#[test]
fn completed_oracles_preserve_output_larger_than_pipe_buffers() {
    let dir = TempDir::new().unwrap();
    let result =
        common::oracle_output(&mut fixture("output"), dir.path(), Duration::from_secs(10)).unwrap();
    assert!(!result.timed_out);
    assert!(result.output.status.success());
    assert!(
        result
            .output
            .stdout
            .windows(262_144)
            .any(|bytes| bytes.iter().all(|&byte| byte == b'o'))
    );
    assert_eq!(result.output.stderr, vec![b'e'; 262_144]);
}

#[test]
fn failed_oracles_preserve_exit_status_and_diagnostics() {
    let dir = TempDir::new().unwrap();
    let result =
        common::oracle_output(&mut fixture("failure"), dir.path(), Duration::from_secs(10))
            .unwrap();
    assert!(!result.timed_out);
    assert_eq!(result.output.status.code(), Some(23));
    assert_eq!(result.output.stderr, b"fixture failure\n");
}

#[test]
fn timed_out_oracles_preserve_partial_output_and_are_reaped() {
    let dir = TempDir::new().unwrap();
    let result =
        common::oracle_output(&mut fixture("timeout"), dir.path(), Duration::from_secs(2)).unwrap();
    assert!(result.timed_out);
    assert!(!result.output.status.success());
    assert_eq!(result.output.stderr, b"partial diagnostic\n");
    let output = String::from_utf8(result.output.stdout).unwrap();
    let pid: i32 = output
        .lines()
        .find_map(|line| line.split_once("fixture pid ").map(|(_, pid)| pid))
        .unwrap()
        .parse()
        .unwrap();
    #[cfg(unix)]
    {
        const WNOHANG: i32 = 1;
        const ECHILD: i32 = 10;
        unsafe extern "C" {
            fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        }
        let mut status = 0;
        // SAFETY: the recorded child PID is positive and status is writable.
        assert_eq!(unsafe { waitpid(pid, &mut status, WNOHANG) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(ECHILD));
    }
    #[cfg(not(unix))]
    let _ = pid;
}

#[test]
#[ignore = "invoked only as an oracle subprocess fixture"]
fn oracle_subprocess_fixture() {
    match std::env::var("ZORE_ORACLE_TEST_MODE").unwrap().as_str() {
        "output" => {
            std::io::stdout().write_all(&vec![b'o'; 262_144]).unwrap();
            std::io::stderr().write_all(&vec![b'e'; 262_144]).unwrap();
        }
        "failure" => {
            std::io::stderr().write_all(b"fixture failure\n").unwrap();
            std::process::exit(23);
        }
        "timeout" => {
            println!("fixture pid {}", std::process::id());
            std::io::stdout().flush().unwrap();
            std::io::stderr()
                .write_all(b"partial diagnostic\n")
                .unwrap();
            std::thread::sleep(Duration::from_secs(10));
            std::process::exit(91);
        }
        mode => panic!("unknown fixture mode: {mode}"),
    }
}
