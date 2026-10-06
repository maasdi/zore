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
    use super::zore_string_compare;

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
}
