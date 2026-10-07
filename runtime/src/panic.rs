use std::cell::RefCell;
use std::io::Write;

#[derive(Default)]
pub(super) struct PanicState {
    current: Option<Vec<u8>>,
    suspended: Vec<Option<Vec<u8>>>,
}

thread_local! {
    static STATE: RefCell<PanicState> = RefCell::new(PanicState::default());
}

pub(super) fn fail(message: &[u8]) -> ! {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(b"panic in the main task: ");
    let _ = stderr.write_all(message);
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
    std::process::exit(2);
}

#[inline(never)]
pub(super) fn raise(message: &[u8]) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.current.is_some() {
            std::process::abort();
        }
        state.current = Some(message.to_vec());
    });
}

#[inline(never)]
pub(super) fn take() -> Option<Vec<u8>> {
    STATE.with(|state| state.borrow_mut().current.take())
}

/// A task that moves between threads carries its panic state with it.
#[cfg(all(target_arch = "x86_64", any(target_os = "linux", target_os = "macos")))]
#[inline(never)]
pub(super) fn swap_state(other: &mut PanicState) {
    STATE.with(|state| std::mem::swap(&mut *state.borrow_mut(), other));
}

pub(super) fn report_task(id: u64, message: &[u8]) {
    let mut stderr = std::io::stderr().lock();
    let _ = write!(stderr, "panic in task {id}: ");
    let _ = stderr.write_all(message);
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
}

pub(super) fn finish() {
    STATE.with(|state| {
        if let Some(message) = &state.borrow().current {
            fail(message);
        }
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_raise_panic(message: *const u8, len: i64) {
    raise(unsafe { super::bytes(message, len) });
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_panic_pending() -> bool {
    STATE.with(|state| state.borrow().current.is_some())
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_enter_drop() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let current = state.current.take();
        state.suspended.push(current);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_leave_drop() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let previous = state.suspended.pop().expect("unbalanced drop status");
        if previous.is_some() && state.current.is_some() {
            std::process::abort();
        }
        if previous.is_some() {
            state.current = previous;
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_abort() -> ! {
    std::process::abort();
}
