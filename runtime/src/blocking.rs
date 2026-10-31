//! Runs a blocking system call on a helper thread while the calling task waits.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

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
    #[cfg(any(test, zore_test_hooks))]
    active_jobs: usize,
    #[cfg(any(test, zore_test_hooks))]
    starts: usize,
    #[cfg(any(test, zore_test_hooks))]
    retirements: usize,
    #[cfg(any(test, zore_test_hooks))]
    empty_wakes: usize,
    #[cfg(test)]
    now: Option<Instant>,
    #[cfg(test)]
    wait_generation: usize,
}

static POOL: Mutex<Pool> = Mutex::new(Pool {
    queue: VecDeque::new(),
    idle: 0,
    threads: 0,
    #[cfg(any(test, zore_test_hooks))]
    active_jobs: 0,
    #[cfg(any(test, zore_test_hooks))]
    starts: 0,
    #[cfg(any(test, zore_test_hooks))]
    retirements: 0,
    #[cfg(any(test, zore_test_hooks))]
    empty_wakes: 0,
    #[cfg(test)]
    now: None,
    #[cfg(test)]
    wait_generation: 0,
});
static WORK: Condvar = Condvar::new();
#[cfg(any(test, zore_test_hooks))]
static CHANGED: Condvar = Condvar::new();

fn pool() -> MutexGuard<'static, Pool> {
    POOL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now(_state: &Pool) -> Instant {
    #[cfg(test)]
    if let Some(now) = _state.now {
        return now;
    }
    Instant::now()
}

thread_local! {
    static HELPER: Cell<bool> = const { Cell::new(false) };
}

