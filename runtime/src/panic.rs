//! Panic reporting for the initial task of the all-Copy subset.

use std::io::Write;

pub(super) fn fail(message: &[u8]) -> ! {
    // Reporting is best effort: a failed stderr cannot itself be reported.
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(b"panic in the main task: ");
    let _ = stderr.write_all(message);
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
    // No supported Zore value needs destruction yet. This must gain task
    // unwinding before the compiler accepts values requiring drop.
    std::process::exit(2);
}

/// Reports a Zore panic and terminates the initial task's process.
///
/// # Safety
/// `message` must address `len` live, initialized bytes, or be empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_panic(message: *const u8, len: i64) -> ! {
    // SAFETY: guaranteed by the generated call's string ABI.
    fail(unsafe { super::bytes(message, len) });
}
