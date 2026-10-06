use std::alloc::{Layout, alloc, dealloc};
use std::io::Write;

/// Every allocation uses this alignment, which covers all element types.
const ALIGN: usize = 16;

fn out_of_memory() -> ! {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(b"fatal: out of memory\n");
    let _ = stderr.flush();
    std::process::abort();
}

fn layout(bytes: i64) -> Option<Layout> {
    let size = usize::try_from(bytes).ok().filter(|&size| size > 0)?;
    Layout::from_size_align(size, ALIGN).ok()
}

/// Zero bytes yield a dangling, aligned pointer; aborts if memory is unavailable.
#[unsafe(no_mangle)]
pub extern "C" fn zore_alloc(bytes: i64) -> *mut u8 {
    if bytes == 0 {
        return std::ptr::without_provenance_mut(ALIGN);
    }
    let Some(layout) = layout(bytes) else {
        out_of_memory();
    };
    // SAFETY: `layout` has a nonzero size.
    let data = unsafe { alloc(layout) };
    if data.is_null() {
        out_of_memory();
    }
    data
}

/// Frees storage from `zore_alloc(bytes)`; null or zero-byte storage is a no-op.
///
/// # Safety
/// A nonnull `data` with nonzero `bytes` must come from `zore_alloc(bytes)`
/// and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_free(data: *mut u8, bytes: i64) {
    if data.is_null() {
        return;
    }
    if let Some(layout) = layout(bytes) {
        // SAFETY: the caller passes storage allocated with this same layout.
        unsafe { dealloc(data, layout) };
    }
}

/// Moves storage from `zore_alloc(old_bytes)` to a new `new_bytes` allocation.
///
/// # Safety
/// `data` must come from `zore_alloc(old_bytes)` and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_realloc(data: *mut u8, old_bytes: i64, new_bytes: i64) -> *mut u8 {
    let grown = zore_alloc(new_bytes);
    let kept = usize::try_from(old_bytes.min(new_bytes)).unwrap_or(0);
    if kept > 0 {
        // SAFETY: both allocations hold at least `kept` bytes and are distinct.
        unsafe { std::ptr::copy_nonoverlapping(data, grown, kept) };
    }
    // SAFETY: guaranteed by the caller.
    unsafe { zore_free(data, old_bytes) };
    grown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_round_trip_and_empty_storage_needs_no_free() {
        let data = zore_alloc(24);
        assert!(!data.is_null());
        assert_eq!(data as usize % ALIGN, 0);
        // SAFETY: `data` holds 24 writable bytes until freed below.
        unsafe {
            data.write_bytes(7, 24);
            assert_eq!(*data.add(23), 7);
            zore_free(data, 24);
        }
        let empty = zore_alloc(0);
        assert!(!empty.is_null());
        // SAFETY: zero-byte and null storage are documented no-ops.
        unsafe {
            zore_free(empty, 0);
            zore_free(std::ptr::null_mut(), 0);
        }
    }

    #[test]
    fn reallocation_keeps_the_prefix() {
        let data = zore_alloc(8);
        // SAFETY: `data` holds 8 writable bytes; `zore_realloc` takes ownership.
        unsafe {
            data.write_bytes(3, 8);
            let grown = zore_realloc(data, 8, 32);
            assert_eq!(*grown.add(7), 3);
            zore_free(grown, 32);
            let fresh = zore_realloc(std::ptr::null_mut(), 0, 16);
            zore_free(fresh, 16);
        }
    }
}