fn worker() {
    HELPER.set(true);
    let mut state = pool();
    let mut idle_deadline = None;
    loop {
        if let Some(job) = state.queue.pop_front() {
            #[cfg(any(test, zore_test_hooks))]
            {
                state.active_jobs += 1;
                CHANGED.notify_all();
            }
            drop(state);
            job();
            state = pool();
            idle_deadline = None;
            #[cfg(any(test, zore_test_hooks))]
            {
                state.active_jobs -= 1;
                CHANGED.notify_all();
            }
            continue;
        }
        let current = now(&state);
        let deadline = *idle_deadline.get_or_insert_with(|| current + idle_timeout());
        let remaining = deadline.saturating_duration_since(current);
        if remaining.is_zero() {
            state.threads -= 1;
            #[cfg(any(test, zore_test_hooks))]
            {
                state.retirements += 1;
                CHANGED.notify_all();
            }
            return;
        }
        state.idle += 1;
        #[cfg(test)]
        {
            state.wait_generation += 1;
            CHANGED.notify_all();
        }
        let (guard, _timeout) = WORK
            .wait_timeout(state, remaining)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state = guard;
        state.idle -= 1;
        #[cfg(any(test, zore_test_hooks))]
        if !_timeout.timed_out() && state.queue.is_empty() {
            state.empty_wakes += 1;
            CHANGED.notify_all();
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
        #[cfg(any(test, zore_test_hooks))]
        {
            state.starts += 1;
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
    let mut panic = super::panic::PanicState::for_current_task();
    submit(Box::new(move || {
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
    let Some(panic) = result.take() else {
        return 0;
    };
    drop(result);
    super::panic::adopt(panic);
    drop(unsafe { Arc::from_raw(operation) });
    1
}

#[cfg(any(test, zore_test_hooks))]
pub(super) fn snapshot() -> (usize, usize, usize) {
    let state = pool();
    (state.threads, state.idle, state.queue.len())
}

#[cfg(any(test, zore_test_hooks))]
pub(super) struct LifecycleSnapshot {
    pub threads: usize,
    pub idle: usize,
    pub queued: usize,
    pub active_jobs: usize,
    pub starts: usize,
    pub retirements: usize,
    pub empty_wakes: usize,
}

#[cfg(any(test, zore_test_hooks))]
pub(super) fn lifecycle_snapshot() -> LifecycleSnapshot {
    let state = pool();
    LifecycleSnapshot {
        threads: state.threads,
        idle: state.idle,
        queued: state.queue.len(),
        active_jobs: state.active_jobs,
        starts: state.starts,
        retirements: state.retirements,
        empty_wakes: state.empty_wakes,
    }
}

#[cfg(any(test, zore_test_hooks))]
#[derive(Default)]
struct BurstState {
    released: bool,
    completed: usize,
}

#[cfg(any(test, zore_test_hooks))]
pub(super) struct HelperBurst {
    count: usize,
    gate: Arc<(Mutex<BurstState>, Condvar)>,
}

#[cfg(any(test, zore_test_hooks))]
impl HelperBurst {
    pub fn start(count: usize) -> Self {
        assert!(count > 0);
        let burst = Self {
            count,
            gate: Arc::default(),
        };
        let mut state = pool();
        assert_eq!(state.active_jobs, 0, "helper jobs must be drained");
        assert!(state.queue.is_empty(), "helper queue must be drained");
        for _ in 0..count {
            let gate = Arc::clone(&burst.gate);
            enqueue(
                &mut state,
                Box::new(move || {
                    let (state, changed) = &*gate;
                    let guard = state.lock().unwrap();
                    drop(changed.wait_while(guard, |state| !state.released).unwrap());
                    assert_eq!(run(|| 42), 42);
                    state.lock().unwrap().completed += 1;
                    changed.notify_all();
                }),
            );
        }
        drop(state);
        burst.wait_active();
        burst
    }

    pub fn wait_active(&self) {
        wait_pool(|state| {
            let active = self.count.min(capacity());
            state.active_jobs == active && state.queue.len() == self.count - active
        });
    }

    pub fn release(&self) {
        let (state, changed) = &*self.gate;
        state.lock().unwrap().released = true;
        changed.notify_all();
    }

    pub fn wait_completed(&self) {
        let (state, changed) = &*self.gate;
        let (guard, _) = changed
            .wait_timeout_while(state.lock().unwrap(), Duration::from_secs(30), |state| {
                state.completed != self.count
            })
            .unwrap();
        assert!(
            guard.completed == self.count,
            "helper burst completion timed out"
        );
        drop(guard);
        wait_pool(|state| state.active_jobs == 0 && state.queue.is_empty());
    }
}

#[cfg(any(test, zore_test_hooks))]
impl Drop for HelperBurst {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(any(test, zore_test_hooks))]
fn wait_pool(mut ready: impl FnMut(&Pool) -> bool) {
    let (state, _) = CHANGED
        .wait_timeout_while(pool(), Duration::from_secs(30), |state| !ready(state))
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(ready(&state), "helper checkpoint timed out");
}

#[cfg(any(test, zore_test_hooks))]
pub(super) fn wake_idle_helpers() {
    let _state = pool();
    WORK.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::{Poll, TestPool};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Instant;

    fn advance_idle_clock(duration: Duration) {
        let generation = {
            let mut state = pool();
            assert_eq!((state.threads, state.idle, state.active_jobs), (1, 1, 0));
            state.now = Some(state.now.unwrap() + duration);
            WORK.notify_all();
            state.wait_generation
        };
        wait_pool(|state| state.wait_generation > generation && state.idle == 1);
        let state = lifecycle_snapshot();
        assert_eq!((state.threads, state.active_jobs, state.queued), (1, 0, 0));
    }

    fn expire_idle_clock(duration: Duration) {
        {
            let mut state = pool();
            state.now = Some(state.now.unwrap() + duration);
            WORK.notify_all();
        }
        wait_pool(|state| state.threads == 0);
        assert_eq!(snapshot(), (0, 0, 0));
        assert_eq!(lifecycle_snapshot().active_jobs, 0);
    }

    #[test]
    fn idle_deadlines_survive_empty_wakes_reset_after_work_and_admit_expired_queue() {
        const CHILD: &str = "ZORE_TEST_HELPER_IDLE_DEADLINE";
        if std::env::var_os(CHILD).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "blocking::tests::idle_deadlines_survive_empty_wakes_reset_after_work_and_admit_expired_queue",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(40);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("helper idle deadline subprocess timed out");
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            return;
        }
        TEST_CAPACITY.set(1).unwrap();
        pool().now = Some(Instant::now());
        assert_eq!(run(|| run(|| 42)), 42);
        wait_pool(|state| state.idle == 1);
        for _ in 0..4 {
            advance_idle_clock(Duration::from_secs(1));
        }
        assert!(lifecycle_snapshot().empty_wakes >= 4);
        expire_idle_clock(Duration::from_secs(1));
        let state = lifecycle_snapshot();
        assert_eq!((state.starts, state.retirements), (1, 1));

        assert_eq!(run(|| run(|| 42)), 42);
        wait_pool(|state| state.idle == 1);
        advance_idle_clock(Duration::from_secs(4));
        let generation = pool().wait_generation;
        assert_eq!(run(|| run(|| 42)), 42);
        wait_pool(|state| state.wait_generation > generation && state.idle == 1);
        advance_idle_clock(Duration::from_secs(1));
        advance_idle_clock(Duration::from_secs(3));

        let (started, active) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (finished, completed) = mpsc::channel();
        let retained = Arc::new(());
        let payload = Arc::clone(&retained);
        let generation = {
            let mut state = pool();
            state.now = Some(state.now.unwrap() + Duration::from_secs(1));
            enqueue(
                &mut state,
                Box::new(move || {
                    assert_eq!(run(|| 42), 42);
                    started.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(10)).unwrap();
                    drop(payload);
                }),
            );
            enqueue(
                &mut state,
                Box::new(move || finished.send(run(|| 42)).unwrap()),
            );
            state.wait_generation
        };
        active.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(snapshot(), (1, 0, 1));
        let state = lifecycle_snapshot();
        assert_eq!(
            (state.starts, state.retirements, state.active_jobs),
            (2, 1, 1)
        );
        assert_eq!(Arc::strong_count(&retained), 2);
        release.send(()).unwrap();
        assert_eq!(completed.recv_timeout(Duration::from_secs(10)).unwrap(), 42);
        wait_pool(|state| {
            state.wait_generation > generation
                && state.idle == 1
                && state.active_jobs == 0
                && state.queue.is_empty()
        });
        assert_eq!(Arc::strong_count(&retained), 1);
        advance_idle_clock(Duration::from_secs(4));
        expire_idle_clock(Duration::from_secs(1));
        let state = lifecycle_snapshot();
        assert_eq!((state.starts, state.retirements), (2, 2));
    }

    #[test]
    fn helper_bursts_track_active_queued_completed_and_retired_jobs() {
        const CHILD: &str = "ZORE_TEST_HELPER_BURST_LIFECYCLE";
        if std::env::var_os(CHILD).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "blocking::tests::helper_bursts_track_active_queued_completed_and_retired_jobs",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(40);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("helper burst lifecycle subprocess timed out");
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            return;
        }
        TEST_CAPACITY.set(2).unwrap();
        TEST_IDLE.set(Duration::ZERO).unwrap();
        let burst = crate::scheduler::test_support::HelperBurst::start(3);
        burst.wait_active();
        let state = lifecycle_snapshot();
        assert_eq!((state.threads, state.active_jobs, state.queued), (2, 2, 1));
        assert_eq!(
            (state.starts, state.retirements, state.empty_wakes),
            (2, 0, 0)
        );
        assert_eq!(snapshot(), (2, 0, 1));
        burst.release();
        burst.wait_completed();
        wait_pool(|state| state.threads == 0);
        let state = lifecycle_snapshot();
        assert_eq!((state.active_jobs, state.queued), (0, 0));
        assert_eq!((state.starts, state.retirements), (2, 2));
        let dropped = crate::scheduler::test_support::HelperBurst::start(3);
        dropped.wait_active();
        drop(dropped);
        wait_pool(|state| state.threads == 0 && state.active_jobs == 0 && state.queue.is_empty());
        let state = lifecycle_snapshot();
        assert_eq!((state.starts, state.retirements), (4, 4));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn socket_reads_and_deadlines_progress_while_the_only_helper_is_held() {
        use std::io::Write;

        unsafe extern "C" {
            #[link_name = "zore_native_net_read_start"]
            fn socket_read_start(id: i64, max: i64, context: *mut u8) -> *mut u8;

            #[link_name = "zore_native_net_poll"]
            fn socket_poll(operation: *mut u8, context: *mut u8, output: *mut u8) -> u8;
        }

        const CHILD: &str = "ZORE_TEST_SOCKET_PROGRESS_WITH_HELD_HELPER";
        if std::env::var_os(CHILD).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "blocking::tests::socket_reads_and_deadlines_progress_while_the_only_helper_is_held", "--nocapture"])
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
                    panic!("socket progress subprocess timed out");
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            return;
        }
        TEST_CAPACITY.set(1).unwrap();
        TEST_IDLE.set(Duration::ZERO).unwrap();

        let (started, active) = mpsc::channel();
        let (release, held) = mpsc::channel();
        submit(Box::new(move || {
            started.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(20)).unwrap();
        }));
        active.recv_timeout(Duration::from_secs(10)).unwrap();
        let (sentinel, queued) = mpsc::channel();
        submit(Box::new(move || sentinel.send(()).unwrap()));
        assert_eq!(snapshot(), (1, 0, 1));

        let scheduler = TestPool::new(1);
        let (registered, registrations) = mpsc::channel();
        let (finished, completions) = mpsc::channel();
        let mut peers = Vec::new();
        for timed in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let peer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (stream, _) = listener.accept().unwrap();
            let handle = crate::net::test_connection(stream);
            peers.push(peer);
            let registered = registered.clone();
            let finished = finished.clone();
            let mut operation = 0usize;
            scheduler.spawn(move |context| unsafe {
                let mut output = std::mem::MaybeUninit::<crate::sys::ByteArrayError>::uninit();
                if operation == 0 {
                    if timed {
                        let mut deadline_result = crate::sys::ErrorOut::ok();
                        crate::net::zore_native_net_set_deadline(
                            &mut deadline_result,
                            handle,
                            1,
                            crate::sys::zore_native_time_now() + 100_000_000,
                        );
                        assert_eq!(deadline_result.failed, 0);
                    }
                    operation =
                        socket_read_start(handle, 1, std::ptr::from_mut(context).cast()) as usize;
                    registered.send(()).unwrap();
                }
                if socket_poll(
                    operation as *mut u8,
                    std::ptr::from_mut(context).cast(),
                    output.as_mut_ptr().cast(),
                ) == 0
                {
                    return Poll::Pending;
                }
                let output = output.assume_init();
                if timed {
                    assert_eq!((output.failed, output.array.len), (1, 0));
                    assert_eq!(
                        crate::bytes(output.message, output.message_len),
                        b"net.Read: timed out"
                    );
                } else {
                    assert_eq!(output.failed, 0);
                    assert_eq!(crate::bytes(output.array.data, output.array.len), b"x");
                }
                crate::alloc::zore_free(output.array.data, output.array.cap);
                crate::string::zore_string_release(output.message, output.message_len);
                crate::net::zore_native_net_close_handle(handle);
                finished.send(timed).unwrap();
                Poll::Ready
            });
        }
        for _ in 0..2 {
            registrations.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        peers[0].write_all(b"x").unwrap();
        let mut completed = [
            completions.recv_timeout(Duration::from_secs(10)).unwrap(),
            completions.recv_timeout(Duration::from_secs(10)).unwrap(),
        ];
        completed.sort_unstable();
        assert_eq!(completed, [false, true]);
        assert_eq!(queued.try_recv(), Err(mpsc::TryRecvError::Empty));
        assert_eq!(snapshot(), (1, 0, 1));
        scheduler.idle();
        assert_eq!(crate::net::live_handles(), 0);
        assert_eq!(crate::reactor::snapshot().0, 0);

        release.send(()).unwrap();
        queued.recv_timeout(Duration::from_secs(10)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while snapshot() != (0, 0, 0) {
            assert!(Instant::now() < deadline, "helper did not finish");
            std::thread::yield_now();
        }
    }

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
