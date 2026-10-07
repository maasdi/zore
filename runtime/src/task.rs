use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::alloc::{zore_alloc, zore_free};

const STACK_BYTES: usize = 16 << 20;

type Code = unsafe extern "C" fn(*mut u8);

static SPAWNED: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct State {
    finished: bool,
    detached: bool,
    panic: Option<Vec<u8>>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    finished: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
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
    RUNNING.load(Ordering::SeqCst)
}

/// Starts a thread that runs `entry` on the block.
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
    let id = SPAWNED.fetch_add(1, Ordering::SeqCst) + 1;
    RUNNING.fetch_add(1, Ordering::SeqCst);
    let task_shared = Arc::clone(&shared);
    let spawned = std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || {
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
            drop(state);
            task_shared.finished.notify_all();
            if detached {
                block.discard(panicked);
            }
            RUNNING.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        super::panic::fail(b"cannot start a task");
    }
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
    let mut state = handle.shared.lock();
    while !state.finished {
        state = handle
            .shared
            .finished
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    let panic = state.panic.take();
    drop(state);
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
