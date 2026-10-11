//! The code behind `zore/errors`: an error's text, and the error it wraps.

use super::string::{StringOut, cause_of, with_cause};
use super::sys::ErrorOut;

/// # Safety
/// `out` must be writable and the error's text must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_errors_message(
    out: *mut StringOut,
    present: bool,
    data: *const u8,
    len: i64,
) {
    let text = if present {
        StringOut::shared(data, usize::try_from(len).unwrap_or(0))
    } else {
        StringOut::empty()
    };
    // SAFETY: `out` is writable.
    unsafe { out.write(text) };
}

/// # Safety
/// `out` must be writable and both texts must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_errors_wrap_text(
    out: *mut ErrorOut,
    data: *const u8,
    len: i64,
    present: bool,
    cause: *const u8,
    cause_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let text = unsafe { super::bytes(data, len) };
    let cause_len = if present {
        usize::try_from(cause_len).unwrap_or(0)
    } else {
        0
    };
    let built = with_cause(text, cause, cause_len);
    // SAFETY: `out` is writable.
    unsafe {
        out.write(ErrorOut {
            failed: 1,
            message: built.data,
            message_len: built.len,
        })
    };
}

/// # Safety
/// `out` must be writable and the error's text must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_errors_unwrap(
    out: *mut ErrorOut,
    present: bool,
    data: *const u8,
    len: i64,
) {
    let cause = present
        .then(|| cause_of(data, usize::try_from(len).unwrap_or(0)))
        .flatten();
    let result = match cause {
        Some((message, message_len)) => ErrorOut {
            failed: 1,
            message,
            message_len: message_len as i64,
        },
        None => ErrorOut::ok(),
    };
    // SAFETY: `out` is writable.
    unsafe { out.write(result) };
}

/// # Safety
/// As `zore_native_errors_wrap_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_fmt_wrap_text(
    out: *mut ErrorOut,
    data: *const u8,
    len: i64,
    present: bool,
    cause: *const u8,
    cause_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    unsafe { zore_native_errors_wrap_text(out, data, len, present, cause, cause_len) };
}
