use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::task::{Wake, Waker};

use super::panic::{self, PanicState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    Ready,
    Pending,
}

pub struct Context {
    waker: Waker,
    internal: bool,
    task_id: u64,
}

impl Context {
    pub(super) fn task_id(&self) -> u64 {
        self.task_id
    }
    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    /// Use only for internal waits; external events must return ordinary Pending.
    pub fn pending_internal(&mut self) -> Poll {
        self.internal = true;
        Poll::Pending
    }
}

type Body = Box<dyn FnMut(&mut Context) -> Poll + Send>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Queued,
    Running,
    Idle,
    Finished,
}

struct Scheduling {
    status: Status,
    notified: bool,
    counted: bool,
}

struct Frame {
    body: Option<Body>,
    panic: PanicState,
}

struct Task {
    id: u64,
    pool: Weak<Pool>,
    scheduling: Mutex<Scheduling>,
    frame: Mutex<Frame>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let mut state = lock(&self.scheduling);
        match state.status {
            Status::Running => state.notified = true,
            Status::Idle => {
                if std::mem::take(&mut state.counted) {
                    super::deadlock::remove_blocked();
                }
                state.status = Status::Queued;
                // Lock order is scheduling then pool; enqueue before releasing the scheduling lock.
                if let Some(pool) = self.pool.upgrade() {
                    pool.enqueue(Arc::clone(self));
                }
            }
            Status::Queued | Status::Finished => {}
        }
    }
}

impl Task {
    fn run(self: &Arc<Self>) {
        {
            let mut state = lock(&self.scheduling);
            assert!(state.status == Status::Queued);
            state.status = Status::Running;
            state.notified = false;
        }
        let mut frame = lock(&self.frame);
        let mut context = Context {
            waker: Waker::from(Arc::clone(self)),
            internal: false,
            task_id: self.id,
        };
        panic::swap_state(&mut frame.panic);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (frame.body.as_mut().expect("a runnable task has a body"))(&mut context)
        }));
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                // Restore task-local panic state before reporting an internal Rust unwind.
                panic::swap_state(&mut frame.panic);
                panic::fail(b"runtime task poll unwound");
            }
        };
        if result == Poll::Ready {
            // Destroy the frame with its task's panic state installed.
            frame.body.take();
            if let Some(message) = panic::take() {
                panic::report_task(self.id, &message);
            }
        }
        panic::swap_state(&mut frame.panic);
        drop(frame);

        let mut state = lock(&self.scheduling);
        if result == Poll::Ready {
            state.status = Status::Finished;
            if let Some(pool) = self.pool.upgrade() {
                lock(&pool.state).tasks.remove(&self.id);
                pool.ready.notify_all();
            }
            super::task::finished();
        } else if state.notified {
            state.status = Status::Queued;
            if let Some(pool) = self.pool.upgrade() {
                pool.enqueue(Arc::clone(self));
            }
        } else {
            state.status = Status::Idle;
            if context.internal {
                state.counted = true;
                super::deadlock::add_blocked();
            }
        }
        drop(state);
        super::deadlock::check();
    }
}

#[derive(Default)]
struct PoolState {
    queue: VecDeque<Arc<Task>>,
    tasks: HashMap<u64, Arc<Task>>,
    workers: usize,
    blocked: usize,
    #[cfg(test)]
    stopped: bool,
}

struct Pool {
    capacity: usize,
    state: Mutex<PoolState>,
    ready: Condvar,
}

thread_local! {
    static WORKER: RefCell<Option<Worker>> = const { RefCell::new(None) };
}

struct Worker {
    pool: Weak<Pool>,
    blocking: bool,
}

impl Pool {
    fn new(capacity: usize) -> Arc<Self> {
        let pool = Arc::new(Self {
            capacity,
            state: Mutex::new(PoolState::default()),
            ready: Condvar::new(),
        });
        lock(&pool.state).workers = capacity;
        for _ in 0..capacity {
            pool.start_worker();
        }
        pool
    }

    fn start_worker(self: &Arc<Self>) {
        let pool = Arc::clone(self);
        if std::thread::Builder::new()
            .name("zore-poll-worker".into())
            .spawn(move || pool.worker())
            .is_err()
        {
            panic::fail(b"cannot start a worker thread");
        }
    }

    fn enqueue(&self, task: Arc<Task>) {
        lock(&self.state).queue.push_back(task);
        self.ready.notify_one();
    }

