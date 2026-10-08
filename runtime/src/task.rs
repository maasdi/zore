use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::alloc::{zore_alloc, zore_free};
use super::fiber;
use super::waiter::Waiter;

type Code = unsafe extern "C" fn(*mut u8);
type PollCode = unsafe extern "C" fn(*mut u8, *mut super::scheduler::Context) -> u8;

static SPAWNED: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub(super) struct State {
    pub(super) finished: bool,
    detached: bool,
    panic: Option<Vec<u8>>,
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

fn complete(shared: &Shared, block: Block, id: u64) {
    let panic = super::panic::take();
    if let Some(message) = &panic {
        super::panic::report_task(id, message);
    }
    let panicked = panic.is_some();
    let mut state = shared.lock();
    state.finished = true;
    state.panic = panic;
    let detached = state.detached;
    let waiter = state.waiter.take();
    if std::mem::take(&mut state.counted) {
        super::deadlock::remove_blocked();
    }
    drop(state);
    shared.finished.notify_all();
    if let Some(waiter) = waiter {
        waiter.wake();
    }
    if detached {
        block.discard(panicked);
    }
}

/// # Safety
/// Own a zore_alloc(size) block; callbacks accept it; entry returns 0/1 and borrows context only during polls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_task_spawn_poll(
    entry: PollCode,
    drop_results: Code,
    data: *mut u8,
    size: i64,
) -> *mut Handle {
    let shared = Arc::new(Shared::default());
    let block = Block {
        data,
        size,
        drop_results,
    };
    let task_shared = Arc::clone(&shared);
    super::scheduler::spawn(move |context| {
        // SAFETY: guaranteed by the caller; the block remains task-owned until Ready.
        if unsafe { entry(block.data, context) } == 0 {
            return super::scheduler::Poll::Pending;
        }
        complete(&task_shared, block, context.task_id());
        super::scheduler::Poll::Ready
    });
    Box::into_raw(Box::new(Handle { shared, block }))
}

/// # Safety
/// Use a unique live or null handle, current context, and writable out; retain Pending handles until Ready.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_task_poll(
    handle: *mut Handle,
    context: *mut super::scheduler::Context,
    size: i64,
    out: *mut *mut u8,
) -> u8 {
    if let Some(handle_ref) = unsafe { handle.as_ref() } {
        let mut state = handle_ref.shared.lock();
        if !state.finished {
            // SAFETY: this context is live only for the current poll; its waker is cloned.
            let context = unsafe { &mut *context };
            state.waiter = Some(context.waker().clone().into());
            context.pending_internal();
            return 0;
        }
    }
    // SAFETY: Ready transfers the uniquely owned handle to the ordinary result retrieval path.
    unsafe { out.write(zore_task_wait(handle, size)) };
    1
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
        complete(&task_shared, block, id);
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

    unsafe extern "C" fn poll_entry(block: *mut u8, context: *mut crate::scheduler::Context) -> u8 {
        // SAFETY: this test supplies an initialized i64 block and the scheduler's live context.
        let count = unsafe { block.cast::<i64>().read() };
        if count == 100 {
            return 1;
        }
        // SAFETY: this poll owns the block, and context is valid until it returns.
        unsafe {
            block.cast::<i64>().write(count + 1);
            (*context).waker().wake_by_ref();
            (*context).pending_internal();
        }
        0
    }

    #[test]
    fn generated_poll_entry_can_wake_before_pending_and_use_the_existing_handle_api() {
        let block = zore_alloc(8);
        // SAFETY: initialize the poll entry's eight-byte frame/result block.
        unsafe { block.cast::<i64>().write(0) };
        // SAFETY: both callbacks accept the allocated block, and the poll entry obeys its ABI.
        let handle = unsafe { zore_task_spawn_poll(poll_entry, drop_results, block, 8) };
        // SAFETY: the uniquely owned handle is consumed once.
        let result = unsafe { zore_task_wait(handle, 8) };
        // SAFETY: Ready leaves an initialized i64 in the block and transfers it to this caller.
        unsafe {
            assert_eq!(result.cast::<i64>().read(), 100);
            zore_free(result, 8);
        }
    }

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
        // Only a replacement can run while the sole original worker blocks.
        pool.spawn(move |_| {
            gate.wake();
            Poll::Ready
        });
        assert_eq!(completed.recv_timeout(Duration::from_secs(10)).unwrap(), 42);
        pool.idle();
    }
}
