use std::cell::RefCell;
use std::io::Write;

#[derive(Default)]
pub(super) struct PanicState {
    current: Option<Vec<u8>>,
    suspended: Vec<Option<Vec<u8>>>,
    task: Option<u64>,
}

impl PanicState {
    pub(super) fn for_task(id: u64) -> Self {
        Self {
            task: Some(id),
            ..Self::default()
        }
    }

    /// Fresh state for work done on the current task's behalf on another thread.
    pub(super) fn for_current_task() -> Self {
        Self {
            task: STATE.with(|state| state.borrow().task),
            ..Self::default()
        }
    }
}

thread_local! {
    static STATE: RefCell<PanicState> = RefCell::new(PanicState::default());
}

pub(super) fn fail(message: &[u8]) -> ! {
    report(None, message);
    std::process::exit(2);
}

fn report(task: Option<u64>, message: &[u8]) {
    let mut stderr = std::io::stderr().lock();
    let _ = match task {
        Some(id) => write!(stderr, "panic in task {id}: "),
        None => stderr.write_all(b"panic in the main task: "),
    };
    let _ = stderr.write_all(message);
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
}

pub(super) fn deadlock() -> ! {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(
        b"fatal error: all tasks are asleep, waiting on each other with nothing to wake them\n",
    );
    let _ = stderr.flush();
    std::process::exit(2);
}

/// Reports the panic now, before unwinding releases anything another task could observe.
#[inline(never)]
pub(super) fn raise(message: &[u8]) {
    let task = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.current.is_some() {
            std::process::abort();
        }
        state.current = Some(message.to_vec());
        state.task
    });
    report(task, message);
}

#[inline(never)]
pub(super) fn take() -> Option<Vec<u8>> {
    STATE.with(|state| state.borrow_mut().current.take())
}

/// Takes on a panic raised on this task's behalf elsewhere, keeping its own state otherwise.
pub(super) fn adopt(other: PanicState) {
    let Some(message) = other.current else {
        return;
    };
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.current.is_some() {
            std::process::abort();
        }
        state.current = Some(message);
    });
}

/// A task that moves between threads carries its panic state with it.
#[inline(never)]
pub(super) fn swap_state(other: &mut PanicState) {
    STATE.with(|state| std::mem::swap(&mut *state.borrow_mut(), other));
}

pub(super) fn finish() {
    if zore_panic_pending() {
        std::process::exit(2);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_raise_panic(message: *const u8, len: i64) {
    raise(unsafe { super::bytes(message, len) });
}

/// Raises `message at location`, the form of a `panic` call in source.
///
/// # Safety
/// Both texts must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_raise_panic_at(
    message: *const u8,
    len: i64,
    location: *const u8,
    location_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (message, location) = unsafe {
        (
            super::bytes(message, len),
            super::bytes(location, location_len),
        )
    };
    let mut text = Vec::with_capacity(message.len() + location.len() + 4);
    text.extend_from_slice(message);
    text.extend_from_slice(b" at ");
    text.extend_from_slice(location);
    raise(&text);
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
