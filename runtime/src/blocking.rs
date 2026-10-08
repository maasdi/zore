//! Runs a blocking system call on a helper thread while the calling task waits.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use super::fiber::Slot;
use super::waiter::Waiter;

type Job = Box<dyn FnOnce() + Send>;

const MAX_THREADS: usize = 512;
const IDLE_SECONDS: u64 = 5;

#[derive(Default)]
struct Pool {
    queue: VecDeque<Job>,
    idle: usize,
    threads: usize,
}

static POOL: Mutex<Pool> = Mutex::new(Pool {
    queue: VecDeque::new(),
    idle: 0,
    threads: 0,
});
static WORK: Condvar = Condvar::new();

fn pool() -> MutexGuard<'static, Pool> {
    POOL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn worker() {
    let mut state = pool();
    loop {
        if let Some(job) = state.queue.pop_front() {
            drop(state);
            job();
            state = pool();
            continue;
        }
        state.idle += 1;
        let (guard, timeout) = WORK
            .wait_timeout(state, Duration::from_secs(IDLE_SECONDS))
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state = guard;
        state.idle -= 1;
        if timeout.timed_out() && state.queue.is_empty() {
            state.threads -= 1;
            return;
        }
    }
}

fn submit(job: Job) {
    let mut state = pool();
    state.queue.push_back(job);
    if state.idle == 0 && state.threads < MAX_THREADS {
        state.threads += 1;
        drop(state);
        if std::thread::Builder::new().spawn(worker).is_err() {
            super::panic::fail(b"cannot start a helper thread");
        }
    } else {
        drop(state);
        WORK.notify_one();
    }
}

/// Runs `work` on a helper thread and waits for its result; only the calling task waits.
pub(super) fn run<R: Send + 'static>(work: impl FnOnce() -> R + Send + 'static) -> R {
    let slot = Arc::new(Slot::default());
    let result = Arc::new(Mutex::new(None));
    let (finished, output) = (Waiter::from(Arc::clone(&slot)), Arc::clone(&result));
    submit(Box::new(move || {
        let value = work();
        *output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(value);
        finished.wake();
    }));
    slot.park();
    let value = result
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    value.expect("a helper thread stores its result before waking the task")
}
