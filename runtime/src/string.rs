use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use super::alloc::{zore_alloc, zore_free};

/// Built text lives in a buffer. No string reads past `used`, so appending at the end
/// never changes the text any existing string shows. The buffer is freed when its last
/// owner lets go.
struct Buffer {
    data: *mut u8,
    capacity: usize,
    used: usize,
    owners: usize,
    /// Held by a package-level value until the program ends, so it is not a leak.
    pinned: bool,
    /// The error a wrapped error's text was made from, and the length of that text.
    cause: Option<Cause>,
}

#[derive(Clone, Copy)]
struct Cause {
    data: usize,
    len: usize,
    text_len: usize,
}

struct Registry(BTreeMap<usize, Buffer>);

// SAFETY: a buffer is only reached through the lock, and its storage is not tied to a thread.
unsafe impl Send for Registry {}

/// Every task shares one table, so text can move between tasks.
static OWNED: Mutex<Registry> = Mutex::new(Registry(BTreeMap::new()));

impl std::ops::Deref for Registry {
    type Target = BTreeMap<usize, Buffer>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Registry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Tests that count buffers cannot overlap with any test that makes text.
#[cfg(test)]
pub(super) fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn owned() -> MutexGuard<'static, Registry> {
    OWNED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Adds a buffer with one owner holding `used` bytes of `capacity`, copied from the given pieces.
fn new_buffer(pieces: &[&[u8]], capacity: usize) -> *mut u8 {
    let data = zore_alloc(capacity as i64);
    let mut used = 0;
    for piece in pieces {
        // SAFETY: `data` has room for every piece and does not overlap them.
        unsafe { std::ptr::copy_nonoverlapping(piece.as_ptr(), data.add(used), piece.len()) };
        used += piece.len();
    }
    owned().insert(
        data as usize,
        Buffer {
            data,
            capacity,
            used,
            owners: 1,
            pinned: false,
            cause: None,
        },
    );
    data
}

/// Where a compiler-produced `string` descriptor is written.
#[repr(C)]
pub struct StringOut {
    pub(super) data: *const u8,
    pub(super) len: i64,
}

impl StringOut {
    pub(super) fn empty() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }

    /// A string that reads `len` bytes of storage that another string already keeps alive.
    pub(super) fn shared(data: *const u8, len: usize) -> Self {
        if len == 0 {
            return Self::empty();
        }
        retain(data, len);
        Self {
            data,
            len: len as i64,
        }
    }

    pub(super) fn built(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::empty();
        }
        Self {
            data: new_buffer(&[bytes], bytes.len()),
            len: bytes.len() as i64,
        }
    }
}

fn with_buffer<R>(data: *const u8, len: usize, act: impl FnOnce(&mut Buffer) -> R) -> Option<R> {
    if len == 0 {
        return None;
    }
    let mut owned = owned();
    let start = data as usize;
    let (_, buffer) = owned.range_mut(..=start).next_back()?;
    (start < buffer.data as usize + buffer.capacity).then(|| act(buffer))
}

/// Another owner now reads these bytes. Static text and empty text have no buffer.
pub(super) fn retain(data: *const u8, len: usize) {
    with_buffer(data, len, |buffer| buffer.owners += 1);
}

/// One owner stops reading these bytes; the last one frees the buffer.
pub(super) fn release(data: *const u8, len: usize) {
    let freed = with_buffer(data, len, |buffer| {
        buffer.owners -= 1;
        (buffer.owners == 0).then_some((buffer.data, buffer.capacity, buffer.cause))
    });
    if let Some(Some((base, capacity, cause))) = freed {
        owned().remove(&(base as usize));
        // SAFETY: the buffer came from `zore_alloc(capacity)` and had no owners left.
        unsafe { zore_free(base, capacity as i64) };
        if let Some(cause) = cause {
            release(cause.data as *const u8, cause.len);
        }
    }
}

/// A package-level value holds this string for the rest of the program.
#[unsafe(no_mangle)]
pub extern "C" fn zore_string_pin(data: *const u8, len: i64) {
    let cause = with_buffer(data, usize::try_from(len).unwrap_or(0), |buffer| {
        buffer.pinned = true;
        buffer.cause
    });
    if let Some(Some(cause)) = cause {
        zore_string_pin(cause.data as *const u8, cause.len as i64);
    }
}

