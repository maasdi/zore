use std::alloc::{Layout, alloc, dealloc};
use std::io::Write;

/// Every allocation uses this alignment, which covers all element types.
const ALIGN: usize = 16;

#[cfg(any(test, zore_test_hooks))]
static LIVE_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(any(test, zore_test_hooks))]
pub(super) fn live_bytes() -> usize {
    LIVE_BYTES.load(std::sync::atomic::Ordering::Relaxed)
}

pub(super) fn out_of_memory() -> ! {
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
    alloc_layout(layout, false, true)
}

/// Storage for `bytes` bytes aligned to `align`; aborts if memory is unavailable.
pub(super) fn alloc_aligned(bytes: usize, align: usize) -> *mut u8 {
    alloc_layout(aligned_layout(bytes, align), false, true)
}

/// Zeroed storage that stays for the rest of the program, outside the live byte count.
pub(super) fn alloc_kept(bytes: usize, align: usize) -> *mut u8 {
    alloc_layout(aligned_layout(bytes, align), true, false)
}

fn aligned_layout(bytes: usize, align: usize) -> Layout {
    match Layout::from_size_align(bytes, align) {
        Ok(layout) if layout.size() > 0 => layout,
        _ => out_of_memory(),
    }
}

fn alloc_layout(layout: Layout, zeroed: bool, counted: bool) -> *mut u8 {
    // SAFETY: `layout` has a nonzero size.
    let data = unsafe {
        if zeroed {
            std::alloc::alloc_zeroed(layout)
        } else {
            alloc(layout)
        }
    };
    if data.is_null() {
        out_of_memory();
    }
    #[cfg(any(test, zore_test_hooks))]
    if counted {
        LIVE_BYTES.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(not(any(test, zore_test_hooks)))]
    let _ = counted;
    data
}

pub(super) fn count_live(bytes: usize, live: bool) {
    #[cfg(any(test, zore_test_hooks))]
    if live {
        LIVE_BYTES.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
    } else {
        LIVE_BYTES.fetch_sub(bytes, std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(not(any(test, zore_test_hooks)))]
    let _ = (bytes, live);
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
        unsafe { free_layout(data, layout) };
    }
}

/// Frees storage from `alloc_aligned(bytes, align)`.
///
/// # Safety
/// `data` must come from `alloc_aligned(bytes, align)` and must not be used afterwards.
pub(super) unsafe fn free_aligned(data: *mut u8, bytes: usize, align: usize) {
    if let Ok(layout) = Layout::from_size_align(bytes, align) {
        // SAFETY: the caller passes storage allocated with this same layout.
        unsafe { free_layout(data, layout) };
    }
}

/// # Safety
/// `data` must come from `alloc_layout(layout)` and must not be used afterwards.
unsafe fn free_layout(data: *mut u8, layout: Layout) {
    // SAFETY: guaranteed by the caller.
    unsafe { dealloc(data, layout) };
    #[cfg(any(test, zore_test_hooks))]
    LIVE_BYTES.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
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
