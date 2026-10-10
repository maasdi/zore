//! The code behind `zore/time`, and the result layouts shared by the other runtime packages.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::alloc::zore_alloc;
use super::reactor;
use super::string::StringOut;

/// An `error` result, written by functions whose only result is an `error`.
#[repr(C)]
pub struct ErrorOut {
    pub(super) failed: u8,
    pub(super) message: *const u8,
    pub(super) message_len: i64,
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
    pub(super) data: *const u8,
    pub(super) len: i64,
    pub(super) failed: u8,
    pub(super) message: *const u8,
    pub(super) message_len: i64,
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

/// Where an `Array<byte>` descriptor is written.
#[repr(C)]
pub struct ByteArray {
    pub(super) data: *mut u8,
    pub(super) len: i64,
    pub(super) cap: i64,
}

impl ByteArray {
    pub(super) fn copy_of(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
        }
        let data = zore_alloc(bytes.len() as i64);
        // SAFETY: `data` has room for every byte.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len()) };
        Self {
            data,
            len: bytes.len() as i64,
            cap: bytes.len() as i64,
        }
    }
}

/// An `Array<byte>` and an `error`, written by results of the shape `(Array<byte>, error)`.
#[repr(C)]
pub struct ByteArrayError {
    pub(super) array: ByteArray,
    pub(super) failed: u8,
    pub(super) message: *const u8,
    pub(super) message_len: i64,
}

impl ByteArrayError {
    pub(super) fn ok(bytes: &[u8]) -> Self {
        Self {
            array: ByteArray::copy_of(bytes),
            failed: 0,
            message: std::ptr::null(),
            message_len: 0,
        }
    }

    pub(super) fn failed(message: &str) -> Self {
        let text = StringOut::built(message.as_bytes());
        Self {
            array: ByteArray::copy_of(&[]),
            failed: 1,
            message: text.data,
            message_len: text.len,
        }
    }

    pub(super) fn from_bytes(result: Result<Vec<u8>, String>) -> Self {
        match result {
            Ok(bytes) => Self::ok(&bytes),
            Err(message) => Self::failed(&message),
        }
    }
}

/// # Safety
/// The string must satisfy the storage rule of `bytes`.
pub(super) unsafe fn text_of(data: *const u8, len: i64) -> String {
    // SAFETY: guaranteed by the caller.
    String::from_utf8_lossy(unsafe { super::bytes(data, len) }).into_owned()
}

/// Whole milliseconds covering `nanoseconds`, or `None` when there is nothing to wait for.
fn wait_milliseconds(nanoseconds: i64) -> Option<u64> {
    let nanoseconds = u64::try_from(nanoseconds).ok().filter(|&n| n > 0)?;
    Some(nanoseconds.div_ceil(1_000_000))
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_time_sleep(nanoseconds: i64) {
    if let Some(milliseconds) = wait_milliseconds(nanoseconds) {
        reactor::sleep(milliseconds);
    }
}

/// # Safety
/// `context` must be the current live poll context for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_time_sleep_start(
    nanoseconds: i64,
    context: *mut super::scheduler::Context,
) -> *const reactor::Operation {
    let Some(milliseconds) = wait_milliseconds(nanoseconds) else {
        return std::ptr::null();
    };
    // SAFETY: guaranteed by the caller; only the cloned waker survives this call.
    let waiter = unsafe { &*context }.waker().clone().into();
    std::sync::Arc::into_raw(reactor::start_sleep(milliseconds, waiter))
}

fn clock_start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// The instant a `time.Now` reading names, or `None` for zero or less, which means no deadline.
pub(super) fn instant_of(reading: i64) -> Option<Instant> {
    let nanoseconds = u64::try_from(reading).ok().filter(|&n| n > 0)?;
    Some(clock_start() + Duration::from_nanos(nanoseconds - 1))
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_time_now() -> i64 {
    let elapsed = clock_start().elapsed().as_nanos();
    i64::try_from(elapsed).unwrap_or(i64::MAX - 1) + 1
}

/// Where an `Array<string>` or an `Array<int>` descriptor is written.
#[repr(C)]
pub struct WordArray {
    pub(super) data: *mut u8,
    pub(super) len: i64,
    pub(super) cap: i64,
}

impl WordArray {
    fn from_words<T>(words: Vec<T>) -> Self {
        let len = words.len();
        if len == 0 {
            return Self {
                data: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
        }
        let data = zore_alloc((len * std::mem::size_of::<T>()) as i64);
        for (index, word) in words.into_iter().enumerate() {
            // SAFETY: `data` has room for every element.
            unsafe { data.cast::<T>().add(index).write(word) };
        }
        Self {
            data,
            len: len as i64,
            cap: len as i64,
        }
    }

    pub(super) fn strings(texts: &[String]) -> Self {
        Self::from_words(
            texts
                .iter()
                .map(|text| StringOut::built(text.as_bytes()))
                .collect(),
        )
    }

    pub(super) fn ints(values: &[i64]) -> Self {
        Self::from_words(values.to_vec())
    }
}

/// An `Array<string>` or `Array<int>` and an `error`.
#[repr(C)]
pub struct WordArrayError {
    pub(super) array: WordArray,
    pub(super) failed: u8,
    pub(super) message: *const u8,
    pub(super) message_len: i64,
}

impl WordArrayError {
    pub(super) fn ok(array: WordArray) -> Self {
        Self {
            array,
            failed: 0,
            message: std::ptr::null(),
            message_len: 0,
        }
    }

    pub(super) fn failed(message: &str) -> Self {
        let text = StringOut::built(message.as_bytes());
        Self {
            array: WordArray::ints(&[]),
            failed: 1,
            message: text.data,
            message_len: text.len,
        }
    }
}

impl StringError {
    pub(super) fn from_text(result: Result<String, String>) -> Self {
        match result {
            Ok(text) => Self::ok(&text),
            Err(message) => Self::failed(&message),
        }
    }
}

impl ErrorOut {
    pub(super) fn from_failure(failure: Option<String>) -> Self {
        match failure {
            None => Self::ok(),
            Some(message) => Self::failed(&message),
        }
    }
}

/// The strings of a `[]string` argument.
///
/// # Safety
/// `parts` must address `count` strings, each satisfying the storage rule of `bytes`.
pub(super) unsafe fn texts_of(parts: *const StringOut, count: i64) -> Vec<String> {
    if count <= 0 {
        return Vec::new();
    }
    // SAFETY: guaranteed by the caller.
    let parts = unsafe { std::slice::from_raw_parts(parts, count as usize) };
    parts
        .iter()
        // SAFETY: guaranteed by the caller.
        .map(|part| unsafe { text_of(part.data, part.len) })
        .collect()
}

/// # Safety
/// The message must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_sync_fail(message: *const u8, message_len: i64) {
    // SAFETY: guaranteed by the caller.
    super::panic::raise(unsafe { super::bytes(message, message_len) });
}

/// # Safety
/// The message must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_bytes_fail(message: *const u8, message_len: i64) {
    // SAFETY: guaranteed by the caller.
    super::panic::raise(unsafe { super::bytes(message, message_len) });
}
