use std::cell::RefCell;

use super::alloc::{zore_alloc, zore_free};

thread_local! {
    /// Storage built at run time, released when the program ends because `string` is Copy.
    static OWNED: RefCell<Vec<(*mut u8, i64)>> = const { RefCell::new(Vec::new()) };
}

/// Where a compiler-produced `string` descriptor is written.
#[repr(C)]
pub struct StringOut {
    data: *const u8,
    len: i64,
}

impl StringOut {
    fn empty() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }

    fn built(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::empty();
        }
        let len = bytes.len() as i64;
        let data = zore_alloc(len);
        // SAFETY: `data` holds `len` writable bytes and does not overlap `bytes`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len()) };
        OWNED.with(|owned| owned.borrow_mut().push((data, len)));
        Self { data, len }
    }
}

pub(super) fn release_all() {
    OWNED.with(|owned| {
        for (data, len) in owned.borrow_mut().drain(..) {
            // SAFETY: each entry came from `zore_alloc(len)` and is released once.
            unsafe { zore_free(data, len) };
        }
    });
}

/// The two strings joined; an empty operand shares the other's storage.
///
/// # Safety
/// `out` must be writable, and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_concat(
    out: *mut StringOut,
    a: *const u8,
    a_len: i64,
    b: *const u8,
    b_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (left, right) = unsafe { (super::bytes(a, a_len), super::bytes(b, b_len)) };
    let joined = if right.is_empty() {
        StringOut {
            data: a,
            len: a_len,
        }
    } else if left.is_empty() {
        StringOut {
            data: b,
            len: b_len,
        }
    } else {
        StringOut::built(&[left, right].concat())
    };
    // SAFETY: `out` is writable.
    unsafe { out.write(joined) };
}

/// The one-character string for a scalar value.
///
/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_from_rune(out: *mut StringOut, rune: u32) {
    let character = char::from_u32(rune).unwrap_or(char::REPLACEMENT_CHARACTER);
    let mut buffer = [0; 4];
    let text = character.encode_utf8(&mut buffer);
    // SAFETY: `out` is writable.
    unsafe { out.write(StringOut::built(text.as_bytes())) };
}

/// Whether `position` starts a character or equals the length.
///
/// # Safety
/// The string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_is_boundary(data: *const u8, len: i64, position: i64) -> bool {
    // SAFETY: guaranteed by the caller.
    let text = unsafe { super::bytes(data, len) };
    let Ok(position) = usize::try_from(position) else {
        return false;
    };
    position == text.len() || text.get(position).is_some_and(|byte| byte & 0xC0 != 0x80)
}

fn character_at(text: &[u8], position: usize) -> (char, usize) {
    let width = match text.get(position) {
        Some(lead) if lead & 0x80 == 0 => 1,
        Some(lead) if lead & 0xE0 == 0xC0 => 2,
        Some(lead) if lead & 0xF0 == 0xE0 => 3,
        Some(_) => 4,
        None => return (char::REPLACEMENT_CHARACTER, 1),
    };
    text.get(position..position + width)
        .and_then(|piece| std::str::from_utf8(piece).ok())
        .and_then(|piece| piece.chars().next())
        .map_or((char::REPLACEMENT_CHARACTER, 1), |character| {
            (character, width)
        })
}

/// The scalar value that starts at `position`.
///
/// # Safety
/// The string must satisfy the storage rule of `bytes`, and `position` must be a character start.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_rune_at(data: *const u8, len: i64, position: i64) -> u32 {
    // SAFETY: guaranteed by the caller.
    let text = unsafe { super::bytes(data, len) };
    character_at(text, usize::try_from(position).unwrap_or(usize::MAX)).0 as u32
}

/// The byte length of the character that starts at `position`.
///
/// # Safety
/// The string must satisfy the storage rule of `bytes`, and `position` must be a character start.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_char_width(data: *const u8, len: i64, position: i64) -> i64 {
    // SAFETY: guaranteed by the caller.
    let text = unsafe { super::bytes(data, len) };
    character_at(text, usize::try_from(position).unwrap_or(usize::MAX)).1 as i64
}

/// Compares two compiler-produced strings lexicographically by UTF-8 bytes.
///
/// # Safety
/// Each pointer must address its corresponding number of live, initialized
/// bytes, or have length zero. Both allocations must remain immutable here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_compare(
    a: *const u8,
    a_len: i64,
    b: *const u8,
    b_len: i64,
) -> i32 {
    // SAFETY: guaranteed by the generated call's string ABI.
    let (a, b) = unsafe { (super::bytes(a, a_len), super::bytes(b, b_len)) };
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_null_strings_and_embedded_nuls_compare_by_bytes() {
        // SAFETY: empty strings need no allocation; byte literals stay live.
        unsafe {
            assert_eq!(
                zore_string_compare(std::ptr::null(), 0, std::ptr::null(), 0),
                0
            );
            assert_eq!(
                zore_string_compare(std::ptr::null(), 0, b"a".as_ptr(), 1),
                -1
            );
            assert_eq!(
                zore_string_compare(b"a\0b".as_ptr(), 3, b"a\0c".as_ptr(), 3),
                -1
            );
            assert_eq!(zore_string_compare(b"ab".as_ptr(), 2, b"a".as_ptr(), 1), 1);
        }
    }

    unsafe fn joined(a: &str, b: &str) -> String {
        let mut out = StringOut::empty();
        // SAFETY: both strings are live and `out` is writable.
        unsafe {
            zore_string_concat(
                &mut out,
                a.as_ptr(),
                a.len() as i64,
                b.as_ptr(),
                b.len() as i64,
            );
            String::from_utf8(super::super::bytes(out.data, out.len).to_vec()).unwrap()
        }
    }

    #[test]
    fn concatenation_builds_new_text_and_shares_for_empty_operands() {
        // SAFETY: the operands are live string literals.
        unsafe {
            assert_eq!(joined("ab", "cd"), "abcd");
            assert_eq!(joined("", "x"), "x");
            assert_eq!(joined("x", ""), "x");
            assert_eq!(joined("", ""), "");
        }
        release_all();
    }

    #[test]
    fn runes_encode_and_decode_across_widths() {
        for (text, rune, width) in [
            ("a", 'a', 1),
            ("é", 'é', 2),
            ("用", '用', 3),
            ("🦀", '🦀', 4),
        ] {
            let mut out = StringOut::empty();
            // SAFETY: `out` is writable and the text is a live literal.
            unsafe {
                zore_string_from_rune(&mut out, rune as u32);
                assert_eq!(super::super::bytes(out.data, out.len), text.as_bytes());
                assert_eq!(zore_string_rune_at(text.as_ptr(), width, 0), rune as u32);
                assert_eq!(zore_string_char_width(text.as_ptr(), width, 0), width);
            }
        }
        release_all();
    }

    #[test]
    fn boundaries_are_character_starts_and_the_end() {
        let text = "aé";
        // SAFETY: the text is a live literal.
        unsafe {
            let check = |position| zore_string_is_boundary(text.as_ptr(), 3, position);
            assert!(check(0) && check(1) && !check(2) && check(3));
            assert!(!check(4) && !check(-1));
        }
    }
}
