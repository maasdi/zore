use std::io::Write;

use super::panic::{fail, raise};

fn write_line(text: &[u8]) {
    let mut line = Vec::new();
    let Some(capacity) = text.len().checked_add(1) else {
        raise(b"out of memory while printing");
        return;
    };
    if line.try_reserve_exact(capacity).is_err() {
        raise(b"out of memory while printing");
        return;
    }
    line.extend_from_slice(text);
    line.push(b'\n');
    // Hold the lock for the whole line and flush, so write failures are observed.
    let _blocking = super::scheduler::BlockingGuard::enter();
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&line)
        .and_then(|()| stdout.flush())
        .is_err()
    {
        raise(b"failed to write to standard output");
    }
}

/// Prints a string's bytes unchanged, followed by a line feed.
///
/// # Safety
/// `data` must address `len` live, initialized bytes, or be empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_println_str(data: *const u8, len: i64) {
    // SAFETY: guaranteed by the generated call's string ABI.
    write_line(unsafe { super::bytes(data, len) });
}

fn format_integer(buffer: &mut [u8; 21], mut value: u64, negative: bool) -> &[u8] {
    let mut start = buffer.len();
    loop {
        start -= 1;
        buffer[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    if negative {
        start -= 1;
        buffer[start] = b'-';
    }
    &buffer[start..]
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_println_u64(value: u64) {
    write_line(format_integer(&mut [0; 21], value, false));
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_println_i64(value: i64) {
    write_line(format_integer(
        &mut [0; 21],
        value.unsigned_abs(),
        value < 0,
    ));
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_println_f64(value: f64) {
    write_line(super::float::default_text(value, false).as_bytes());
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_println_f32(value: f32) {
    write_line(super::float::default_text(f64::from(value), true).as_bytes());
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_println_bool(value: bool) {
    write_line(if value { b"true" } else { b"false" });
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_println_rune(value: u32) {
    let scalar = char::from_u32(value).unwrap_or_else(|| fail(b"invalid runtime rune"));
    write_line(scalar.encode_utf8(&mut [0; 4]).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::format_integer;

    #[test]
    fn integer_extremes_have_exact_decimal_text() {
        for value in [0, 1, 10, u64::MAX] {
            assert_eq!(
                format_integer(&mut [0; 21], value, false),
                value.to_string().as_bytes()
            );
        }
        for value in [i64::MIN, -1, 0, i64::MAX] {
            assert_eq!(
                format_integer(&mut [0; 21], value.unsigned_abs(), value < 0),
                value.to_string().as_bytes()
            );
        }
    }
}
