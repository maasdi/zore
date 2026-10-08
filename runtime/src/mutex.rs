use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as Lock, MutexGuard};

use super::alloc::{zore_alloc, zore_free};
use super::slot::Slot;
use super::waiter::Waiter;

type Destroy = unsafe extern "C" fn(*mut u8);

static LIVE: AtomicUsize = AtomicUsize::new(0);

pub(super) fn live_mutexes() -> usize {
    LIVE.load(Ordering::SeqCst)
}

#[derive(Default)]
struct State {
    locked: bool,
    poisoned: bool,
    waiters: VecDeque<Arc<Acquisition>>,
}

struct Acquisition {
    ready: AtomicBool,
    waiter: Waiter,
}

impl Acquisition {
    fn complete(&self) {
        self.ready.store(true, Ordering::Release);
        self.waiter.wake();
    }
}

pub struct Operation {
    cell: Option<Arc<Cell>>,
    acquisition: Arc<Acquisition>,
}

impl Operation {
    fn start(cell: Option<Arc<Cell>>, waiter: Waiter) -> Self {
        let acquisition = Arc::new(Acquisition {
            ready: AtomicBool::new(false),
            waiter,
        });
        if let Some(cell) = &cell {
            let mut state = cell.lock();
            if state.poisoned {
                acquisition.ready.store(true, Ordering::Release);
            } else if !state.locked {
                state.locked = true;
                acquisition.ready.store(true, Ordering::Release);
            } else {
                state.waiters.push_back(Arc::clone(&acquisition));
            }
        } else {
            acquisition.ready.store(true, Ordering::Release);
        }
        Self { cell, acquisition }
    }

    fn finish(&self) -> *mut u8 {
        let Some(cell) = &self.cell else {
            super::panic::raise(ZERO);
            return std::ptr::null_mut();
        };
        if cell.lock().poisoned {
            super::panic::raise(POISONED);
            return std::ptr::null_mut();
        }
        cell.value
    }
}

unsafe fn retain_cell(cell: *const Cell) -> Option<Arc<Cell>> {
    if cell.is_null() {
        return None;
    }
    unsafe {
        Arc::increment_strong_count(cell);
        Some(Arc::from_raw(cell))
    }
}

/// # Safety
/// `cell` must be null or live through unlock; `context` must be valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_mutex_start(
    cell: *const Cell,
    context: *mut super::scheduler::Context,
) -> *mut Operation {
    let operation =
        unsafe { Operation::start(retain_cell(cell), (*context).waker().clone().into()) };
    Box::into_raw(Box::new(operation))
}