/// New text that remembers the error it wraps; empty text cannot remember one.
pub(super) fn with_cause(text: &[u8], cause: *const u8, cause_len: usize) -> StringOut {
    let built = StringOut::built(text);
    if text.is_empty() || cause_len == 0 {
        return built;
    }
    retain(cause, cause_len);
    with_buffer(built.data, text.len(), |buffer| {
        buffer.cause = Some(Cause {
            data: cause as usize,
            len: cause_len,
            text_len: text.len(),
        });
    });
    built
}

/// The text of the error that this exact text wraps, with a new owner.
pub(super) fn cause_of(data: *const u8, len: usize) -> Option<(*const u8, usize)> {
    let cause = with_buffer(data, len, |buffer| {
        buffer
            .cause
            .filter(|cause| std::ptr::eq(buffer.data, data) && cause.text_len == len)
    })??;
    retain(cause.data as *const u8, cause.len);
    Some((cause.data as *const u8, cause.len))
}

/// Another owner now reads this string.
#[unsafe(no_mangle)]
pub extern "C" fn zore_string_retain(data: *const u8, len: i64) {
    retain(data, usize::try_from(len).unwrap_or(0));
}

/// One owner stops reading this string.
#[unsafe(no_mangle)]
pub extern "C" fn zore_string_release(data: *const u8, len: i64) {
    release(data, usize::try_from(len).unwrap_or(0));
}

pub(super) fn live_buffers() -> usize {
    owned().values().filter(|buffer| !buffer.pinned).count()
}

pub(super) fn release_all() {
    let buffers = std::mem::take(&mut owned().0);
    for buffer in buffers.into_values() {
        // SAFETY: each buffer came from `zore_alloc(capacity)` and is released once.
        unsafe { zore_free(buffer.data, buffer.capacity as i64) };
    }
}

/// How `left + right` can reuse the buffer that holds `left`.
enum Append {
    /// `left` ends where its buffer's text ends, and there is room.
    InPlace,
    /// `left` ends there, but the buffer is full: build a larger one.
    Grow,
    /// `left` is not at the end of a buffer: build exactly enough.
    Copy,
}

fn plan_append(left: *const u8, left_len: usize, right_len: usize) -> Append {
    let mut owned = owned();
    let start = left as usize;
    let Some((_, buffer)) = owned.range_mut(..=start).next_back() else {
        return Append::Copy;
    };
    let base = buffer.data as usize;
    if start >= base + buffer.capacity || start + left_len != base + buffer.used {
        return Append::Copy;
    }
    if buffer.used + right_len <= buffer.capacity {
        buffer.used += right_len;
        buffer.owners += 1;
        Append::InPlace
    } else {
        Append::Grow
    }
}

/// The two strings joined, with one owner for the caller; an empty operand shares the other's storage.
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
        retain(a, left.len());
        StringOut {
            data: a,
            len: a_len,
        }
    } else if left.is_empty() {
        retain(b, right.len());
        StringOut {
            data: b,
            len: b_len,
        }
    } else {
        let total = left.len() + right.len();
        match plan_append(a, left.len(), right.len()) {
            Append::InPlace => {
                // SAFETY: the buffer has room after `left`, which no string reads past.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        right.as_ptr(),
                        (a as *mut u8).add(left.len()),
                        right.len(),
                    )
                };
                StringOut {
                    data: a,
                    len: total as i64,
                }
            }
            Append::Grow => StringOut {
                data: new_buffer(&[left, right], total * 2),
                len: total as i64,
            },
            Append::Copy => StringOut {
                data: new_buffer(&[left, right], total),
                len: total as i64,
            },
        }
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

