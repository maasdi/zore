//! Owned direct-child processes and streaming pipe endpoints.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::blocking;
use super::strconv::ValueError;
use super::string::StringOut;
use super::sys::{
    ByteArray, ByteArrayError, ErrorOut, WordArray, WordArrayError, text_of, texts_of,
};

const MAX_CAPTURE: i64 = 1 << 30;
const READ_CHUNK: usize = 65536;
const OUTPUT_LIMIT: &str = "exec: output limit exceeded";

#[derive(Clone, Debug)]
struct Outcome {
    code: i64,
    signal: i64,
    failure: Option<String>,
}

impl Outcome {
    fn exited(status: ExitStatus, cancellation: Option<String>) -> Self {
        let exit_code = status.code();
        let code = exit_code.map_or(-1, i64::from);
        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            status.signal().map_or(0, i64::from)
        };
        #[cfg(not(unix))]
        let signal = 0;
        let failure = cancellation.or_else(|| {
            if status.success() {
                None
            } else if let Some(code) = exit_code {
                Some(format!("exit status {code}"))
            } else if signal != 0 {
                Some(format!("signal: {signal}"))
            } else {
                Some(status.to_string())
            }
        });
        Self {
            code,
            signal,
            failure,
        }
    }
}

enum Pipe {
    Input(ChildStdin),
    Output(ChildStdout),
    Error(ChildStderr),
}

struct ChildState {
    child: Child,
    pipes: [Option<Pipe>; 3],
    cancellation: Option<String>,
    outcome: Option<Outcome>,
}

impl ChildState {
    fn observe(&mut self) -> Result<Option<Outcome>, String> {
        if self.outcome.is_none()
            && let Some(status) = self
                .child
                .try_wait()
                .map_err(|e| format!("exec: wait: {e}"))?
        {
            self.outcome = Some(Outcome::exited(status, self.cancellation.take()));
        }
        Ok(self.outcome.clone())
    }

    fn terminate(&mut self, reason: Option<String>) -> Result<(), String> {
        if self.observe()?.is_none() {
            if let Err(error) = self.child.kill() {
                if self.observe()?.is_some() {
                    return Ok(());
                }
                return Err(format!("exec: kill: {error}"));
            }
            if self.cancellation.is_none() {
                self.cancellation = reason;
            }
        }
        Ok(())
    }
}

