//! The code behind `zore/strings`. Results share storage with their arguments when they can.

use super::alloc::zore_alloc;
use super::string::{StringOut, character_at};
use super::sys::{ByteArray, StringError};

/// Where an `Array<string>` descriptor is written.
#[repr(C)]
pub struct ArrayOut {
    data: *mut StringOut,
    len: i64,
    cap: i64,
}

/// # Safety
/// The string must satisfy the storage rule of `bytes`.
unsafe fn text<'a>(data: *const u8, len: i64) -> &'a [u8] {
    // SAFETY: guaranteed by the caller.
    unsafe { super::bytes(data, len) }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn lossy(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

/// # Safety
/// Each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_contains(
    s: *const u8,
    s_len: i64,
    sub: *const u8,
    sub_len: i64,
) -> bool {
    // SAFETY: guaranteed by the caller.
    let (s, sub) = unsafe { (text(s, s_len), text(sub, sub_len)) };
    find(s, sub).is_some()
}

/// # Safety
/// Each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_has_prefix(
    s: *const u8,
    s_len: i64,
    prefix: *const u8,
    prefix_len: i64,
) -> bool {
    // SAFETY: guaranteed by the caller.
    let (s, prefix) = unsafe { (text(s, s_len), text(prefix, prefix_len)) };
    s.starts_with(prefix)
}

/// # Safety
/// Each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_has_suffix(
    s: *const u8,
    s_len: i64,
    suffix: *const u8,
    suffix_len: i64,
) -> bool {
    // SAFETY: guaranteed by the caller.
    let (s, suffix) = unsafe { (text(s, s_len), text(suffix, suffix_len)) };
    s.ends_with(suffix)
}

/// # Safety
/// Each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_index(
    s: *const u8,
    s_len: i64,
    sub: *const u8,
    sub_len: i64,
) -> i64 {
    // SAFETY: guaranteed by the caller.
    let (s, sub) = unsafe { (text(s, s_len), text(sub, sub_len)) };
    find(s, sub).map_or(-1, |position| position as i64)
}

/// # Safety
/// Each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_last_index(
    s: *const u8,
    s_len: i64,
    sub: *const u8,
    sub_len: i64,
) -> i64 {
    // SAFETY: guaranteed by the caller.
    let (s, sub) = unsafe { (text(s, s_len), text(sub, sub_len)) };
    if sub.is_empty() {
        return s.len() as i64;
    }
    s.windows(sub.len())
        .rposition(|window| window == sub)
        .map_or(-1, |position| position as i64)
}

/// # Safety
/// Each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_count(
    s: *const u8,
    s_len: i64,
    sub: *const u8,
    sub_len: i64,
) -> i64 {
    // SAFETY: guaranteed by the caller.
    let (s, sub) = unsafe { (text(s, s_len), text(sub, sub_len)) };
    if sub.is_empty() {
        return lossy(s).chars().count() as i64 + 1;
    }
    let mut count = 0;
    let mut start = 0;
    while let Some(found) = find(&s[start..], sub) {
        count += 1;
        start += found + sub.len();
    }
    count
}

/// # Safety
/// Each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_equal_fold(
    s: *const u8,
    s_len: i64,
    t: *const u8,
    t_len: i64,
) -> bool {
    // SAFETY: guaranteed by the caller.
    let (s, t) = unsafe { (lossy(text(s, s_len)), lossy(text(t, t_len))) };
    let mut left = s.chars();
    let mut right = t.chars();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a == b || a.to_lowercase().eq(b.to_lowercase()) => {}
            _ => return false,
        }
    }
}