pub(super) fn character_at(text: &[u8], position: usize) -> (char, usize) {
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
        let _serial = crate::string::serial();
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
        let _serial = crate::string::serial();
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
        let _serial = crate::string::serial();
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
        let _serial = crate::string::serial();
        let text = "aé";
        // SAFETY: the text is a live literal.
        unsafe {
            let check = |position| zore_string_is_boundary(text.as_ptr(), 3, position);
            assert!(check(0) && check(1) && !check(2) && check(3));
            assert!(!check(4) && !check(-1));
        }
    }

    fn buffer_count() -> usize {
        live_buffers()
    }

    fn append(left: &StringOut, right: &str) -> StringOut {
        let mut out = StringOut::empty();
        // SAFETY: `left` is live, `right` is a literal, and `out` is writable.
        unsafe {
            zore_string_concat(
                &mut out,
                left.data,
                left.len,
                right.as_ptr(),
                right.len() as i64,
            )
        };
        out
    }

    fn read(text: &StringOut) -> String {
        // SAFETY: the text is live until the buffers are released.
        unsafe { String::from_utf8(super::super::bytes(text.data, text.len).to_vec()).unwrap() }
    }

    #[test]
    fn appending_at_the_end_reuses_the_buffer_and_leaves_older_text_alone() {
        let _serial = crate::string::serial();
        release_all();
        let ab = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let abc = append(&ab, "c");
        let abcd = append(&abc, "d");
        assert_eq!(abcd.data, abc.data, "the tail grows in place");
        let abx = append(&ab, "x");
        let abcy = append(&abc, "y");
        assert_ne!(abx.data, ab.data);
        assert_ne!(
            abcy.data, abc.data,
            "a text that is not at the end is copied"
        );
        assert_eq!(
            [read(&ab), read(&abc), read(&abcd), read(&abx), read(&abcy)],
            ["ab", "abc", "abcd", "abx", "abcy"]
        );
        release_all();
    }

    #[test]
    fn a_long_chain_of_appends_uses_few_buffers() {
        let _serial = crate::string::serial();
        release_all();
        let mut text = StringOut::shared(b"x".as_ptr(), 1);
        for _ in 0..10_000 {
            text = append(&text, "y");
        }
        assert_eq!(text.len, 10_001);
        assert!(buffer_count() <= 16, "{} buffers", buffer_count());
        release_all();
    }

    #[test]
    fn appending_a_text_to_itself_and_to_a_suffix_is_correct() {
        let _serial = crate::string::serial();
        release_all();
        let ab = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let abab = append(&ab, "ab");
        let doubled = {
            let mut out = StringOut::empty();
            // SAFETY: both operands are live and `out` is writable.
            unsafe { zore_string_concat(&mut out, abab.data, abab.len, abab.data, abab.len) };
            out
        };
        assert_eq!(read(&doubled), "abababab");
        let tail = StringOut::shared(unsafe { doubled.data.add(4) }, 4);
        assert_eq!(read(&append(&tail, "!")), "abab!");
        assert_eq!(read(&doubled), "abababab");
        release_all();
    }

    #[test]
    fn the_last_owner_frees_a_buffer_and_static_or_empty_text_has_none() {
        let _serial = crate::string::serial();
        let built = StringOut::built(b"hello");
        assert_eq!(live_buffers(), 1);
        let piece = StringOut::shared(unsafe { built.data.add(1) }, 3);
        retain(built.data, 5);
        release(built.data, 5);
        release(built.data, 5);
        assert_eq!(live_buffers(), 1, "the shared piece still owns the buffer");
        assert_eq!(read(&piece), "ell");
        release(piece.data, 3);
        assert_eq!(live_buffers(), 0);
        retain(b"static".as_ptr(), 6);
        release(b"static".as_ptr(), 6);
        retain(std::ptr::null(), 0);
        release(std::ptr::null(), 0);
        assert_eq!(live_buffers(), 0);
    }

    #[test]
    fn every_concatenation_result_has_its_own_owner() {
        let _serial = crate::string::serial();
        let ab = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let abc = append(&ab, "c");
        let same = append(&abc, "");
        assert_eq!(same.data, abc.data);
        release(ab.data, 2);
        release(abc.data, 3);
        assert_eq!(live_buffers(), 1, "the unchanged text still owns it");
        release(same.data, 3);
        assert_eq!(live_buffers(), 0);
    }

    #[test]
    fn a_text_that_ends_a_buffer_is_still_readable_after_the_older_owners_leave() {
        let _serial = crate::string::serial();
        let ab = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let abcd = append(&append(&ab, "c"), "d");
        release(ab.data, 2);
        assert_eq!(read(&abcd), "abcd");
        assert!(live_buffers() >= 1);
        release_all();
    }
}
