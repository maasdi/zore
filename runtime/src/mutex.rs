use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as Lock, MutexGuard};

use super::alloc::{zore_alloc, zore_free};
use super::fiber::Slot;

type Destroy = unsafe extern "C" fn(*mut u8);

static LIVE: AtomicUsize = AtomicUsize::new(0);

pub(super) fn live_mutexes() -> usize {
    LIVE.load(Ordering::SeqCst)
}

#[derive(Default)]
struct State {
    locked: bool,
    poisoned: bool,
    waiters: VecDeque<Arc<Slot>>,
}

/// The shared cell behind every `Mutex<T>` handle: one value and the lock that guards it.
pub struct Cell {
    size: usize,
    destroy: Option<Destroy>,
    value: *mut u8,
    state: Lock<State>,
}

// SAFETY: the value is only reached by the task that holds the lock.
unsafe impl Send for Cell {}
// SAFETY: as above; the state is behind its own lock.
unsafe impl Sync for Cell {}

impl Cell {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for Cell {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
        if let Some(destroy) = self.destroy {
            // SAFETY: the cell holds one live value of the type `destroy` handles.
            unsafe { destroy(self.value) };
        }
        // SAFETY: the storage came from `zore_alloc(size)`.
        unsafe { zore_free(self.value, self.size as i64) };
    }
}

const ZERO: &[u8] = b"withLock on a zero-value mutex";
const POISONED: &[u8] = b"withLock on a poisoned mutex: a task panicked while holding its lock";

/// A mutex that owns a copy of the `size` bytes at `value`; `destroy` drops one such value.
///
/// # Safety
/// `value` must point to `size` readable bytes holding a live value, which the mutex takes over.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_mutex_new(
    size: i64,
    destroy: Option<Destroy>,
    value: *const u8,
) -> *const Cell {
    let size = usize::try_from(size).unwrap_or(0);
    let storage = zore_alloc(size as i64);
    // SAFETY: both ranges hold `size` bytes and do not overlap.
    unsafe { std::ptr::copy_nonoverlapping(value, storage, size) };
    LIVE.fetch_add(1, Ordering::SeqCst);
    Arc::into_raw(Arc::new(Cell {
        size,
        destroy,
        value: storage,
        state: Lock::new(State::default()),
    }))
}

/// Another handle now refers to the mutex.
///
/// # Safety
/// `cell` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_mutex_retain(cell: *const Cell) {
    if !cell.is_null() {
        // SAFETY: guaranteed by the caller.
        unsafe { Arc::increment_strong_count(cell) };
    }
}

/// A handle is gone; the last one drops the guarded value.
///
/// # Safety
/// `cell` must be null or a live handle that is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_mutex_release(cell: *const Cell) {
    if !cell.is_null() {
        // SAFETY: guaranteed by the caller.
        unsafe { Arc::decrement_strong_count(cell) };
    }
}

/// Waits for the lock and returns the guarded value's address, or raises a panic and returns
/// null for a zero-value or poisoned mutex.
///
/// # Safety
/// `cell` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_mutex_lock(cell: *const Cell) -> *mut u8 {
    // SAFETY: guaranteed by the caller.
    let Some(cell) = (unsafe { cell.as_ref() }) else {
        super::panic::raise(ZERO);
        return std::ptr::null_mut();
    };
    let mut state = cell.lock();
    if state.poisoned {
        drop(state);
        super::panic::raise(POISONED);
        return std::ptr::null_mut();
    }
    if !state.locked {
        state.locked = true;
        return cell.value;
    }
    let slot = Arc::new(Slot::internal());
    state.waiters.push_back(Arc::clone(&slot));
    drop(state);
    slot.park();
    // The lock was handed to this task, unless the mutex was poisoned meanwhile.
    if cell.lock().poisoned {
        super::panic::raise(POISONED);
        return std::ptr::null_mut();
    }
    cell.value
}

/// Releases the lock to the next waiter. A poisoned release marks the mutex poisoned and wakes
/// every waiter, who then panic.
///
/// # Safety
/// `cell` must be a live handle whose lock the caller holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_mutex_unlock(cell: *const Cell, poisoned: bool) {
    // SAFETY: guaranteed by the caller.
    let cell = unsafe { &*cell };
    let mut state = cell.lock();
    if poisoned {
        state.poisoned = true;
        state.locked = false;
        let waiters: Vec<_> = state.waiters.drain(..).collect();
        drop(state);
        for waiter in waiters {
            waiter.wake();
        }
    } else if let Some(waiter) = state.waiters.pop_front() {
        drop(state);
        waiter.wake();
    } else {
        state.locked = false;
    }
}

/// Whether a task panicked while holding the lock.
///
/// # Safety
/// `cell` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_mutex_is_poisoned(cell: *const Cell) -> bool {
    // SAFETY: guaranteed by the caller.
    unsafe { cell.as_ref() }.is_some_and(|cell| cell.lock().poisoned)
}
