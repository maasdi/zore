use super::blocking;
use super::sys::ByteArrayError;

unsafe extern "C" {
    fn zore_crypto_random(output: *mut u8, length: usize) -> i32;
    fn zore_crypto_sha256(data: *const u8, length: usize, output: *mut u8) -> i32;
    fn zore_crypto_hmac_sha256(
        key: *const u8,
        key_length: usize,
        data: *const u8,
        length: usize,
        output: *mut u8,
    ) -> i32;
    fn zore_crypto_equal(
        left: *const u8,
        left_length: usize,
        right: *const u8,
        right_length: usize,
    ) -> i32;
}

fn readable(data: *const u8, length: i64) -> Result<usize, &'static str> {
    usize::try_from(length)
        .ok()
        .filter(|&length| length <= isize::MAX as usize && (length == 0 || !data.is_null()))
        .ok_or("crypto: invalid byte buffer")
}

fn digest_result(status: i32, bytes: &[u8; 32]) -> ByteArrayError {
    if status == 0 {
        ByteArrayError::ok(bytes)
    } else {
        ByteArrayError::failed("crypto: digest provider failed")
    }
}

/// # Safety
/// out must point to writable ByteArrayError storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_crypto_random_bytes(out: *mut ByteArrayError, size: i64) {
    let result = if !(0..=1_048_576).contains(&size) {
        ByteArrayError::failed("crypto.RandomBytes: size must be between 0 and 1048576")
    } else if size == 0 {
        ByteArrayError::ok(&[])
    } else {
        let generated = blocking::run(move || {
            let mut bytes = vec![0; size as usize];
            // SAFETY: the output allocation holds size writable bytes.
            let status = unsafe { zore_crypto_random(bytes.as_mut_ptr(), bytes.len()) };
            (status == 0).then_some(bytes)
        });
        match generated {
            Some(bytes) => ByteArrayError::ok(&bytes),
            None => ByteArrayError::failed("crypto.RandomBytes: entropy or random provider failed"),
        }
    };
    // SAFETY: the generated caller supplies writable result storage.
    unsafe { out.write(result) };
}

/// # Safety
/// out is writable and data holds length readable bytes when length is positive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_crypto_sha256(
    out: *mut ByteArrayError,
    data: *const u8,
    length: i64,
) {
    let result = match readable(data, length) {
        Ok(length) => {
            let mut bytes = [0; 32];
            // SAFETY: input is borrowed for this call and output holds 32 bytes.
            let status = unsafe { zore_crypto_sha256(data, length, bytes.as_mut_ptr()) };
            digest_result(status, &bytes)
        }
        Err(message) => ByteArrayError::failed(message),
    };
    // SAFETY: the generated caller supplies writable result storage.
    unsafe { out.write(result) };
}

/// # Safety
/// out is writable and both inputs hold their stated readable byte counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_crypto_hmac_sha256(
    out: *mut ByteArrayError,
    key: *const u8,
    key_length: i64,
    data: *const u8,
    length: i64,
) {
    let result = readable(key, key_length).and_then(|key_length| {
        readable(data, length).map(|length| {
            let mut bytes = [0; 32];
            // SAFETY: inputs are borrowed for this call and output holds 32 bytes.
            let status = unsafe {
                zore_crypto_hmac_sha256(key, key_length, data, length, bytes.as_mut_ptr())
            };
            digest_result(status, &bytes)
        })
    });
    let result = result.unwrap_or_else(ByteArrayError::failed);
    // SAFETY: the generated caller supplies writable result storage.
    unsafe { out.write(result) };
}

/// # Safety
/// Both inputs hold their stated readable byte counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_crypto_equal(
    left: *const u8,
    left_length: i64,
    right: *const u8,
    right_length: i64,
) -> bool {
    let (Ok(left_length), Ok(right_length)) =
        (readable(left, left_length), readable(right, right_length))
    else {
        return false;
    };
    // SAFETY: both validated inputs are borrowed for this call only.
    unsafe { zore_crypto_equal(left, left_length, right, right_length) != 0 }
}
