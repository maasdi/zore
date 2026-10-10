//! The code behind `zore/strconv`.

use super::string::StringOut;
use super::sys::StringError;

/// A value and an `error`, written by results of the shape `(T, error)`.
#[repr(C)]
pub struct ValueError {
    pub(super) value: i64,
    pub(super) failed: u8,
    pub(super) message: *const u8,
    pub(super) message_len: i64,
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
        Self::failed_with(0, message)
    }

    /// A failure that still reports a value, such as the bytes sent before an error.
    pub(super) fn failed_with(value: i64, message: &str) -> Self {
        let text = StringOut::built(message.as_bytes());
        Self {
            value,
            failed: 1,
            message: text.data,
            message_len: text.len,
        }
    }
}

/// # Safety
/// The string must satisfy the storage rule of `bytes`.
unsafe fn text<'a>(data: *const u8, len: i64) -> &'a str {
    // SAFETY: guaranteed by the caller; Zore strings are well-formed UTF-8.
    let bytes = unsafe { super::bytes(data, len) };
    std::str::from_utf8(bytes).unwrap_or_default()
}

fn hex_escape(out: &mut String, c: char) {
    let code = c as u32;
    if code > 0xFFFF {
        out.push_str(&format!("\\U{code:08x}"));
    } else {
        out.push_str(&format!("\\u{code:04x}"));
    }
}

fn is_control(c: char) -> bool {
    matches!(c as u32, 0..=0x1F | 0x7F..=0x9F)
}

fn quote_with(text: &str, delimiter: char) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(delimiter);
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            c if c == delimiter => {
                out.push('\\');
                out.push(c);
            }
            c if is_control(c) => hex_escape(&mut out, c),
            c => out.push(c),
        }
    }
    out.push(delimiter);
    out
}

pub(super) fn quote(text: &str) -> String {
    quote_with(text, '"')
}

fn syntax_error(function: &str, input: &str, problem: &str) -> String {
    format!("strconv.{function}: parsing {}: {problem}", quote(input))
}

fn digit_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(byte - b'A') + 10),
        _ => None,
    }
}

enum ParseFailure {
    Syntax,
    Range,
}

/// Parses an optional sign and digits; `base` 0 reads an integer literal's prefix and separators.
fn parse_integer(input: &str, base: u32, bits: u32) -> Result<i64, ParseFailure> {
    let bytes = input.as_bytes();
    let (negative, rest) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        Some(b'+') => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    let (radix, digits, separators) = if base == 0 {
        match rest {
            [b'0', b'x' | b'X', tail @ ..] => (16, tail, true),
            [b'0', b'o' | b'O', tail @ ..] => (8, tail, true),
            [b'0', b'b' | b'B', tail @ ..] => (2, tail, true),
            _ => (10, rest, true),
        }
    } else {
        (base, rest, false)
    };
    if digits.is_empty() || digits.first() == Some(&b'_') || digits.last() == Some(&b'_') {
        return Err(ParseFailure::Syntax);
    }
    let limit = 1u128 << (bits - 1);
    let mut magnitude: u128 = 0;
    let mut previous_separator = false;
    let mut out_of_range = false;
    for &byte in digits {
        if byte == b'_' {
            if !separators || previous_separator {
                return Err(ParseFailure::Syntax);
            }
            previous_separator = true;
            continue;
        }
        previous_separator = false;
        let value = digit_value(byte)
            .filter(|&value| value < radix)
            .ok_or(ParseFailure::Syntax)?;
        if !out_of_range {
            magnitude = magnitude * u128::from(radix) + u128::from(value);
            out_of_range = magnitude > limit;
        }
    }
    if out_of_range || (!negative && magnitude == limit) {
        return Err(ParseFailure::Range);
    }
    let signed = if negative {
        -(magnitude as i128)
    } else {
        magnitude as i128
    };
    Ok(signed as i64)
}

