//! Heap storage for owned dynamic arrays.

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

/// Allocates `bytes` bytes; zero bytes yield a dangling, aligned, non-null
/// pointer that is never dereferenced. Aborts if memory is unavailable.
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
}