    fn spawn(self: &Arc<Self>, body: Body) -> Waker {
        let id = super::task::started();
        let task = Arc::new(Task {
            id,
            pool: Arc::downgrade(self),
            scheduling: Mutex::new(Scheduling {
                status: Status::Queued,
                notified: false,
                counted: false,
            }),
            frame: Mutex::new(Frame {
                body: Some(body),
                panic: PanicState::default(),
            }),
        });
        lock(&self.state).tasks.insert(id, Arc::clone(&task));
        self.enqueue(Arc::clone(&task));
        Waker::from(task)
    }

    fn worker(self: Arc<Self>) {
        WORKER.with(|worker| {
            *worker.borrow_mut() = Some(Worker {
                pool: Arc::downgrade(&self),
                blocking: false,
            });
        });
        let mut state = lock(&self.state);
        loop {
            #[cfg(test)]
            if state.stopped {
                state.workers -= 1;
                self.ready.notify_all();
                return;
            }
            if let Some(task) = state.queue.pop_front() {
                drop(state);
                task.run();
                state = lock(&self.state);
                continue;
            }
            if state.workers - state.blocked > self.capacity {
                state.workers -= 1;
                self.ready.notify_all();
                return;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

/// Register a waker before Pending; dropping the returned waker does not cancel the task.
pub fn spawn(body: impl FnMut(&mut Context) -> Poll + Send + 'static) -> Waker {
    static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
    POOL.get_or_init(|| {
        Pool::new(std::thread::available_parallelism().map_or(2, |n| n.get().max(2)))
    })
    .spawn(Box::new(body))
}

/// Keep this guard on the blocked worker until the call resumes.
pub(super) struct BlockingGuard {
    pool: Option<Arc<Pool>>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl BlockingGuard {
    pub(super) fn enter() -> Self {
        let pool = WORKER.with(|worker| {
            let mut worker = worker.borrow_mut();
            let worker = worker.as_mut()?;
            if worker.blocking {
                return None;
            }
            let pool = worker.pool.upgrade()?;
            worker.blocking = true;
            Some(pool)
        });
        if let Some(pool) = &pool {
            let mut state = lock(&pool.state);
            state.blocked += 1;
            let replace = state.workers - state.blocked < pool.capacity;
            if replace {
                state.workers += 1;
            }
            drop(state);
            if replace {
                pool.start_worker();
            }
        }
        Self {
            pool,
            _thread: std::marker::PhantomData,
        }
    }
}

impl Drop for BlockingGuard {
    fn drop(&mut self) {
        if let Some(pool) = &self.pool {
            lock(&pool.state).blocked -= 1;
            WORKER.with(|worker| {
                worker
                    .borrow_mut()
                    .as_mut()
                    .expect("a pool worker")
                    .blocking = false;
            });
            pool.ready.notify_all();
        }
    }
}

#[cfg(test)]
pub(super) struct TestPool(Arc<Pool>);

#[cfg(test)]
impl TestPool {
    pub(super) fn new(capacity: usize) -> Self {
        Self(Pool::new(capacity))
    }

    pub(super) fn spawn(&self, body: impl FnMut(&mut Context) -> Poll + Send + 'static) -> Waker {
        self.0.spawn(Box::new(body))
    }

    pub(super) fn idle(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut state = lock(&self.0.state);
        while !state.tasks.is_empty() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!remaining.is_zero(), "poll tasks did not finish");
            state = self.0.ready.wait_timeout(state, remaining).unwrap().0;
        }
    }
}

#[cfg(test)]
impl Drop for TestPool {
    fn drop(&mut self) {
        self.idle();
        let mut state = lock(&self.0.state);
        state.stopped = true;
        self.0.ready.notify_all();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while state.workers != 0 {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!remaining.is_zero(), "poll workers did not stop");
            state = self.0.ready.wait_timeout(state, remaining).unwrap().0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Barrier, mpsc};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::fiber::Slot;

    fn receive<T>(rx: &mpsc::Receiver<T>) -> T {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("worker progress")
    }

    fn until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "scheduler transition timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn ready_runs_once_and_late_wakes_do_nothing() {
        let pool = TestPool::new(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let waker = pool.spawn(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Poll::Ready
        });
        pool.idle();
        for _ in 0..100 {
            waker.wake_by_ref();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(lock(&pool.0.state).queue.is_empty());
    }

    #[test]
    fn wake_before_pending_is_retained_and_coalesced() {
        let pool = TestPool::new(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        pool.spawn(move |context| {
            if count.fetch_add(1, Ordering::SeqCst) == 1000 {
                return Poll::Ready;
            }
            for _ in 0..10 {
                context.waker().wake_by_ref();
            }
            context.pending_internal()
        });
        pool.idle();
        assert_eq!(calls.load(Ordering::SeqCst), 1001);
    }

    #[test]
    fn concurrent_wakes_while_running_never_poll_concurrently() {
        let pool = TestPool::new(2);
        let (started, rx) = mpsc::channel();
        let barrier = Arc::new(Barrier::new(2));
        let release = Arc::clone(&barrier);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let waker = pool.spawn(move |_| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                started.send(()).unwrap();
                release.wait();
                Poll::Pending
            } else {
                Poll::Ready
            }
        });
        receive(&rx);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..1000 {
                        waker.wake_by_ref();
                    }
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        barrier.wait();
        pool.idle();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn pending_tasks_are_retained_and_idle_wakes_schedule_once() {
        let pool = TestPool::new(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let (registered, rx) = mpsc::channel();
        // Dropping a waker must not cancel a pending task.
        drop(pool.spawn(move |context| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                registered.send(context.waker().clone()).unwrap();
                Poll::Pending
            } else {
                Poll::Ready
            }
        }));
        let waker = receive(&rx);
        let task = lock(&pool.0.state).tasks.values().next().unwrap().clone();
        until(|| lock(&task.scheduling).status == Status::Idle);
        assert!(!lock(&task.scheduling).counted);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..1000 {
                        waker.wake_by_ref();
                    }
                });
            }
        });
        pool.idle();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn internal_pending_counts_until_wake_and_external_pending_does_not() {
        let pool = TestPool::new(1);
        for internal in [true, false] {
            let mut first = true;
            let waker = pool.spawn(move |context| {
                if std::mem::take(&mut first) {
                    if internal {
                        context.pending_internal()
                    } else {
                        Poll::Pending
                    }
                } else {
                    Poll::Ready
                }
            });
            let task = lock(&pool.0.state).tasks.values().next().unwrap().clone();
            until(|| lock(&task.scheduling).status == Status::Idle);
            assert_eq!(lock(&task.scheduling).counted, internal);
            waker.wake_by_ref();
            pool.idle();
            assert!(!lock(&task.scheduling).counted);
        }
    }

    #[test]
    fn panic_state_follows_pending_task_and_does_not_leak_to_other_polls() {
        let pool = TestPool::new(1);
        let (entered, rx) = mpsc::channel();
        let mut first = true;
        let waker = pool.spawn(move |_| {
            if std::mem::take(&mut first) {
                panic::raise(b"saved task panic");
                entered.send(()).unwrap();
                Poll::Pending
            } else {
                assert_eq!(panic::take(), Some(b"saved task panic".to_vec()));
                Poll::Ready
            }
        });
        receive(&rx);
        let (checked, rx) = mpsc::channel();
        pool.spawn(move |_| {
            assert!(!panic::zore_panic_pending());
            panic::raise(b"other task");
            assert_eq!(panic::take(), Some(b"other task".to_vec()));
            checked.send(()).unwrap();
            Poll::Ready
        });
        receive(&rx);
        waker.wake();
        pool.idle();
        assert!(!panic::zore_panic_pending());
    }

    #[test]
    fn pending_task_carries_panic_state_to_a_replacement_worker() {
        let pool = TestPool::new(1);
        let (entered, rx) = mpsc::channel();
        let (resumed, completed) = mpsc::channel();
        let mut origin = None;
        let waker = pool.spawn(move |_| {
            if let Some(origin) = origin {
                assert_ne!(std::thread::current().id(), origin);
                assert_eq!(panic::take(), Some(b"migrating panic".to_vec()));
                resumed.send(()).unwrap();
                Poll::Ready
            } else {
                origin = Some(std::thread::current().id());
                panic::raise(b"migrating panic");
                entered.send(()).unwrap();
                Poll::Pending
            }
        });
        receive(&rx);
        let task = lock(&pool.0.state).tasks.values().next().unwrap().clone();
        until(|| lock(&task.scheduling).status == Status::Idle);
        let (blocked, rx) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        pool.spawn(move |_| {
            assert!(!panic::zore_panic_pending());
            let _blocking = BlockingGuard::enter();
            blocked.send(()).unwrap();
            receive(&gate);
            Poll::Ready
        });
        receive(&rx);
        waker.wake();
        receive(&completed);
        release.send(()).unwrap();
        pool.idle();
    }

    #[test]
    fn detached_poll_panics_are_reported_after_frame_cleanup() {
        const CHILD: &str = "ZORE_TEST_POLL_PANIC";
        if std::env::var_os(CHILD).is_some() {
            struct Cleanup(Arc<AtomicBool>);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    self.0.store(panic::zore_panic_pending(), Ordering::SeqCst);
                }
            }
            let pool = TestPool::new(1);
            let dropped_with_panic = Arc::new(AtomicBool::new(false));
            let cleanup = Cleanup(Arc::clone(&dropped_with_panic));
            drop(pool.spawn(move |_| {
                let _keep_alive = &cleanup;
                panic::raise(b"detached poll panic");
                Poll::Ready
            }));
            pool.idle();
            assert!(dropped_with_panic.load(Ordering::SeqCst));
            assert!(!panic::zore_panic_pending());
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "scheduler::tests::detached_poll_panics_are_reported_after_frame_cleanup",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("panic in task "));
        assert!(stderr.contains("detached poll panic"));
    }

    #[test]
    fn blocking_slots_compensate_all_workers_and_replacements_retire() {
        let pool = TestPool::new(2);
        let (started, rx) = mpsc::channel();
        let slots: Vec<_> = (0..2).map(|_| Arc::new(Slot::default())).collect();
        for slot in &slots {
            let slot = Arc::clone(slot);
            let started = started.clone();
            pool.spawn(move |_| {
                started.send(()).unwrap();
                slot.park();
                Poll::Ready
            });
        }
        receive(&rx);
        receive(&rx);
        until(|| lock(&pool.0.state).blocked == 2);
        pool.spawn(move |_| {
            for slot in &slots {
                slot.wake();
            }
            Poll::Ready
        });
        pool.idle();
        until(|| lock(&pool.0.state).workers == 2);
        assert_eq!(lock(&pool.0.state).blocked, 0);
    }

    #[test]
    fn nested_blocking_guards_compensate_once() {
        let pool = TestPool::new(1);
        let inspect = Arc::clone(&pool.0);
        pool.spawn(move |_| {
            let outer = BlockingGuard::enter();
            let inner = BlockingGuard::enter();
            assert_eq!(lock(&inspect.state).blocked, 1);
            drop(inner);
            assert_eq!(lock(&inspect.state).blocked, 1);
            drop(outer);
            assert_eq!(lock(&inspect.state).blocked, 0);
            Poll::Ready
        });
        pool.idle();
    }

    #[test]
    fn wakes_racing_with_pending_settlement_are_not_lost() {
        let pool = TestPool::new(4);
        let (registered, rx) = mpsc::channel::<Waker>();
        let receiver = Mutex::new(rx);
        let calls = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let receiver = &receiver;
                scope.spawn(move || {
                    for _ in 0..125 {
                        let waker = receive(&lock(receiver));
                        for _ in 0..10 {
                            waker.wake_by_ref();
                        }
                    }
                });
            }
            for _ in 0..1000 {
                let registered = registered.clone();
                let count = Arc::clone(&calls);
                let mut first = true;
                pool.spawn(move |context| {
                    count.fetch_add(1, Ordering::SeqCst);
                    if std::mem::take(&mut first) {
                        registered.send(context.waker().clone()).unwrap();
                        context.pending_internal()
                    } else {
                        Poll::Ready
                    }
                });
            }
        });
        pool.idle();
        assert_eq!(calls.load(Ordering::SeqCst), 2000);
    }

    #[test]
    fn internal_pending_participates_in_process_deadlock_detection() {
        const CHILD: &str = "ZORE_TEST_POLL_DEADLOCK";
        if std::env::var_os(CHILD).is_some() {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(10));
                std::process::exit(71);
            });
            let pool = TestPool::new(1);
            pool.spawn(|context| context.pending_internal());
            Arc::new(Slot::internal()).park();
            unreachable!("no task can wake the initial task");
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "scheduler::tests::internal_pending_participates_in_process_deadlock_detection",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("all tasks are asleep"));
    }

    #[test]
    fn external_pending_can_wake_an_internally_blocked_initial_task() {
        const CHILD: &str = "ZORE_TEST_POLL_EXTERNAL";
        if std::env::var_os(CHILD).is_some() {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(10));
                std::process::exit(71);
            });
            let pool = TestPool::new(1);
            let slot = Arc::new(Slot::internal());
            let done = Arc::clone(&slot);
            let mut first = true;
            pool.spawn(move |context| {
                if std::mem::take(&mut first) {
                    let waker = context.waker().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(10));
                        waker.wake();
                    });
                    Poll::Pending
                } else {
                    done.wake();
                    Poll::Ready
                }
            });
            slot.park();
            pool.idle();
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "scheduler::tests::external_pending_can_wake_an_internally_blocked_initial_task",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
