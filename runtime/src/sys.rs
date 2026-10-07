//! The code behind `zore/time`, `zore/io`, and `zore/os`.

use std::io::{BufRead, Write};
use std::sync::OnceLock;
use std::time::Instant;

use super::string::StringOut;
use super::{blocking, reactor};

/// An `error` result, written by functions whose only result is an `error`.
#[repr(C)]
pub struct ErrorOut {
    failed: u8,
    message: *const u8,
    message_len: i64,
}

impl ErrorOut {
    pub(super) fn ok() -> Self {
        Self {
            failed: 0,
            message: std::ptr::null(),
            message_len: 0,
        }
    }

    pub(super) fn failed(message: &str) -> Self {
        let text = StringOut::built(message.as_bytes());
        Self {
            failed: 1,
            message: text.data,
            message_len: text.len,
        }
    }
}

/// A `string` and an `error`, written by results of the shape `(string, error)`.
#[repr(C)]
pub struct StringError {
    data: *const u8,
    len: i64,
    failed: u8,
    message: *const u8,
    message_len: i64,
}

impl StringError {
    pub(super) fn ok(text: &str) -> Self {
        let out = StringOut::built(text.as_bytes());
        Self {
            data: out.data,
            len: out.len,
            failed: 0,
            message: std::ptr::null(),
            message_len: 0,
        }
    }

    pub(super) fn failed(message: &str) -> Self {
        let text = StringOut::built(message.as_bytes());
        Self {
            data: std::ptr::null(),
            len: 0,
            failed: 1,
            message: text.data,
            message_len: text.len,
        }
    }
}

/// # Safety
/// The string must satisfy the storage rule of `bytes`.
unsafe fn text_of(data: *const u8, len: i64) -> String {
    // SAFETY: guaranteed by the caller.
    String::from_utf8_lossy(unsafe { super::bytes(data, len) }).into_owned()
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_time_sleep(milliseconds: i64) {
    if let Ok(milliseconds) = u64::try_from(milliseconds)
        && milliseconds > 0
    {
        reactor::sleep(milliseconds);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_time_millis() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX)
}

impl StringError {
    fn from_text(result: Result<String, String>) -> Self {
        match result {
            Ok(text) => Self::ok(&text),
            Err(message) => Self::failed(&message),
        }
    }
}

fn read_line() -> Result<String, String> {
    let mut line = Vec::new();
    match std::io::stdin().lock().read_until(b'\n', &mut line) {
        Ok(0) => return Err("EOF".into()),
        Ok(_) => {}
        Err(error) => return Err(format!("io.ReadLine: {error}")),
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    }
    String::from_utf8(line).map_err(|_| "io.ReadLine: invalid UTF-8".to_string())
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_io_read_line(out: *mut StringError) {
    let result = StringError::from_text(blocking::run(read_line));
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_read_file(
    out: *mut StringError,
    path: *const u8,
    path_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let result = blocking::run(move || match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| "os.ReadFile: invalid UTF-8".to_string()),
        Err(error) => Err(format!("os.ReadFile: {error}")),
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringError::from_text(result)) };
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_write_file(
    out: *mut ErrorOut,
    path: *const u8,
    path_len: i64,
    text: *const u8,
    text_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (path, text) = unsafe {
        (
            text_of(path, path_len),
            super::bytes(text, text_len).to_vec(),
        )
    };
    let failure = blocking::run(move || {
        std::fs::File::create(&path)
            .and_then(|mut file| file.write_all(&text))
            .err()
            .map(|error| format!("os.WriteFile: {error}"))
    });
    let result = match failure {
        None => ErrorOut::ok(),
        Some(message) => ErrorOut::failed(&message),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}
