// Copyright 2026 The Zore Authors
// SPDX-License-Identifier: Apache-2.0

use super::blocking;
use super::sys::ByteArrayError;

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_rand_secure_read(out: *mut ByteArrayError, size: i64) {
    let result = match usize::try_from(size) {
        Ok(0) => Ok(Vec::new()),
        Ok(size) => blocking::run(move || random_bytes(size)),
        Err(_) => Err("crypto/rand.Bytes: negative size".to_string()),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ByteArrayError::from_bytes(result)) };
}

fn random_bytes(size: usize) -> Result<Vec<u8>, String> {
    let mut buffer = vec![0u8; size];
    fill(&mut buffer)
        .map(|()| buffer)
        .map_err(|error| format!("crypto/rand.Bytes: {error}"))
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
static FAIL_RANDOM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// # Safety
/// Each slice must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_subtle_constant_time_equal(
    a: *const u8,
    a_len: i64,
    b: *const u8,
    b_len: i64,
) -> bool {
    // SAFETY: guaranteed by the caller.
    let (a, b) = unsafe { (super::bytes(a, a_len), super::bytes(b, b_len)) };
    constant_time_equal(a, b)
}

fn constant_time_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for index in 0..a.len() {
        // SAFETY: `index` is below the length of both slices; volatile loads cannot be skipped.
        let (left, right) = unsafe {
            (
                std::ptr::read_volatile(a.as_ptr().add(index)),
                std::ptr::read_volatile(b.as_ptr().add(index)),
            )
        };
        difference |= left ^ right;
    }
    std::hint::black_box(difference) == 0
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn fill(buffer: &mut [u8]) -> std::io::Result<()> {
    #[cfg(target_arch = "x86_64")]
    const SYS_GETRANDOM: i64 = 318;
    #[cfg(target_arch = "aarch64")]
    const SYS_GETRANDOM: i64 = 278;
    unsafe extern "C" {
        fn syscall(number: i64, ...) -> i64;
    }
    #[cfg(test)]
    if FAIL_RANDOM.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(std::io::Error::other("injected failure"));
    }
    let mut filled = 0;
    while filled < buffer.len() {
        let rest = &mut buffer[filled..];
        // SAFETY: `rest` is a writable buffer of exactly `rest.len()` bytes; flags 0 waits for an initialized pool.
        let count = unsafe { syscall(SYS_GETRANDOM, rest.as_mut_ptr(), rest.len(), 0u32) };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        filled += count.unsigned_abs() as usize;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn fill(buffer: &mut [u8]) -> std::io::Result<()> {
    unsafe extern "C" {
        fn getentropy(buf: *mut u8, len: usize) -> i32;
    }
    for chunk in buffer.chunks_mut(256) {
        // SAFETY: `chunk` is writable and at most 256 bytes, the most getentropy accepts.
        if unsafe { getentropy(chunk.as_mut_ptr(), chunk.len()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn fill(buffer: &mut [u8]) -> std::io::Result<()> {
    const USE_SYSTEM_PREFERRED_RNG: u32 = 2;
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut std::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    for chunk in buffer.chunks_mut(u32::MAX as usize) {
        let length = u32::try_from(chunk.len()).unwrap_or(u32::MAX);
        // SAFETY: `chunk` is writable for `length` bytes; a null algorithm selects the system RNG.
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                chunk.as_mut_ptr(),
                length,
                USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status != 0 {
            return Err(std::io::Error::other(format!(
                "BCryptGenRandom failed with status {status:#x}"
            )));
        }
    }
    Ok(())
}

#[cfg(not(any(
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    target_os = "macos",
    windows
)))]
fn fill(buffer: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_bytes_fill_the_buffer_and_differ() {
        let mut first = vec![0u8; 4096];
        let mut second = vec![0u8; 4096];
        fill(&mut first).unwrap();
        fill(&mut second).unwrap();
        assert_ne!(first, second);
        assert!(first.iter().any(|byte| *byte != 0));
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[test]
    fn a_failing_source_returns_an_error_and_no_bytes() {
        FAIL_RANDOM.store(true, std::sync::atomic::Ordering::SeqCst);
        let result = random_bytes(16);
        FAIL_RANDOM.store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            result,
            Err("crypto/rand.Bytes: injected failure".to_string())
        );
        assert_eq!(random_bytes(16).map(|bytes| bytes.len()), Ok(16));
    }

    #[test]
    fn large_requests_fill_every_chunk() {
        let bytes = random_bytes(1 << 20).unwrap();
        assert_eq!(bytes.len(), 1 << 20);
        assert!(bytes[(1 << 20) - 256..].iter().any(|byte| *byte != 0));
    }

    #[test]
    fn constant_time_equal_compares_whole_inputs() {
        assert!(constant_time_equal(b"", b""));
        assert!(constant_time_equal(b"abc", b"abc"));
        assert!(!constant_time_equal(b"abc", b"abd"));
        assert!(!constant_time_equal(b"abc", b"xbc"));
        assert!(!constant_time_equal(b"abc", b"abcd"));
    }
}
