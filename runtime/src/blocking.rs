//! Runs a blocking system call on a helper thread while the calling task waits.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use super::slot::Slot;
use super::waiter::Waiter;

type Job = Box<dyn FnOnce() + Send>;

const MAX_THREADS: usize = 512;
const IDLE_SECONDS: u64 = 5;

#[cfg(test)]
static TEST_CAPACITY: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
#[cfg(test)]
static TEST_IDLE: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();

fn capacity() -> usize {
    #[cfg(test)]
    if let Some(capacity) = TEST_CAPACITY.get() {
        return *capacity;
    }
    MAX_THREADS
}

fn idle_timeout() -> Duration {
    #[cfg(test)]
    if let Some(timeout) = TEST_IDLE.get() {
        return *timeout;
    }
    Duration::from_secs(IDLE_SECONDS)
}

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
            .wait_timeout(state, idle_timeout())
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state = guard;
        state.idle -= 1;
        if timeout.timed_out() && state.queue.is_empty() {
            state.threads -= 1;
            return;
        }
    }
}

fn enqueue(state: &mut Pool, job: Job) {
    state.queue.push_back(job);
    if state.queue.len() > state.idle && state.threads < capacity() {
        state.threads += 1;
        if std::thread::Builder::new().spawn(worker).is_err() {
            super::panic::fail(b"cannot start a helper thread");
        }
    }
    WORK.notify_one();
}

fn submit(job: Job) {
    enqueue(&mut pool(), job);
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

#[cfg(any(test, zore_test_hooks))]
pub(super) fn snapshot() -> (usize, usize, usize) {
    let state = pool();
    (state.threads, state.idle, state.queue.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::{Poll, TestPool};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn capacity_one_queue_preserves_progress_expired_deadlines_and_cleanup() {
        const CHILD: &str = "ZORE_TEST_SMALL_HELPER_POOL";
        if std::env::var_os(CHILD).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "blocking::tests::capacity_one_queue_preserves_progress_expired_deadlines_and_cleanup", "--nocapture"])
                .env(CHILD, "1")
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("small helper capacity subprocess timed out");
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            return;
        }
        TEST_CAPACITY.set(1).unwrap();
        TEST_IDLE.set(Duration::ZERO).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        drop(listener);
        let handle = crate::net::test_connection(stream);
        let mut deadline_result = crate::sys::ErrorOut::ok();
        unsafe { crate::net::zore_native_net_set_deadline(&mut deadline_result, handle, 1, 1) };
        assert_eq!(deadline_result.failed, 0);
        let (started, active) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        submit(Box::new(move || {
            started.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
        }));
        active.recv_timeout(Duration::from_secs(10)).unwrap();
        let (finished, results) = mpsc::channel();
        let deadline_finished = finished.clone();
        let retained = Arc::new(());
        let queued_payload = Arc::clone(&retained);
        {
            let mut state = pool();
            enqueue(
                &mut state,
                Box::new(move || {
                    let mut result = crate::sys::ByteArrayError::ok(&[]);
                    unsafe { crate::net::zore_native_net_read(&mut result, handle, 1) };
                    assert_eq!(result.failed, 1);
                    assert_eq!(result.array.len, 0);
                    assert_eq!(
                        unsafe { crate::bytes(result.message, result.message_len) },
                        b"net.Read: timed out"
                    );
                    crate::net::zore_native_net_close_handle(handle);
                    drop(queued_payload);
                    deadline_finished.send(1).unwrap();
                }),
            );
            enqueue(
                &mut state,
                Box::new(move || {
                    assert_eq!(run(|| 42), 42);
                    finished.send(2).unwrap();
                }),
            );
            assert_eq!((state.threads, state.idle, state.queue.len()), (1, 0, 2));
        }
        assert_eq!(Arc::strong_count(&retained), 2);
        release.send(()).unwrap();
        assert_eq!(results.recv_timeout(Duration::from_secs(10)).unwrap(), 1);
        assert_eq!(results.recv_timeout(Duration::from_secs(10)).unwrap(), 2);
        let cleanup_deadline = Instant::now() + Duration::from_secs(10);
        while snapshot() != (0, 0, 0) {
            assert!(Instant::now() < cleanup_deadline);
            std::thread::yield_now();
        }
        assert_eq!(Arc::strong_count(&retained), 1);
        assert_eq!(crate::net::live_handles(), 0);
        assert_eq!(crate::reactor::snapshot().0, 0);
        drop(peer);
    }

    #[test]
    fn queued_jobs_reserve_idle_helpers_before_they_wake() {
        run(|| ());
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = loop {
            let state = pool();
            if state.idle > 0 {
                break state;
            }
            drop(state);
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        for _ in 0..state.idle {
            let release = Arc::clone(&release);
            enqueue(
                &mut state,
                Box::new(move || {
                    let (released, changed) = &*release;
                    let guard = released.lock().unwrap();
                    drop(changed.wait_while(guard, |released| !*released).unwrap());
                }),
            );
        }
        let (done, completed) = mpsc::channel();
        enqueue(
            &mut state,
            Box::new(move || {
                let _ = done.send(());
            }),
        );
        drop(state);
        let result = completed.recv_timeout(Duration::from_secs(10));
        let (released, changed) = &*release;
        *released.lock().unwrap() = true;
        changed.notify_all();
        result.expect("the completion job needs a helper beyond the reserved idle workers");
    }

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