#[derive(Clone, Copy)]
enum Ends {
    Both,
    Left,
    Right,
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
unsafe fn trim_set(out: *mut StringOut, s: *const u8, s_len: i64, set: &[u8], ends: Ends) {
    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { text(s, s_len) };
    let set = lossy(set);
    let result = match std::str::from_utf8(bytes) {
        Ok(valid) => {
            let cut = |c: char| set.contains(c);
            let inner = match ends {
                Ends::Both => valid.trim_matches(cut),
                Ends::Left => valid.trim_start_matches(cut),
                Ends::Right => valid.trim_end_matches(cut),
            };
            let start = inner.as_ptr() as usize - bytes.as_ptr() as usize;
            // SAFETY: `inner` lies inside `bytes`.
            StringOut::shared(unsafe { s.add(start) }, inner.len())
        }
        Err(_) => StringOut::shared(s, bytes.len()),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_trim(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
    set: *const u8,
    set_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    unsafe { trim_set(out, s, s_len, text(set, set_len), Ends::Both) };
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_trim_left(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
    set: *const u8,
    set_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    unsafe { trim_set(out, s, s_len, text(set, set_len), Ends::Left) };
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_trim_right(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
    set: *const u8,
    set_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    unsafe { trim_set(out, s, s_len, text(set, set_len), Ends::Right) };
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_replace(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
    old: *const u8,
    old_len: i64,
    new: *const u8,
    new_len: i64,
    n: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (s, old, new) = unsafe { (text(s, s_len), text(old, old_len), text(new, new_len)) };
    let (s, old, new) = (lossy(s), lossy(old), lossy(new));
    let replaced = match usize::try_from(n) {
        Ok(count) => s.replacen(old.as_ref(), new.as_ref(), count),
        Err(_) => s.replace(old.as_ref(), new.as_ref()),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringOut::built(replaced.as_bytes())) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_to_upper(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let upper = lossy(unsafe { text(s, s_len) }).to_uppercase();
    unsafe { out.write(StringOut::built(upper.as_bytes())) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_to_lower(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let lower = lossy(unsafe { text(s, s_len) }).to_lowercase();
    unsafe { out.write(StringOut::built(lower.as_bytes())) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_trim_space(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { text(s, s_len) };
    let trimmed = match std::str::from_utf8(bytes) {
        Ok(valid) => {
            let inner = valid.trim();
            let start = inner.as_ptr() as usize - bytes.as_ptr() as usize;
            StringOut::shared(unsafe { s.add(start) }, inner.len())
        }
        Err(_) => StringOut::shared(s, bytes.len()),
    };
    unsafe { out.write(trimmed) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_repeat(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
    count: i64,
) {
    // SAFETY: guaranteed by the caller.
    let piece = unsafe { text(s, s_len) };
    let result = if count < 0 {
        super::panic::raise(b"strings.Repeat: negative count");
        StringOut::empty()
    } else {
        match piece.len().checked_mul(count as usize) {
            Some(total) if total <= isize::MAX as usize => {
                StringOut::built(&piece.repeat(count as usize))
            }
            _ => {
                super::panic::raise(b"strings.Repeat: result too large");
                StringOut::empty()
            }
        }
    };
    unsafe { out.write(result) };
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_replace_all(
    out: *mut StringOut,
    s: *const u8,
    s_len: i64,
    old: *const u8,
    old_len: i64,
    new: *const u8,
    new_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (s, old, new) = unsafe { (text(s, s_len), text(old, old_len), text(new, new_len)) };
    let replaced = lossy(s).replace(lossy(old).as_ref(), lossy(new).as_ref());
    unsafe { out.write(StringOut::built(replaced.as_bytes())) };
}

/// The `(start, length)` pieces of `whole` between occurrences of `sep`; with a positive
/// `limit`, at most that many, the last holding the rest.
fn split_pieces(whole: &[u8], sep: &[u8], limit: Option<usize>) -> Vec<(usize, usize)> {
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let room = |pieces: &Vec<(usize, usize)>| limit.is_none_or(|limit| pieces.len() + 1 < limit);
    if sep.is_empty() {
        let mut position = 0;
        while position < whole.len() {
            if !room(&pieces) {
                pieces.push((position, whole.len() - position));
                break;
            }
            let width = character_at(whole, position).1;
            pieces.push((position, width));
            position += width;
        }
    } else {
        let mut start = 0;
        while room(&pieces)
            && let Some(found) = find(&whole[start..], sep)
        {
            pieces.push((start, found));
            start += found + sep.len();
        }
        pieces.push((start, whole.len() - start));
    }
    pieces
}

/// # Safety
/// Each piece must lie inside the string at `s`, which must satisfy the storage rule of `bytes`.
unsafe fn shared_pieces(s: *const u8, pieces: &[(usize, usize)]) -> ArrayOut {
    if pieces.is_empty() {
        return ArrayOut {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
    }
    let data =
        zore_alloc((pieces.len() * std::mem::size_of::<StringOut>()) as i64) as *mut StringOut;
    for (index, &(start, len)) in pieces.iter().enumerate() {
        // SAFETY: `data` has room for every piece, and each piece lies inside the string.
        unsafe { data.add(index).write(StringOut::shared(s.add(start), len)) };
    }
    ArrayOut {
        data,
        len: pieces.len() as i64,
        cap: pieces.len() as i64,
    }
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_split(
    out: *mut ArrayOut,
    s: *const u8,
    s_len: i64,
    sep: *const u8,
    sep_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (whole, sep) = unsafe { (text(s, s_len), text(sep, sep_len)) };
    let pieces = split_pieces(whole, sep, None);
    // SAFETY: the pieces lie inside `s`, and `out` is writable.
    unsafe { out.write(shared_pieces(s, &pieces)) };
}

/// # Safety
/// `out` must be writable and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_split_n(
    out: *mut ArrayOut,
    s: *const u8,
    s_len: i64,
    sep: *const u8,
    sep_len: i64,
    n: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (whole, sep) = unsafe { (text(s, s_len), text(sep, sep_len)) };
    let pieces = match usize::try_from(n) {
        Ok(0) => Vec::new(),
        Ok(limit) => split_pieces(whole, sep, Some(limit)),
        Err(_) => split_pieces(whole, sep, None),
    };
    // SAFETY: the pieces lie inside `s`, and `out` is writable.
    unsafe { out.write(shared_pieces(s, &pieces)) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_fields(out: *mut ArrayOut, s: *const u8, s_len: i64) {
    // SAFETY: guaranteed by the caller.
    let whole = unsafe { text(s, s_len) };
    let valid = lossy(whole);
    let pieces: Vec<(usize, usize)> = valid
        .split(char::is_whitespace)
        .filter(|piece| !piece.is_empty())
        .map(|piece| {
            (
                piece.as_ptr() as usize - valid.as_ptr() as usize,
                piece.len(),
            )
        })
        .collect();
    // SAFETY: valid UTF-8 is borrowed unchanged, so the pieces lie inside `s`.
    unsafe { out.write(shared_pieces(s, &pieces)) };
}

/// # Safety
/// `out` must be writable; `parts` must address `count` strings; each must satisfy the
/// storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_join(
    out: *mut StringOut,
    parts: *const StringOut,
    count: i64,
    sep: *const u8,
    sep_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let sep = unsafe { text(sep, sep_len) };
    let parts: &[StringOut] = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(parts, count as usize) }
    };
    let joined = match parts {
        [] => StringOut::empty(),
        [only] => StringOut::shared(only.data, only.len as usize),
        _ => {
            let mut bytes = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                if index > 0 {
                    bytes.extend_from_slice(sep);
                }
                bytes.extend_from_slice(unsafe { text(part.data, part.len) });
            }
            StringOut::built(&bytes)
        }
    };
    unsafe { out.write(joined) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_bytes(out: *mut ByteArray, s: *const u8, s_len: i64) {
    // SAFETY: guaranteed by the caller.
    let whole = unsafe { text(s, s_len) };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ByteArray::copy_of(whole)) };
}

/// # Safety
/// `out` must be writable and `data` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_strings_from_bytes(
    out: *mut StringError,
    data: *const u8,
    len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { text(data, len) };
    let result = match std::str::from_utf8(bytes) {
        Ok(valid) => StringError::ok(valid),
        Err(_) => StringError::failed("strings.FromBytes: invalid UTF-8"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn read(out: &StringOut) -> String {
        // SAFETY: the test strings are live.
        unsafe { String::from_utf8(text(out.data, out.len).to_vec()).unwrap() }
    }

    fn split(s: &str, sep: &str) -> Vec<String> {
        let mut out = ArrayOut {
            data: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
        // SAFETY: both strings are live and `out` is writable.
        unsafe {
            zore_native_strings_split(
                &mut out,
                s.as_ptr(),
                s.len() as i64,
                sep.as_ptr(),
                sep.len() as i64,
            );
            (0..out.len as usize)
                .map(|index| read(&*out.data.add(index)))
                .collect()
        }
    }

    #[test]
    fn splitting_follows_go_for_empty_inputs() {
        let _serial = crate::string::serial();
        assert_eq!(split("a,b,c", ","), ["a", "b", "c"]);
        assert_eq!(split("", ","), [""]);
        assert_eq!(split("abc", ""), ["a", "b", "c"]);
        assert_eq!(split("é用", ""), ["é", "用"]);
        assert!(split("", "").is_empty());
        assert_eq!(split(",a,", ","), ["", "a", ""]);
        assert_eq!(split("a--b", "--"), ["a", "b"]);
    }

    #[test]
    fn searching_and_affixes() {
        let _serial = crate::string::serial();
        // SAFETY: the strings are live literals.
        unsafe {
            let (s, sub) = ("hello", "ell");
            assert!(zore_native_strings_contains(s.as_ptr(), 5, sub.as_ptr(), 3));
            assert!(zore_native_strings_contains(
                s.as_ptr(),
                5,
                std::ptr::null(),
                0
            ));
            assert_eq!(zore_native_strings_index(s.as_ptr(), 5, sub.as_ptr(), 3), 1);
            assert_eq!(
                zore_native_strings_index(s.as_ptr(), 5, b"z".as_ptr(), 1),
                -1
            );
            assert_eq!(
                zore_native_strings_index(s.as_ptr(), 5, std::ptr::null(), 0),
                0
            );
            assert!(zore_native_strings_has_prefix(
                s.as_ptr(),
                5,
                b"he".as_ptr(),
                2
            ));
            assert!(!zore_native_strings_has_prefix(
                s.as_ptr(),
                5,
                b"lo".as_ptr(),
                2
            ));
            assert!(zore_native_strings_has_suffix(
                s.as_ptr(),
                5,
                b"lo".as_ptr(),
                2
            ));
        }
    }

    #[test]
    fn case_trim_replace_repeat_and_join() {
        let _serial = crate::string::serial();
        let mut out = StringOut::empty();
        // SAFETY: every string is a live literal and `out` is writable.
        unsafe {
            zore_native_strings_to_upper(&mut out, "héllo".as_ptr(), 6);
            assert_eq!(read(&out), "HÉLLO");
            zore_native_strings_to_lower(&mut out, "HÉLLO".as_ptr(), 6);
            assert_eq!(read(&out), "héllo");
            let padded = "  a b \n";
            zore_native_strings_trim_space(&mut out, padded.as_ptr(), padded.len() as i64);
            assert_eq!(read(&out), "a b");
            zore_native_strings_trim_space(&mut out, b"   ".as_ptr(), 3);
            assert_eq!(read(&out), "");
            zore_native_strings_replace_all(
                &mut out,
                b"aaa".as_ptr(),
                3,
                b"a".as_ptr(),
                1,
                b"bc".as_ptr(),
                2,
            );
            assert_eq!(read(&out), "bcbcbc");
            zore_native_strings_replace_all(
                &mut out,
                b"ab".as_ptr(),
                2,
                std::ptr::null(),
                0,
                b"-".as_ptr(),
                1,
            );
            assert_eq!(read(&out), "-a-b-");
            zore_native_strings_repeat(&mut out, b"ab".as_ptr(), 2, 3);
            assert_eq!(read(&out), "ababab");
            zore_native_strings_repeat(&mut out, b"ab".as_ptr(), 2, 0);
            assert_eq!(read(&out), "");
            let parts = [
                StringOut::shared(b"x".as_ptr(), 1),
                StringOut::shared(b"yz".as_ptr(), 2),
            ];
            zore_native_strings_join(&mut out, parts.as_ptr(), 2, b", ".as_ptr(), 2);
            assert_eq!(read(&out), "x, yz");
            zore_native_strings_join(&mut out, parts.as_ptr(), 1, b", ".as_ptr(), 2);
            assert_eq!(read(&out), "x");
            zore_native_strings_join(&mut out, parts.as_ptr(), 0, b", ".as_ptr(), 2);
            assert_eq!(read(&out), "");
        }
        super::super::string::release_all();
    }
}