/// # Safety
/// `operation` is live and exclusive until Ready consumes it; context and output are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_mutex_poll(
    operation: *mut Operation,
    context: *mut super::scheduler::Context,
    output: *mut *mut u8,
) -> u8 {
    let waiting = unsafe { &*operation };
    if !waiting.acquisition.ready.load(Ordering::Acquire) {
        unsafe {
            (*context).pending_internal();
        }
        return 0;
    }
    let operation = unsafe { Box::from_raw(operation) };
    unsafe {
        *output = operation.finish();
    }
    1
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
    let slot = Arc::new(Slot::internal());
    let operation = unsafe { Operation::start(retain_cell(cell), Arc::clone(&slot).into()) };
    if !operation.acquisition.ready.load(Ordering::Acquire) {
        slot.park();
    }
    operation.finish()
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
            waiter.complete();
        }
    } else if let Some(waiter) = state.waiters.pop_front() {
        drop(state);
        waiter.complete();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::{Poll, TestPool};
    use std::sync::mpsc;
    use std::time::Duration;

    struct WakeCount(AtomicUsize);

    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    unsafe fn cell() -> *const Cell {
        unsafe { zore_mutex_new(8, None, (&0_i64 as *const i64).cast()) }
    }

    #[test]
    fn mixed_waiters_receive_fifo_grants_without_barging() {
        unsafe {
            let cell = cell();
            assert!(!zore_mutex_lock(cell).is_null());
            let slot = Arc::new(Slot::internal());
            let first = Operation::start(retain_cell(cell), slot.clone().into());
            let counter = Arc::new(WakeCount(AtomicUsize::new(0)));
            let second = Operation::start(
                retain_cell(cell),
                std::task::Waker::from(counter.clone()).into(),
            );
            zore_mutex_unlock(cell, false);
            slot.park();
            let third = Operation::start(retain_cell(cell), Arc::new(Slot::internal()).into());
            assert!(first.acquisition.ready.load(Ordering::Acquire));
            assert!(!second.acquisition.ready.load(Ordering::Acquire));
            assert!(!third.acquisition.ready.load(Ordering::Acquire));
            assert!(!first.finish().is_null());
            zore_mutex_unlock(cell, false);
            assert!(second.acquisition.ready.load(Ordering::Acquire));
            assert_eq!(counter.0.load(Ordering::SeqCst), 1);
            assert!(!third.acquisition.ready.load(Ordering::Acquire));
            zore_mutex_unlock(cell, false);
            assert!(third.acquisition.ready.load(Ordering::Acquire));
            zore_mutex_unlock(cell, false);
            zore_mutex_release(cell);
        }
    }

    #[test]
    fn wake_before_pending_and_repoll_register_only_once() {
        let pool = TestPool::new(1);
        let (registered, rx) = mpsc::channel();
        let (released, release) = mpsc::channel();
        let cell = unsafe { cell() } as usize;
        unsafe {
            zore_mutex_lock(cell as *const Cell);
        }
        let mut operation = 0usize;
        pool.spawn(move |context| unsafe {
            if operation == 0 {
                operation = zore_mutex_start(cell as *const Cell, context) as usize;
                let mut value = std::ptr::null_mut();
                for _ in 0..20 {
                    assert_eq!(
                        zore_mutex_poll(operation as *mut Operation, context, &mut value),
                        0
                    );
                }
                assert_eq!((&*(cell as *const Cell)).lock().waiters.len(), 1);
                registered.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(10)).unwrap();
                Poll::Pending
            } else {
                let mut value = std::ptr::null_mut();
                assert_eq!(
                    zore_mutex_poll(operation as *mut Operation, context, &mut value),
                    1
                );
                assert!(!value.is_null());
                zore_mutex_unlock(cell as *const Cell, false);
                Poll::Ready
            }
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        unsafe {
            zore_mutex_unlock(cell as *const Cell, false);
        }
        released.send(()).unwrap();
        pool.idle();
        unsafe {
            zore_mutex_release(cell as *const Cell);
        }
    }

    #[test]
    fn concurrent_polled_acquisitions_preserve_exclusive_access() {
        let pool = TestPool::new(4);
        let cell = unsafe { cell() } as usize;
        for _ in 0..32 {
            let mut operation = 0usize;
            let mut count = 0;
            pool.spawn(move |context| unsafe {
                loop {
                    if operation == 0 {
                        operation = zore_mutex_start(cell as *const Cell, context) as usize;
                    }
                    let mut value = std::ptr::null_mut();
                    if zore_mutex_poll(operation as *mut Operation, context, &mut value) == 0 {
                        return Poll::Pending;
                    }
                    operation = 0;
                    *(value as *mut i64) += 1;
                    zore_mutex_unlock(cell as *const Cell, false);
                    count += 1;
                    if count == 100 {
                        return Poll::Ready;
                    }
                }
            });
        }
        pool.idle();
        unsafe {
            assert_eq!(*(zore_mutex_lock(cell as *const Cell) as *const i64), 3200);
            zore_mutex_unlock(cell as *const Cell, false);
            zore_mutex_release(cell as *const Cell);
        }
    }

    #[test]
    fn poisoning_completes_all_waiters_and_zero_is_immediate() {
        unsafe {
            let cell = cell();
            zore_mutex_lock(cell);
            let slot = Arc::new(Slot::internal());
            let first = Operation::start(retain_cell(cell), slot.clone().into());
            let counter = Arc::new(WakeCount(AtomicUsize::new(0)));
            let second = Operation::start(
                retain_cell(cell),
                std::task::Waker::from(counter.clone()).into(),
            );
            zore_mutex_unlock(cell, true);
            slot.park();
            for operation in [first, second] {
                assert!(operation.acquisition.ready.load(Ordering::Acquire));
                assert!(operation.finish().is_null());
                assert_eq!(crate::panic::take().unwrap(), POISONED);
            }
            let zero = Operation::start(None, Arc::new(Slot::internal()).into());
            assert!(zero.acquisition.ready.load(Ordering::Acquire));
            assert!(zero.finish().is_null());
            assert_eq!(crate::panic::take().unwrap(), ZERO);
            zore_mutex_release(cell);
        }
    }

    #[test]
    fn poisoning_wakes_queued_poll_tasks_without_granting_access() {
        let pool = TestPool::new(2);
        let (registered, rx) = mpsc::channel();
        let cell = unsafe { cell() } as usize;
        unsafe {
            zore_mutex_lock(cell as *const Cell);
        }
        for _ in 0..8 {
            let registered = registered.clone();
            let mut operation = 0usize;
            pool.spawn(move |context| unsafe {
                if operation == 0 {
                    operation = zore_mutex_start(cell as *const Cell, context) as usize;
                    registered.send(()).unwrap();
                }
                let mut value = std::ptr::null_mut();
                if zore_mutex_poll(operation as *mut Operation, context, &mut value) == 0 {
                    Poll::Pending
                } else {
                    assert!(value.is_null());
                    assert_eq!(crate::panic::take().unwrap(), POISONED);
                    Poll::Ready
                }
            });
        }
        for _ in 0..8 {
            rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        unsafe {
            zore_mutex_unlock(cell as *const Cell, true);
        }
        pool.idle();
        unsafe {
            zore_mutex_release(cell as *const Cell);
        }
    }
}
