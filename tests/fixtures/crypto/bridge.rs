use std::sync::atomic::{AtomicUsize, Ordering};

mod blocking {
    pub fn run<R>(work: impl FnOnce() -> R) -> R {
        work()
    }
}
mod sys {
    #[repr(C)]
    pub struct ByteArrayError {
        pub bytes: *mut u8,
        pub length: usize,
        pub failed: bool,
    }
    impl ByteArrayError {
        pub fn ok(bytes: &[u8]) -> Self {
            let bytes = bytes.to_vec().into_boxed_slice();
            let length = bytes.len();
            Self {
                bytes: Box::into_raw(bytes).cast(),
                length,
                failed: false,
            }
        }
        pub fn failed(_: &str) -> Self {
            Self {
                bytes: std::ptr::null_mut(),
                length: 0,
                failed: true,
            }
        }
    }
    impl Drop for ByteArrayError {
        fn drop(&mut self) {
            if !self.bytes.is_null() {
                unsafe {
                    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                        self.bytes,
                        self.length,
                    )))
                };
            }
        }
    }
}
#[path = "bridge.rs"]
mod bridge;
static RANDOM_CALLS: AtomicUsize = AtomicUsize::new(0);
#[unsafe(no_mangle)]
unsafe extern "C" fn zore_crypto_random(output: *mut u8, length: usize) -> i32 {
    RANDOM_CALLS.fetch_add(1, Ordering::Relaxed);
    unsafe { std::ptr::write_bytes(output, 0xaa, length) };
    -1
}
#[unsafe(no_mangle)]
unsafe extern "C" fn zore_crypto_sha256(_: *const u8, _: usize, output: *mut u8) -> i32 {
    unsafe { std::ptr::write_bytes(output, 0xaa, 32) };
    -1
}
#[unsafe(no_mangle)]
unsafe extern "C" fn zore_crypto_hmac_sha256(
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    output: *mut u8,
) -> i32 {
    unsafe { std::ptr::write_bytes(output, 0xaa, 32) };
    -1
}
#[unsafe(no_mangle)]
extern "C" fn zore_crypto_equal(_: *const u8, _: usize, _: *const u8, _: usize) -> i32 {
    0
}
#[test]
fn failure_and_invalid_input_never_expose_partial_bytes() {
    use bridge::*;
    let mut out = std::mem::MaybeUninit::<sys::ByteArrayError>::uninit();
    for size in [-1, 1_048_577, i64::MAX, 16] {
        unsafe { zore_native_crypto_random_bytes(out.as_mut_ptr(), size) };
        let result = unsafe { out.assume_init_read() };
        assert!(result.failed && result.length == 0 && result.bytes.is_null());
    }
    assert_eq!(RANDOM_CALLS.load(Ordering::Relaxed), 1);
    unsafe { zore_native_crypto_random_bytes(out.as_mut_ptr(), 0) };
    let result = unsafe { out.assume_init_read() };
    assert!(!result.failed && result.length == 0);
    assert_eq!(RANDOM_CALLS.load(Ordering::Relaxed), 1);
    let input = [0u8, 128, 255];
    for (data, length) in [
        (input.as_ptr(), 3),
        (std::ptr::null(), 1),
        (input.as_ptr(), -1),
    ] {
        unsafe { zore_native_crypto_sha256(out.as_mut_ptr(), data, length) };
        let result = unsafe { out.assume_init_read() };
        assert!(result.failed && result.length == 0 && result.bytes.is_null());
        unsafe {
            zore_native_crypto_hmac_sha256(out.as_mut_ptr(), data, length, input.as_ptr(), 3)
        };
        let result = unsafe { out.assume_init_read() };
        assert!(result.failed && result.length == 0 && result.bytes.is_null());
    }
    assert!(!unsafe { zore_native_crypto_equal(std::ptr::null(), 1, input.as_ptr(), 1) });
    assert!(!unsafe { zore_native_crypto_equal(input.as_ptr(), -1, input.as_ptr(), 1) });
    assert!(!unsafe { zore_native_crypto_equal(input.as_ptr(), 3, input.as_ptr(), 3) });
}
