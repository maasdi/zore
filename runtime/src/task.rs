use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::alloc::{zore_alloc, zore_free};
use super::fiber;
use super::waiter::Waiter;

type Code = unsafe extern "C" fn(*mut u8);

static SPAWNED: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub(super) struct State {
    pub(super) finished: bool,
    detached: bool,
    panic: Option<Vec<u8>>,
    /// A fiber slot or polled task to resume when the task finishes.
    pub(super) waiter: Option<Waiter>,
    /// A task is waiting and counted as blocked for deadlock detection.
    pub(super) counted: bool,
}

#[derive(Default)]
pub(super) struct Shared {
    state: Mutex<State>,
    pub(super) finished: Condvar,
}

impl Shared {
    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// What the task owns: a block holding its closure, then its results.
#[derive(Clone, Copy)]
struct Block {
    data: *mut u8,
    size: i64,
    drop_results: Code,
}

// SAFETY: the block belongs to the task until it finishes, then to whoever retrieves or detaches it.
unsafe impl Send for Block {}

impl Block {
    /// Destroys the results nobody will read, then frees the block.
    fn discard(self, panicked: bool) {
        if !panicked {
            // SAFETY: the task completed, so its results are initialized and owned by the block.
            unsafe { (self.drop_results)(self.data) };
        }
        // SAFETY: the block came from `zore_alloc(size)` and has no other owner.
        unsafe { zore_free(self.data, self.size) };
    }
}

/// The handle a program holds; dropping or waiting on it consumes it.
pub struct Handle {
    shared: Arc<Shared>,
    block: Block,
}

pub(super) fn running() -> usize {
    super::deadlock::running()
}

pub(super) fn started() -> u64 {
    super::deadlock::started();
    SPAWNED.fetch_add(1, Ordering::SeqCst) + 1
}

pub(super) fn finished() {
    super::deadlock::finished();
}

/// Starts a task that runs `entry` on the block.
///
/// # Safety
/// `block` must come from `zore_alloc(size)`, and `entry` and `drop_results` must accept it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_task_spawn(
    entry: Code,
    drop_results: Code,
    block: *mut u8,
    size: i64,
) -> *mut Handle {
    let shared = Arc::new(Shared::default());
    let block = Block {
        data: block,
        size,
        drop_results,
    };
    let id = started();
    let task_shared = Arc::clone(&shared);
    fiber::spawn(Box::new(move || {
        // SAFETY: the caller guarantees `entry` accepts the block.
        unsafe { entry(block.data) };
        let panic = super::panic::take();
        if let Some(message) = &panic {
            super::panic::report_task(id, message);
        }
        let panicked = panic.is_some();
        let mut state = task_shared.lock();
        state.finished = true;
        state.panic = panic;
        let detached = state.detached;
        let waiter = state.waiter.take();
        if std::mem::take(&mut state.counted) {
            super::deadlock::remove_blocked();
        }
        drop(state);
        task_shared.finished.notify_all();
        if let Some(waiter) = waiter {
            waiter.wake();
        }
        if detached {
            block.discard(panicked);
        }
        finished();
        super::deadlock::check();
    }));
    Box::into_raw(Box::new(Handle { shared, block }))
}

fn zeroed_block(size: i64) -> *mut u8 {
    let data = zore_alloc(size);
    // SAFETY: the allocation holds `size` bytes.
    unsafe { data.write_bytes(0, usize::try_from(size).unwrap_or(0)) };
    data
}

/// Blocks until the task finishes and returns its block, which the caller frees. A panicked or
/// nil task raises its panic here and returns a zeroed block.
///
/// # Safety
/// `handle` must be null or come from `zore_task_spawn`, and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_task_wait(handle: *mut Handle, size: i64) -> *mut u8 {
    if handle.is_null() {
        super::panic::raise(b"wait on a nil task");
        return zeroed_block(size);
    }
    // SAFETY: guaranteed by the caller.
    let handle = unsafe { Box::from_raw(handle) };
    fiber::wait_for(&handle.shared);
    let panic = handle.shared.lock().panic.take();
    match panic {
        Some(message) => {
            // SAFETY: the task panicked, so its block has no results to destroy.
            unsafe { zore_free(handle.block.data, handle.block.size) };
            super::panic::raise(&message);
            zeroed_block(size)
        }
        None => handle.block.data,
    }
}

/// Lets the task run on by itself; whoever finishes second destroys its results.
///
/// # Safety
/// `handle` must be null or come from `zore_task_spawn`, and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_task_detach(handle: *mut Handle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: guaranteed by the caller.
    let handle = unsafe { Box::from_raw(handle) };
    let mut state = handle.shared.lock();
    state.detached = true;
    let finished = state.finished;
    let panicked = state.panic.is_some();
    drop(state);
    if finished {
        handle.block.discard(panicked);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::fiber::Slot;
    use crate::scheduler::{Poll, TestPool};

    unsafe extern "C" fn entry(block: *mut u8) {
        // SAFETY: the test places one Arc<Slot> in the block and transfers it to this entry.
        let gate = unsafe { block.cast::<Arc<Slot>>().read() };
        gate.park();
        // SAFETY: the allocation also has space for one i64 result.
        unsafe { block.cast::<i64>().write(42) };
    }

    unsafe extern "C" fn drop_results(_: *mut u8) {}

    #[test]
    fn blocking_join_from_poll_worker_compensates_and_retrieves_fiber_results() {
        let pool = TestPool::new(1);
        let gate = Arc::new(Slot::default());
        let size = std::mem::size_of::<Arc<Slot>>().max(8) as i64;
        let block = zore_alloc(size);
        // SAFETY: initialize the task's input in its allocated block.
        unsafe { block.cast::<Arc<Slot>>().write(Arc::clone(&gate)) };
        // SAFETY: the entry consumes the Arc input and writes the i64 result into this block.
        let handle = unsafe { zore_task_spawn(entry, drop_results, block, size) } as usize;
        let (entered, rx) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        pool.spawn(move |_| {
            entered.send(()).unwrap();
            // SAFETY: this closure is the sole owner of the live handle and consumes it once.
            let block = unsafe { zore_task_wait(handle as *mut Handle, size) };
            // SAFETY: the completed task wrote an i64 result into the block.
            let result = unsafe { block.cast::<i64>().read() };
            // SAFETY: the result is Copy and the block has no remaining input or result owners.
            unsafe { zore_free(block, size) };
            done.send(result).unwrap();
            Poll::Ready
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        // Only a replacement can run this job if the sole original worker is blocked in wait.
        pool.spawn(move |_| {
            gate.wake();
            Poll::Ready
        });
        assert_eq!(completed.recv_timeout(Duration::from_secs(10)).unwrap(), 42);
        pool.idle();
    }
}