impl Drop for ChildState {
    fn drop(&mut self) {
        self.pipes = [None, None, None];
        if self.outcome.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

type ProcessTable = HashMap<u64, Arc<Mutex<ChildState>>>;
type PipeTable = HashMap<u64, Arc<Mutex<Pipe>>>;
static NEXT: AtomicU64 = AtomicU64::new(1);
static PROCESSES: Mutex<Option<ProcessTable>> = Mutex::new(None);
static PIPES: Mutex<Option<PipeTable>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn process(id: i64) -> Result<Arc<Mutex<ChildState>>, String> {
    lock(&PROCESSES)
        .as_ref()
        .and_then(|table| table.get(&(id as u64)))
        .cloned()
        .ok_or_else(|| "exec: not an open process".to_string())
}

fn pipe(id: i64) -> Result<Arc<Mutex<Pipe>>, String> {
    lock(&PIPES)
        .as_ref()
        .and_then(|table| table.get(&(id as u64)))
        .cloned()
        .ok_or_else(|| "exec: not an open pipe".to_string())
}

fn configured(
    path: &str,
    args: &[String],
    dir: &str,
    env: &[String],
    clear: bool,
) -> Result<Command, String> {
    let mut command = Command::new(path);
    command.args(args.iter().skip(1));
    if !dir.is_empty() {
        command.current_dir(dir);
    }
    if clear {
        command.env_clear();
    }
    for entry in env {
        let Some((key, value)) = entry.split_once('=') else {
            return Err("exec: invalid environment entry".to_string());
        };
        if key.is_empty() || entry.contains('\0') {
            return Err("exec: invalid environment entry".to_string());
        }
        command.env(key, value);
    }
    Ok(command)
}

fn start(
    path: &str,
    args: &[String],
    dir: &str,
    env: &[String],
    clear: bool,
    pipes: i64,
) -> Result<i64, String> {
    let mut command = configured(path, args, dir, env, clear)?;
    let stream = |bit| {
        if pipes & bit != 0 {
            Stdio::piped()
        } else {
            Stdio::null()
        }
    };
    command.stdin(stream(1)).stdout(stream(2)).stderr(stream(4));
    let mut child = command.spawn().map_err(|e| format!("exec: {path}: {e}"))?;
    let pipes = [
        child.stdin.take().map(Pipe::Input),
        child.stdout.take().map(Pipe::Output),
        child.stderr.take().map(Pipe::Error),
    ];
    let state = ChildState {
        child,
        pipes,
        cancellation: None,
        outcome: None,
    };
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    lock(&PROCESSES)
        .get_or_insert_with(HashMap::new)
        .insert(id, Arc::new(Mutex::new(state)));
    Ok(id as i64)
}

fn wait(state: &Mutex<ChildState>) -> Result<Outcome, String> {
    loop {
        if let Some(outcome) = lock(state).observe()? {
            return Ok(outcome);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn close(id: i64) -> Result<(), String> {
    let state = lock(&PROCESSES)
        .as_mut()
        .and_then(|table| table.remove(&(id as u64)))
        .ok_or_else(|| "exec: not an open process".to_string())?;
    let termination = {
        let mut state = lock(&state);
        state.pipes = [None, None, None];
        state.terminate(None)
    };
    let waited = wait(&state);
    termination.and(waited.map(|_| ()))
}

fn take_pipe(id: i64, which: i64) -> Result<i64, String> {
    let index = usize::try_from(which)
        .ok()
        .filter(|&n| n < 3)
        .ok_or_else(|| "exec: invalid pipe".to_string())?;
    let state = process(id)?;
    let endpoint = lock(&state).pipes[index]
        .take()
        .ok_or_else(|| "exec: pipe not requested or already taken".to_string())?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    lock(&PIPES)
        .get_or_insert_with(HashMap::new)
        .insert(id, Arc::new(Mutex::new(endpoint)));
    Ok(id as i64)
}

fn read_pipe(id: i64, max: i64) -> Result<Vec<u8>, String> {
    let endpoint = pipe(id)?;
    let max = usize::try_from(max).map_err(|_| "exec: negative read length".to_string())?;
    if max == 0 {
        return Ok(Vec::new());
    }
    let mut buffer = vec![0; max.min(READ_CHUNK)];
    let read = match &mut *lock(&endpoint) {
        Pipe::Output(reader) => reader.read(&mut buffer),
        Pipe::Error(reader) => reader.read(&mut buffer),
        Pipe::Input(_) => return Err("exec: pipe not open for reading".to_string()),
    }
    .map_err(|e| format!("exec: read: {e}"))?;
    if read == 0 {
        return Err("EOF".to_string());
    }
    buffer.truncate(read);
    Ok(buffer)
}

fn write_pipe(id: i64, bytes: &[u8]) -> Result<usize, (usize, String)> {
    let endpoint = pipe(id).map_err(|e| (0, e))?;
    let mut endpoint = lock(&endpoint);
    let Pipe::Input(writer) = &mut *endpoint else {
        return Err((0, "exec: pipe not open for writing".to_string()));
    };
    let mut sent = 0;
    while sent < bytes.len() {
        match writer.write(&bytes[sent..]) {
            Ok(0) => return Err((sent, "exec: write: no progress".to_string())),
            Ok(count) => sent += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err((sent, format!("exec: write: {error}"))),
        }
    }
    Ok(sent)
}

fn close_pipe(id: i64) -> Result<(), String> {
    lock(&PIPES)
        .as_mut()
        .and_then(|table| table.remove(&(id as u64)))
        .map(drop)
        .ok_or_else(|| "exec: not an open pipe".to_string())
}

fn capture(
    path: &str,
    args: &[String],
    dir: &str,
    mode: i64,
    limit: i64,
) -> (Vec<u8>, Option<String>) {
    if !(0..=MAX_CAPTURE).contains(&limit) {
        return (Vec::new(), Some("exec: invalid output limit".to_string()));
    }
    let result = (|| -> Result<_, String> {
        let mut command = configured(path, args, dir, &[], false)?;
        command.stdin(Stdio::null());
        let combined = if mode == 2 {
            let (reader, writer) = std::io::pipe().map_err(|e| format!("exec: pipe: {e}"))?;
            let copy = writer.try_clone().map_err(|e| format!("exec: pipe: {e}"))?;
            command.stdout(writer).stderr(copy);
            Some(reader)
        } else {
            command
                .stdout(if mode == 1 {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stderr(Stdio::null());
            None
        };
        let mut child = command.spawn().map_err(|e| format!("exec: {path}: {e}"))?;
        drop(command);
        let mut output = Vec::new();
        let mut reader: Option<Box<dyn Read>> = combined
            .map(|r| Box::new(r) as Box<dyn Read>)
            .or_else(|| child.stdout.take().map(|r| Box::new(r) as Box<dyn Read>));
        let read = if let Some(reader) = &mut reader {
            read_bounded(reader, &mut output, limit as usize)
        } else {
            Ok(())
        };
        drop(reader);
        if read.is_err() {
            let _ = child.kill();
        }
        let waited = child.wait().map_err(|e| format!("exec: wait: {e}"));
        let failure = read.err().or_else(|| match waited {
            Ok(status) => Outcome::exited(status, None).failure,
            Err(error) => Some(error),
        });
        Ok((output, failure))
    })();
    result.unwrap_or_else(|error| (Vec::new(), Some(error)))
}

fn read_bounded(reader: &mut dyn Read, output: &mut Vec<u8>, limit: usize) -> Result<(), String> {
    let mut buffer = [0; 8192];
    loop {
        let want = buffer.len().min(limit - output.len() + 1);
        let count = match reader.read(&mut buffer[..want]) {
            Ok(0) => return Ok(()),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("exec: read: {error}")),
        };
        let retained = count.min(limit - output.len());
        output.extend_from_slice(&buffer[..retained]);
        if retained < count {
            return Err(OUTPUT_LIMIT.to_string());
        }
    }
}

/// # Safety
/// `out` must be writable; strings and slices must address their declared lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_start(
    out: *mut ValueError,
    path: *const u8,
    path_len: i64,
    args: *const StringOut,
    args_len: i64,
    dir: *const u8,
    dir_len: i64,
    env: *const StringOut,
    env_len: i64,
    clear: bool,
    pipes: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (path, args, dir, env) = unsafe {
        (
            text_of(path, path_len),
            texts_of(args, args_len),
            text_of(dir, dir_len),
            texts_of(env, env_len),
        )
    };
    let _blocking = super::scheduler::BlockingGuard::enter();
    let result = start(&path, &args, &dir, &env, clear, pipes);
    let value = match result {
        Ok(id) => ValueError::ok(id),
        Err(error) => ValueError::failed_text(&error),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_exec_poll_exited(id: i64) -> bool {
    match process(id) {
        Ok(state) => !matches!(lock(&state).observe(), Ok(None)),
        Err(_) => true,
    }
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_begin_close(out: *mut ErrorOut, id: i64) {
    let failure = process(id)
        .and_then(|state| {
            let mut state = lock(&state);
            state.pipes = [None, None, None];
            state.terminate(None)
        })
        .err();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_wait_fields(out: *mut WordArrayError, id: i64) {
    let result = process(id).and_then(|state| {
        lock(&state)
            .observe()?
            .ok_or_else(|| "exec: process is still running".to_string())
    });
    let value = match result {
        Ok(outcome) => {
            let mut result = match outcome.failure {
                Some(error) => WordArrayError::failed(&error),
                None => WordArrayError::ok(WordArray::ints(&[])),
            };
            result.array = WordArray::ints(&[outcome.code, outcome.signal]);
            result
        }
        Err(error) => WordArrayError::failed(&error),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_kill(out: *mut ErrorOut, id: i64) {
    let failure = process(id)
        .and_then(|state| lock(&state).terminate(None))
        .err();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// The error string must address its declared length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_cancel(
    id: i64,
    present: bool,
    reason: *const u8,
    reason_len: i64,
) {
    if present {
        // SAFETY: guaranteed by the caller.
        let reason = unsafe { text_of(reason, reason_len) };
        if let Ok(state) = process(id) {
            let _ = lock(&state).terminate(Some(reason));
        }
    }
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_process_close(out: *mut ErrorOut, id: i64) {
    let _blocking = super::scheduler::BlockingGuard::enter();
    let failure = close(id).err();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_exec_process_drop(id: i64) {
    let _blocking = super::scheduler::BlockingGuard::enter();
    let _ = close(id);
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_take_pipe(out: *mut ValueError, id: i64, which: i64) {
    let value = match take_pipe(id, which) {
        Ok(id) => ValueError::ok(id),
        Err(error) => ValueError::failed_text(&error),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_pipe_read(out: *mut ByteArrayError, id: i64, max: i64) {
    let value = blocking::run(move || read_pipe(id, max));
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ByteArrayError::from_bytes(value)) };
}

/// # Safety
/// `out` must be writable and data must address its declared length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_pipe_write(
    out: *mut ValueError,
    id: i64,
    data: *const u8,
    data_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let data = unsafe { super::bytes(data, data_len) }.to_vec();
    let result = blocking::run(move || write_pipe(id, &data));
    let value = match result {
        Ok(count) => ValueError::ok(count as i64),
        Err((count, error)) => ValueError::failed_with(count as i64, &error),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_pipe_close(out: *mut ErrorOut, id: i64) {
    let failure = close_pipe(id).err();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_exec_pipe_drop(id: i64) {
    let _ = close_pipe(id);
}

/// # Safety
/// `out` must be writable; strings and slices must address their declared lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_exec_run(
    out: *mut ByteArrayError,
    path: *const u8,
    path_len: i64,
    args: *const StringOut,
    args_len: i64,
    dir: *const u8,
    dir_len: i64,
    mode: i64,
    limit: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (path, args, dir) = unsafe {
        (
            text_of(path, path_len),
            texts_of(args, args_len),
            text_of(dir, dir_len),
        )
    };
    let (output, failure) = blocking::run(move || capture(&path, &args, &dir, mode, limit));
    let mut value = match failure {
        Some(error) => ByteArrayError::failed(&error),
        None => ByteArrayError::ok(&[]),
    };
    value.array = ByteArray::copy_of(&output);
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_negative_exit_code_keeps_its_numeric_error() {
        use std::os::windows::process::ExitStatusExt;
        let outcome = Outcome::exited(ExitStatus::from_raw(u32::MAX), None);
        assert_eq!(outcome.code, -1);
        assert_eq!(outcome.failure.as_deref(), Some("exit status -1"));
    }

    #[test]
    fn controlled_child() {
        if std::env::var_os("ZORE_EXEC_RUNTIME_GATE").is_none() {
            return;
        }
        std::io::stdout().write_all(b"child-ready").unwrap();
        std::io::stdout().flush().unwrap();
        let mut byte = [0];
        let _ = std::io::stdin().read_exact(&mut byte);
    }

    #[test]
    fn owned_cleanup_removes_handles_closes_untaken_pipes_and_reaps() {
        const PROBE: &str = "ZORE_EXEC_CLEANUP_PROBE";
        if std::env::var_os(PROBE).is_none() {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "exec::tests::owned_cleanup_removes_handles_closes_untaken_pipes_and_reaps",
                    "--nocapture",
                ])
                .env(PROBE, "1")
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("process cleanup probe timed out");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        for explicit in [true, false] {
            let path = std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let args = [
                path.clone(),
                "--exact".to_string(),
                "exec::tests::controlled_child".to_string(),
                "--nocapture".to_string(),
            ];
            let id = start(
                &path,
                &args,
                "",
                &["ZORE_EXEC_RUNTIME_GATE=1".to_string()],
                false,
                7,
            )
            .unwrap();
            let state = process(id).unwrap();
            let output = take_pipe(id, 1).unwrap();
            let mut ready = Vec::new();
            while !ready.windows(11).any(|bytes| bytes == b"child-ready") {
                ready.extend(read_pipe(output, 64).unwrap());
                assert!(ready.len() < 4096);
            }
            assert!(lock(&state).observe().unwrap().is_none());
            if explicit {
                close(id).unwrap();
            } else {
                zore_native_exec_process_drop(id);
            }
            assert!(process(id).is_err());
            let mut state = lock(&state);
            assert!(state.outcome.is_some());
            assert!(state.child.try_wait().unwrap().is_some());
            assert!(state.pipes.iter().all(Option::is_none));
            drop(state);
            close_pipe(output).unwrap();
            assert!(pipe(output).is_err());
        }
    }

    #[test]
    fn bounded_reads_keep_exact_prefix_and_probe_only_one_extra_byte() {
        for limit in [0, 1, 8191, 8192, 8193] {
            let bytes = vec![b'x'; limit + 10000];
            let mut reader = bytes.as_slice();
            let mut output = Vec::new();
            assert_eq!(
                read_bounded(&mut reader, &mut output, limit),
                Err(OUTPUT_LIMIT.to_string())
            );
            assert_eq!(output, vec![b'x'; limit]);
            assert_eq!(reader.len(), bytes.len() - limit - 1);
        }
    }

    #[test]
    fn bounded_reads_accept_exact_limit_and_empty_output() {
        for bytes in [b"".as_slice(), b"abcd".as_slice()] {
            let mut output = Vec::new();
            assert!(read_bounded(&mut &bytes[..], &mut output, bytes.len()).is_ok());
            assert_eq!(output, bytes);
        }
    }

    #[test]
    fn invalid_environment_and_limits_fail_before_spawn() {
        for entry in ["missing_equals", "=value", "x=contains\0nul"] {
            assert!(configured("never-started", &[], "", &[entry.to_string()], false).is_err());
        }
        for limit in [-1, MAX_CAPTURE + 1] {
            let (output, error) = capture("never-started", &[], "", 1, limit);
            assert!(output.is_empty());
            assert_eq!(error.as_deref(), Some("exec: invalid output limit"));
        }
    }
}