fn parse_report(function: &str, input: &str, base: u32, bits: u32) -> ValueError {
    match parse_integer(input, base, bits) {
        Ok(value) => ValueError::ok(value),
        Err(ParseFailure::Syntax) => {
            ValueError::failed_text(&syntax_error(function, input, "invalid syntax"))
        }
        Err(ParseFailure::Range) => {
            ValueError::failed_text(&syntax_error(function, input, "value out of range"))
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

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_atoi(out: *mut ValueError, s: *const u8, s_len: i64) {
    // SAFETY: guaranteed by the caller.
    let input = unsafe { text(s, s_len) };
    let parsed = parse_report("Atoi", input, 10, 64);
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(parsed) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_format_int(
    out: *mut StringOut,
    value: i64,
    base: i64,
) {
    let Some(radix) = u32::try_from(base)
        .ok()
        .filter(|radix| (2..=36).contains(radix))
    else {
        super::panic::raise(b"strconv.FormatInt: invalid base");
        // SAFETY: `out` is writable.
        unsafe { out.write(StringOut::empty()) };
        return;
    };
    let mut magnitude = value.unsigned_abs();
    let mut digits = Vec::new();
    loop {
        let digit = (magnitude % u64::from(radix)) as u32;
        digits.push(char::from_digit(digit, radix).unwrap_or('0') as u8);
        magnitude /= u64::from(radix);
        if magnitude == 0 {
            break;
        }
    }
    if value < 0 {
        digits.push(b'-');
    }
    digits.reverse();
    // SAFETY: `out` is writable.
    unsafe { out.write(StringOut::built(&digits)) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_parse_int(
    out: *mut ValueError,
    s: *const u8,
    s_len: i64,
    base: i64,
    bit_size: i64,
) {
    // SAFETY: guaranteed by the caller.
    let input = unsafe { text(s, s_len) };
    let parsed = if base != 0 && !(2..=36).contains(&base) {
        ValueError::failed_text(&syntax_error(
            "ParseInt",
            input,
            &format!("invalid base {base}"),
        ))
    } else if !(0..=64).contains(&bit_size) {
        ValueError::failed_text(&syntax_error(
            "ParseInt",
            input,
            &format!("invalid bit size {bit_size}"),
        ))
    } else {
        let bits = if bit_size == 0 { 64 } else { bit_size as u32 };
        parse_report("ParseInt", input, base as u32, bits)
    };
    // SAFETY: guaranteed by the caller.
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
    let parsed = match unsafe { text(s, s_len) } {
        "true" => ValueError::ok(1),
        "false" => ValueError::ok(0),
        other => ValueError::failed_text(&syntax_error("ParseBool", other, "invalid syntax")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(parsed) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_quote(out: *mut StringOut, s: *const u8, s_len: i64) {
    // SAFETY: guaranteed by the caller.
    let quoted = quote(unsafe { text(s, s_len) });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringOut::built(quoted.as_bytes())) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_quote_rune(out: *mut StringOut, rune: u32) {
    let c = char::from_u32(rune).unwrap_or(char::REPLACEMENT_CHARACTER);
    let quoted = quote_with(c.encode_utf8(&mut [0; 4]), '\'');
    // SAFETY: `out` is writable.
    unsafe { out.write(StringOut::built(quoted.as_bytes())) };
}

fn hex_scalar(chars: &mut std::str::Chars<'_>, count: usize) -> Option<char> {
    let mut code = 0u32;
    for _ in 0..count {
        code = code * 16 + chars.next()?.to_digit(16)?;
    }
    char::from_u32(code)
}

fn unescape(body: &str, delimiter: char) -> Option<String> {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '\n' => return None,
            c if c == delimiter => return None,
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' if delimiter == '\'' => out.push('\''),
                'u' => out.push(hex_scalar(&mut chars, 4)?),
                'U' => out.push(hex_scalar(&mut chars, 8)?),
                _ => return None,
            },
            c => out.push(c),
        }
    }
    Some(out)
}

fn unquote(input: &str) -> Option<String> {
    let mut chars = input.chars();
    let first = chars.next()?;
    let last = chars.next_back()?;
    let body = chars.as_str();
    match (first, last) {
        ('`', '`') if !body.contains('`') => Some(body.to_string()),
        ('"', '"') => unescape(body, '"'),
        ('\'', '\'') => unescape(body, '\'').filter(|value| value.chars().count() == 1),
        _ => None,
    }
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strconv_unquote(
    out: *mut StringError,
    s: *const u8,
    s_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let input = unsafe { text(s, s_len) };
    let result = match unquote(input) {
        Some(value) => StringError::ok(&value),
        None => StringError::failed(&syntax_error("Unquote", input, "invalid syntax")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(result: &ValueError) -> String {
        // SAFETY: the message is live until the registry is released.
        unsafe {
            String::from_utf8_lossy(super::super::bytes(result.message, result.message_len))
                .into_owned()
        }
    }

    fn atoi(input: &str) -> Result<i64, String> {
        let mut result = ValueError::ok(0);
        // SAFETY: the input is live and `result` is writable.
        unsafe { zore_native_strconv_atoi(&mut result, input.as_ptr(), input.len() as i64) };
        if result.failed == 0 {
            Ok(result.value)
        } else {
            Err(message(&result))
        }
    }

    fn parse(input: &str, base: i64, bits: i64) -> Result<i64, String> {
        let mut result = ValueError::ok(0);
        // SAFETY: the input is live and `result` is writable.
        unsafe {
            zore_native_strconv_parse_int(
                &mut result,
                input.as_ptr(),
                input.len() as i64,
                base,
                bits,
            )
        };
        if result.failed == 0 {
            Ok(result.value)
        } else {
            Err(message(&result))
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
                Err(format!(
                    "strconv.Atoi: parsing {}: invalid syntax",
                    quote(bad)
                )),
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
                Err(format!(
                    "strconv.Atoi: parsing \"{big}\": value out of range"
                )),
                "{big:?}"
            );
        }
        super::super::string::release_all();
    }

    #[test]
    fn integers_parse_in_any_base_and_width() {
        let _serial = crate::string::serial();
        assert_eq!(parse("ff", 16, 64), Ok(255));
        assert_eq!(parse("-101", 2, 8), Ok(-5));
        assert!(parse("0x_ff", 0, 64).is_err());
        assert_eq!(parse("0xFF_FF", 0, 64), Ok(0xFFFF));
        assert_eq!(parse("0o755", 0, 64), Ok(0o755));
        assert_eq!(parse("-0b1010", 0, 64), Ok(-10));
        assert_eq!(parse("1_000", 0, 0), Ok(1000));
        assert_eq!(parse("zz", 36, 64), Ok(1295));
        assert_eq!(parse("127", 10, 8), Ok(127));
        assert_eq!(parse("-128", 10, 8), Ok(-128));
        assert_eq!(
            parse("128", 10, 8),
            Err("strconv.ParseInt: parsing \"128\": value out of range".into())
        );
        assert_eq!(
            parse("1_000", 10, 64),
            Err("strconv.ParseInt: parsing \"1_000\": invalid syntax".into())
        );
        assert_eq!(
            parse("1", 37, 64),
            Err("strconv.ParseInt: parsing \"1\": invalid base 37".into())
        );
        assert_eq!(
            parse("1", 10, 65),
            Err("strconv.ParseInt: parsing \"1\": invalid bit size 65".into())
        );
        super::super::string::release_all();
    }

    #[test]
    fn quoting_round_trips() {
        assert_eq!(quote("a\"b\\c\n\t\u{1}é"), "\"a\\\"b\\\\c\\n\\t\\u0001é\"");
        assert_eq!(quote_with("'", '\''), "'\\''");
        assert_eq!(quote_with("\"", '\''), "'\"'");
        for sample in [
            "",
            "plain",
            "tab\there",
            "\u{7f}\u{9f}\u{10FFFF}",
            "\"quoted\"",
        ] {
            assert_eq!(unquote(&quote(sample)).as_deref(), Some(sample));
        }
        assert_eq!(unquote("`raw\\n`").as_deref(), Some("raw\\n"));
        assert_eq!(unquote("'\\u00e9'").as_deref(), Some("é"));
        assert_eq!(unquote("\"\\U0001F600\"").as_deref(), Some("😀"));
        for bad in [
            "",
            "\"",
            "\"a",
            "'ab'",
            "\"\\x41\"",
            "\"a\nb\"",
            "\"\\uD800\"",
            "`a`b`",
        ] {
            assert_eq!(unquote(bad), None, "{bad:?}");
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
            zore_native_strconv_format_int(&mut out, -255, 16);
            assert_eq!(super::super::bytes(out.data, out.len), b"-ff");
            zore_native_strconv_format_int(&mut out, i64::MIN, 2);
            assert_eq!(out.len, 65);
            zore_native_strconv_format_bool(&mut out, true);
            assert_eq!(super::super::bytes(out.data, out.len), b"true");
            let mut parsed = ValueError::ok(0);
            zore_native_strconv_parse_bool(&mut parsed, b"false".as_ptr(), 5);
            assert_eq!((parsed.failed, parsed.value), (0, 0));
            zore_native_strconv_parse_bool(&mut parsed, b"yes".as_ptr(), 3);
            assert_eq!(parsed.failed, 1);
            assert_eq!(
                message(&parsed),
                "strconv.ParseBool: parsing \"yes\": invalid syntax"
            );
        }
        super::super::string::release_all();
    }
}
