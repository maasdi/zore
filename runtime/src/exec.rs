//! The code behind `zore/os/exec`.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};

use super::blocking;
use super::string::StringOut;
use super::sys::{ByteArray, ByteArrayError, text_of, texts_of};

#[derive(Clone, Copy)]
enum Capture {
    Nothing,
    Output,
    Combined,
}

fn status_error(status: ExitStatus) -> Option<String> {
    if status.success() {
        return None;
    }
    if let Some(code) = status.code() {
        return Some(format!("exit status {code}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Some(format!("signal: {signal}"));
        }
    }
    Some(status.to_string())
}

fn run(path: &str, args: &[String], dir: &str, capture: Capture) -> (Vec<u8>, Option<String>) {
    let mut command = Command::new(path);
    command.args(args.iter().skip(1));
    if !dir.is_empty() {
        command.current_dir(dir);
    }
    command.stdin(Stdio::null());
    let reader = match capture {
        Capture::Nothing => {
            command.stdout(Stdio::null()).stderr(Stdio::null());
            None
        }
        Capture::Output => {
            command.stdout(Stdio::piped()).stderr(Stdio::null());
            None
        }
        Capture::Combined => match std::io::pipe() {
            Ok((reader, writer)) => {
                let Ok(copy) = writer.try_clone() else {
                    return (
                        Vec::new(),
                        Some(format!("exec: {path}: cannot share output")),
                    );
                };
                command.stdout(writer).stderr(copy);
                Some(reader)
            }
            Err(error) => return (Vec::new(), Some(format!("exec: {path}: {error}"))),
        },
    };
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return (Vec::new(), Some(format!("exec: {path}: {error}"))),
    };
    drop(command);
    let mut output = Vec::new();
    let read = match (reader, child.stdout.take()) {
        (Some(mut reader), _) => reader.read_to_end(&mut output),
        (None, Some(mut stdout)) => stdout.read_to_end(&mut output),
        (None, None) => Ok(0),
    };
    let waited = child.wait();
    let failure = match (read, waited) {
        (Err(error), _) | (_, Err(error)) => Some(format!("exec: {path}: {error}")),
        (Ok(_), Ok(status)) => status_error(status),
    };
    (output, failure)
}

/// # Safety
/// `out` must be writable; every string must satisfy the storage rule of `bytes`, and `args`
/// must address `args_len` strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_run(
    out: *mut ByteArrayError,
    path: *const u8,
    path_len: i64,
    args: *const StringOut,
    args_len: i64,
    dir: *const u8,
    dir_len: i64,
    capture: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (path, args, dir) = unsafe {
        (
            text_of(path, path_len),
            texts_of(args, args_len),
            text_of(dir, dir_len),
        )
    };
    let capture = match capture {
        1 => Capture::Output,
        2 => Capture::Combined,
        _ => Capture::Nothing,
    };
    let (output, failure) = blocking::run(move || run(&path, &args, &dir, capture));
    let result = match failure {
        None => ByteArrayError::ok(&output),
        Some(message) => {
            let mut result = ByteArrayError::failed(&message);
            result.array = ByteArray::copy_of(&output);
            result
        }
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}
