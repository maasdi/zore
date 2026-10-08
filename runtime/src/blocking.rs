//! Runs a blocking system call on a helper thread while the calling task waits.

use std::cell::Cell;
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

thread_local! {
    static HELPER: Cell<bool> = const { Cell::new(false) };
}

fn worker() {
    HELPER.set(true);
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
    if HELPER.get() {
        return work();
    }
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

pub struct Operation {
    result: Mutex<Option<super::panic::PanicState>>,
}

/// # Safety
/// Keep `data` exclusively live for `work` until Ready; context must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_blocking_start(
    work: unsafe extern "C" fn(*mut u8),
    data: *mut u8,
    context: *mut super::scheduler::Context,
) -> *const Operation {
    let operation = Arc::new(Operation {
        result: Mutex::new(None),
    });
    let completion = Arc::clone(&operation);
    let waiter = Waiter::from(unsafe { &*context }.waker().clone());
    let data = data as usize;
    submit(Box::new(move || {
        let mut panic = super::panic::PanicState::default();
        super::panic::swap_state(&mut panic);
        unsafe {
            work(data as *mut u8);
        }
        super::panic::swap_state(&mut panic);
        *completion
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(panic);
        waiter.wake();
    }));
    Arc::into_raw(operation)
}

/// # Safety
/// Own one live raw Arc and poll serially; Ready consumes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_blocking_poll(operation: *const Operation) -> u8 {
    let mut result = unsafe { &*operation }
        .result
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(mut panic) = result.take() else {
        return 0;
    };
    drop(result);
    super::panic::swap_state(&mut panic);
    drop(unsafe { Arc::from_raw(operation) });
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::{Poll, TestPool};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Instant;

    struct Work {
        started: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        value: AtomicUsize,
    }

    unsafe extern "C" fn controlled(data: *mut u8) {
        let work = unsafe { &*(data as *const Work) };
        work.started.send(()).unwrap();
        work.release.recv_timeout(Duration::from_secs(10)).unwrap();
        work.value.store(run(|| 42), Ordering::SeqCst);
    }

    #[test]
    fn pending_helpers_free_the_only_worker_and_publish_results_before_waking() {
        let pool = TestPool::new(1);
        let (started, rx) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let work = Box::into_raw(Box::new(Work {
            started,
            release: wait,
            value: AtomicUsize::new(0),
        })) as usize;
        let mut operation = 0usize;
        pool.spawn(move |context| {
            if operation == 0 {
                operation =
                    unsafe { zore_blocking_start(controlled, work as *mut u8, context) } as usize;
                rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            if unsafe { zore_blocking_poll(operation as *const Operation) } == 0 {
                Poll::Pending
            } else {
                let work = unsafe { Box::from_raw(work as *mut Work) };
                assert_eq!(work.value.load(Ordering::SeqCst), 42);
                Poll::Ready
            }
        });
        pool.spawn(move |_| {
            release.send(()).unwrap();
            Poll::Ready
        });
        pool.idle();
    }

    #[test]
    fn completion_before_pending_is_not_lost_or_resubmitted() {
        let pool = TestPool::new(1);
        let (started, rx) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let work = Box::into_raw(Box::new(Work {
            started,
            release: wait,
            value: AtomicUsize::new(0),
        })) as usize;
        let mut operation = 0usize;
        pool.spawn(move |context| {
            if operation == 0 {
                operation =
                    unsafe { zore_blocking_start(controlled, work as *mut u8, context) } as usize;
                rx.recv_timeout(Duration::from_secs(10)).unwrap();
                for _ in 0..20 {
                    assert_eq!(
                        unsafe { zore_blocking_poll(operation as *const Operation) },
                        0
                    );
                }
                release.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while unsafe { &*(operation as *const Operation) }
                    .result
                    .lock()
                    .unwrap()
                    .is_none()
                {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                Poll::Pending
            } else {
                assert_eq!(
                    unsafe { zore_blocking_poll(operation as *const Operation) },
                    1
                );
                let work = unsafe { Box::from_raw(work as *mut Work) };
                assert_eq!(work.value.load(Ordering::SeqCst), 42);
                Poll::Ready
            }
        });
        pool.idle();
    }

    unsafe extern "C" fn panicking(_data: *mut u8) {
        crate::panic::raise(b"helper panic");
    }

    #[test]
    fn helper_panics_transfer_to_the_task_and_do_not_contaminate_other_jobs() {
        let pool = TestPool::new(1);
        let mut operation = 0usize;
        pool.spawn(move |context| {
            if operation == 0 {
                operation = unsafe { zore_blocking_start(panicking, std::ptr::null_mut(), context) }
                    as usize;
            }
            if unsafe { zore_blocking_poll(operation as *const Operation) } == 0 {
                return Poll::Pending;
            }
            assert_eq!(crate::panic::take().unwrap(), b"helper panic");
            Poll::Ready
        });
        pool.idle();
        assert!(run(crate::panic::take).is_none());
    }
}
