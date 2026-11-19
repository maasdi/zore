use super::*;

pub(super) fn allocate() -> *mut u8 {
    #[cfg(any(miri, not(any(target_os = "linux", target_os = "macos", windows))))]
    return super::super::alloc::alloc_kept(SPAN, PAGE);
    #[cfg(all(not(miri), any(target_os = "linux", target_os = "macos")))]
    let data = unsafe {
        unsafe extern "C" {
            fn mmap(
                addr: *mut u8,
                len: usize,
                prot: i32,
                flags: i32,
                fd: i32,
                offset: i64,
            ) -> *mut u8;
        }
        let anonymous = if cfg!(target_os = "macos") {
            0x1000
        } else {
            0x20
        };
        mmap(std::ptr::null_mut(), SPAN, 3, 2 | anonymous, -1, 0)
    };
    #[cfg(all(not(miri), windows))]
    let data = unsafe { VirtualAlloc(std::ptr::null_mut(), SPAN, 0x3000, 4) };
    #[cfg(all(not(miri), any(target_os = "linux", target_os = "macos", windows)))]
    {
        if data.is_null() || data.addr() == usize::MAX {
            out_of_memory();
        }
        data
    }
}

pub(super) fn discard(base: usize) -> bool {
    #[cfg(miri)]
    {
        // SAFETY: every slot is free and no cache can access this span.
        unsafe { std::ptr::with_exposed_provenance_mut::<u8>(base).write_bytes(0, SPAN) };
        true
    }
    #[cfg(all(not(miri), any(target_os = "linux", target_os = "macos")))]
    {
        let advice = if cfg!(target_os = "macos") { 7 } else { 4 };
        advise(base, advice)
    }
    #[cfg(all(not(miri), windows))]
    {
        // SAFETY: the entire reserved mapping is exclusively free.
        unsafe { VirtualFree(std::ptr::with_exposed_provenance_mut(base), SPAN, 0x4000) != 0 }
    }
    #[cfg(all(not(miri), not(any(target_os = "linux", target_os = "macos", windows))))]
    {
        let _ = base;
        false
    }
}

pub(super) fn commit(base: usize) {
    #[cfg(all(not(miri), target_os = "macos"))]
    let _ = advise(base, 8);
    #[cfg(all(not(miri), windows))]
    if unsafe { VirtualAlloc(std::ptr::with_exposed_provenance_mut(base), SPAN, 0x1000, 4) }
        .is_null()
    {
        out_of_memory();
    }
    #[cfg(not(all(not(miri), any(windows, target_os = "macos"))))]
    let _ = base;
}

#[cfg(all(not(miri), any(target_os = "linux", target_os = "macos")))]
fn advise(base: usize, advice: i32) -> bool {
    unsafe extern "C" {
        fn madvise(addr: *mut u8, len: usize, advice: i32) -> i32;
    }
    // SAFETY: the page-aligned mapping remains reserved under the class lock.
    unsafe { madvise(std::ptr::with_exposed_provenance_mut(base), SPAN, advice) == 0 }
}

#[cfg(all(not(miri), windows))]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(address: *mut u8, size: usize, kind: u32, protect: u32) -> *mut u8;
    fn VirtualFree(address: *mut u8, size: usize, kind: u32) -> i32;
}
