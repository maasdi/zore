//! The code behind `zore/strconv`.

use super::string::StringOut;

/// A value and an `error`, written by results of the shape `(T, error)`.
#[repr(C)]
pub struct ValueError {
    value: i64,
    failed: u8,
    message: *const u8,
    message_len: i64,
}

impl ValueError {
    pub(super) fn ok(value: i64) -> Self {
        Self {
            value,
            failed: 0,
            message: std::ptr::null(),
            message_len: 0,
        }
    }

    /// The error owns a share of its own copy of the text.
    pub(super) fn failed_text(message: &str) -> Self {
        let text = StringOut::built(message.as_bytes());
        Self {
            value: 0,
            failed: 1,
            message: text.data,
            message_len: text.len,
        }
    }

    fn failed(message: &'static str) -> Self {
        Self {
            value: 0,
            failed: 1,
            message: message.as_ptr(),
            message_len: message.len() as i64,
        }
    }
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_itoa(out: *mut StringOut, value: i64) {
    // SAFETY: `out` is writable.
    unsafe { out.write(StringOut::built(value.to_string().as_bytes())) };
}

fn parse_int(text: &[u8]) -> ValueError {
    let (negative, digits) = match text.first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return ValueError::failed("strconv.Atoi: invalid syntax");
    }
    let mut magnitude: u128 = 0;
    for digit in digits {
        magnitude = magnitude * 10 + u128::from(digit - b'0');
        if magnitude > u128::from(i64::MAX as u64) + 1 {
            return ValueError::failed("strconv.Atoi: value out of range");
        }
    }
    let signed = if negative {
        -(magnitude as i128)
    } else {
        magnitude as i128
    };
    match i64::try_from(signed) {
        Ok(value) => ValueError::ok(value),
        Err(_) => ValueError::failed("strconv.Atoi: value out of range"),
    }
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_atoi(out: *mut ValueError, s: *const u8, s_len: i64) {
    // SAFETY: guaranteed by the caller.
    let parsed = parse_int(unsafe { super::bytes(s, s_len) });
    unsafe { out.write(parsed) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_format_bool(out: *mut StringOut, value: bool) {
    let text: &'static str = if value { "true" } else { "false" };
    // SAFETY: `out` is writable and the text is static.
    unsafe { out.write(StringOut::shared(text.as_ptr(), text.len())) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_parse_bool(
    out: *mut ValueError,
    s: *const u8,
    s_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let parsed = match unsafe { super::bytes(s, s_len) } {
        b"true" => ValueError::ok(1),
        b"false" => ValueError::ok(0),
        _ => ValueError::failed("strconv.ParseBool: invalid syntax"),
    };
    unsafe { out.write(parsed) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atoi(text: &str) -> Result<i64, String> {
        let result = parse_int(text.as_bytes());
        if result.failed == 0 {
            Ok(result.value)
        } else {
            // SAFETY: the message is a static string.
            Err(unsafe {
                String::from_utf8_lossy(super::super::bytes(result.message, result.message_len))
                    .into_owned()
            })
        }
    }

    #[test]
    fn integers_parse_strictly() {
        let _serial = crate::string::serial();
        assert_eq!(atoi("42"), Ok(42));
        assert_eq!(atoi("-7"), Ok(-7));
        assert_eq!(atoi("+7"), Ok(7));
        assert_eq!(atoi("0007"), Ok(7));
        assert_eq!(atoi("9223372036854775807"), Ok(i64::MAX));
        assert_eq!(atoi("-9223372036854775808"), Ok(i64::MIN));
        for bad in ["", "-", "+", " 1", "1 ", "1_0", "0x10", "1.5", "--1", "٣"] {
            assert_eq!(
                atoi(bad),
                Err("strconv.Atoi: invalid syntax".to_string()),
                "{bad:?}"
            );
        }
        for big in [
            "9223372036854775808",
            "-9223372036854775809",
            "99999999999999999999999999",
        ] {
            assert_eq!(
                atoi(big),
                Err("strconv.Atoi: value out of range".to_string()),
                "{big:?}"
            );
        }
    }

    #[test]
    fn booleans_and_integers_format() {
        let _serial = crate::string::serial();
        let mut out = StringOut::empty();
        // SAFETY: `out` is writable; the results are live until the registry is released.
        unsafe {
            zore_native_strconv_itoa(&mut out, i64::MIN);
            assert_eq!(
                super::super::bytes(out.data, out.len),
                b"-9223372036854775808"
            );
            zore_native_strconv_format_bool(&mut out, true);
            assert_eq!(super::super::bytes(out.data, out.len), b"true");
            let mut parsed = ValueError::ok(0);
            zore_native_strconv_parse_bool(&mut parsed, b"false".as_ptr(), 5);
            assert_eq!((parsed.failed, parsed.value), (0, 0));
            zore_native_strconv_parse_bool(&mut parsed, b"yes".as_ptr(), 3);
            assert_eq!(parsed.failed, 1);
        }
        super::super::string::release_all();
    }
}
