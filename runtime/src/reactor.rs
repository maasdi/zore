//! Timers and descriptor readiness. One thread waits on the operating system's event queue and
//! wakes the tasks whose timer is due or whose descriptor is ready.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::waiter::Waiter;

/// One externally driven wait. Both slots and task wakers use the same completion
/// publication; the owning operation stays live until its caller observes Ready.
pub struct Operation {
    ready: AtomicBool,
    waiter: Waiter,
}

impl Operation {
    fn new(waiter: Waiter) -> Arc<Self> {
        Arc::new(Self {
            ready: AtomicBool::new(false),
            waiter,
        })
    }

    fn complete(&self) {
        if !self.ready.swap(true, Ordering::AcqRel) {
            self.waiter.wake();
        }
    }
}

/// Polls a timer/readiness operation without blocking or registering again. Pending
/// preserves the operation; Ready consumes the caller's reference. These external
/// waits do not contribute an internal blocked count for deadlock detection.
///
/// # Safety
/// `operation` is null or an owned raw Arc reference to an Operation. One task polls
/// it serially, and must not access that raw reference after Ready consumes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_reactor_poll(operation: *const Operation) -> u8 {
    if operation.is_null() {
        return 1;
    }
    // SAFETY: guaranteed by the caller; the raw Arc keeps the operation live.
    if !unsafe { &*operation }.ready.load(Ordering::Acquire) {
        return 0;
    }
    // SAFETY: Ready consumes the caller's one owned reference exactly once.
    drop(unsafe { Arc::from_raw(operation) });
    1
}

#[cfg(unix)]
pub(super) type Descriptor = std::os::fd::RawFd;
#[cfg(not(unix))]
pub(super) type Descriptor = i32;

/// Something a task can wait on until it is ready.
pub(super) trait Pollable {
    fn descriptor(&self) -> Descriptor;
}

#[cfg(unix)]
impl<T: std::os::fd::AsRawFd> Pollable for T {
    fn descriptor(&self) -> Descriptor {
        self.as_raw_fd()
    }
}

#[cfg(not(unix))]
impl Pollable for std::net::TcpStream {
    fn descriptor(&self) -> Descriptor {
        0
    }
}

#[cfg(not(unix))]
impl Pollable for std::net::TcpListener {
    fn descriptor(&self) -> Descriptor {
        0
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn fail(message: &[u8]) -> ! {
    super::panic::fail(message)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod imp {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};
    use std::io::{PipeReader, PipeWriter, Read, Write};
    use std::os::fd::{AsRawFd, RawFd};
    use std::sync::{Arc, Mutex, MutexGuard, Once, OnceLock};
    use std::time::{Duration, Instant};

    use super::super::fiber::Slot;
    use super::{Operation, Waiter};

    const WAKE_TOKEN: u64 = 0;
    const BATCH: usize = 64;

    #[cfg(target_os = "linux")]
    mod poller {
        use std::io;
        use std::os::fd::RawFd;
        use std::time::Duration;

        #[cfg_attr(target_arch = "x86_64", repr(C, packed))]
        #[cfg_attr(not(target_arch = "x86_64"), repr(C))]
        #[derive(Clone, Copy)]
        struct EpollEvent {
            events: u32,
            data: u64,
        }

        unsafe extern "C" {
            fn epoll_create1(flags: i32) -> i32;
            fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: *mut EpollEvent) -> i32;
            fn epoll_wait(epfd: i32, events: *mut EpollEvent, max: i32, timeout: i32) -> i32;
        }

        const EPOLL_CLOEXEC: i32 = 0x80000;
        const EPOLL_CTL_ADD: i32 = 1;
        const EPOLL_CTL_MOD: i32 = 3;
        const EPOLLIN: u32 = 1;
        const EPOLLOUT: u32 = 4;
        const EPOLLONESHOT: u32 = 1 << 30;
        const ENOENT: i32 = 2;

        pub struct Poller {
            epoll: i32,
        }

        impl Poller {
            pub fn new() -> Self {
                // SAFETY: plain system call.
                let epoll = unsafe { epoll_create1(EPOLL_CLOEXEC) };
                if epoll < 0 {
                    super::super::fail(b"cannot create an event queue");
                }
                Self { epoll }
            }

            fn control(&self, op: i32, fd: RawFd, events: u32, token: u64) -> io::Result<()> {
                let mut event = EpollEvent {
                    events,
                    data: token,
                };
                // SAFETY: `event` is a valid event for the call.
                if unsafe { epoll_ctl(self.epoll, op, fd, &mut event) } < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            }

            /// Reports `token` every time `fd` is readable.
            pub fn watch(&self, fd: RawFd, token: u64) {
                if self.control(EPOLL_CTL_ADD, fd, EPOLLIN, token).is_err() {
                    super::super::fail(b"cannot watch the wake-up pipe");
                }
            }

            /// Reports `token` once when `fd` can be read or written.
            pub fn arm(&self, fd: RawFd, write: bool, token: u64) -> io::Result<()> {
                let events = if write { EPOLLOUT } else { EPOLLIN } | EPOLLONESHOT;
                match self.control(EPOLL_CTL_MOD, fd, events, token) {
                    Err(error) if error.raw_os_error() == Some(ENOENT) => {
                        self.control(EPOLL_CTL_ADD, fd, events, token)
                    }
                    other => other,
                }
            }

            pub fn wait(&self, timeout: Option<Duration>, ready: &mut Vec<u64>) {
                let milliseconds =
                    timeout.map_or(-1, |t| i32::try_from(t.as_millis() + 1).unwrap_or(i32::MAX));
                let mut events = [EpollEvent { events: 0, data: 0 }; super::BATCH];
                // SAFETY: the buffer holds `BATCH` events.
                let count = unsafe {
                    epoll_wait(
                        self.epoll,
                        events.as_mut_ptr(),
                        super::BATCH as i32,
                        milliseconds,
                    )
                };
                for event in events.iter().take(usize::try_from(count).unwrap_or(0)) {
                    let token = event.data;
                    ready.push(token);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    mod poller {
        use std::io;
        use std::os::fd::RawFd;
        use std::time::Duration;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Kevent {
            ident: usize,
            filter: i16,
            flags: u16,
            fflags: u32,
            data: isize,
            udata: *mut u8,
        }

        #[repr(C)]
        struct Timespec {
            seconds: i64,
            nanoseconds: i64,
        }

        unsafe extern "C" {
            fn kqueue() -> i32;
            fn kevent(
                kq: i32,
                changes: *const Kevent,
                change_count: i32,
                events: *mut Kevent,
                event_count: i32,
                timeout: *const Timespec,
            ) -> i32;
        }

        const EVFILT_READ: i16 = -1;
        const EVFILT_WRITE: i16 = -2;
        const EV_ADD: u16 = 0x1;
        const EV_ONESHOT: u16 = 0x10;

        pub struct Poller {
            queue: i32,
        }

        impl Poller {
            pub fn new() -> Self {
                // SAFETY: plain system call.
                let queue = unsafe { kqueue() };
                if queue < 0 {
                    super::super::fail(b"cannot create an event queue");
                }
                Self { queue }
            }

            fn change(&self, fd: RawFd, filter: i16, flags: u16, token: u64) -> io::Result<()> {
                let change = Kevent {
                    ident: fd as usize,
                    filter,
                    flags,
                    fflags: 0,
                    data: 0,
                    udata: token as usize as *mut u8,
                };
                // SAFETY: one valid change and no event buffer.
                let result = unsafe {
                    kevent(
                        self.queue,
                        &change,
                        1,
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null(),
                    )
                };
                if result < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            }

            /// Reports `token` every time `fd` is readable.
            pub fn watch(&self, fd: RawFd, token: u64) {
                if self.change(fd, EVFILT_READ, EV_ADD, token).is_err() {
                    super::super::fail(b"cannot watch the wake-up pipe");
                }
            }

            /// Reports `token` once when `fd` can be read or written.
            pub fn arm(&self, fd: RawFd, write: bool, token: u64) -> io::Result<()> {
                let filter = if write { EVFILT_WRITE } else { EVFILT_READ };
                self.change(fd, filter, EV_ADD | EV_ONESHOT, token)
            }

            pub fn wait(&self, timeout: Option<Duration>, ready: &mut Vec<u64>) {
                let limit = timeout.map(|t| Timespec {
                    seconds: i64::try_from(t.as_secs()).unwrap_or(i64::MAX),
                    nanoseconds: i64::from(t.subsec_nanos()),
                });
                let mut events = [Kevent {
                    ident: 0,
                    filter: 0,
                    flags: 0,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                }; super::BATCH];
                // SAFETY: the buffer holds `BATCH` events and the timeout outlives the call.
                let count = unsafe {
                    kevent(
                        self.queue,
                        std::ptr::null(),
                        0,
                        events.as_mut_ptr(),
                        super::BATCH as i32,
                        limit
                            .as_ref()
                            .map_or(std::ptr::null(), |l| l as *const Timespec),
                    )
                };
                for event in events.iter().take(usize::try_from(count).unwrap_or(0)) {
                    ready.push(event.udata as usize as u64);
                }
            }
        }
    }

    #[derive(Default)]
    struct State {
        next: u64,
        waiters: HashMap<u64, Arc<Operation>>,
        timers: BinaryHeap<Reverse<(Instant, u64)>>,
    }

    struct Reactor {
        poller: poller::Poller,
        state: Mutex<State>,
        wake: PipeWriter,
        reader: Mutex<Option<PipeReader>>,
    }

    impl Reactor {
        fn lock(&self) -> MutexGuard<'_, State> {
            self.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        fn nudge(&self) {
            let _ = (&self.wake).write_all(&[1]);
        }
    }

    fn started() -> &'static Reactor {
        static REACTOR: OnceLock<Reactor> = OnceLock::new();
        static THREAD: Once = Once::new();
        let reactor = REACTOR.get_or_init(|| {
            let (reader, writer) = std::io::pipe().unwrap_or_else(|_| {
                super::fail(b"cannot create the wake-up pipe");
            });
            let poller = poller::Poller::new();
            poller.watch(reader.as_raw_fd(), WAKE_TOKEN);
            Reactor {
                poller,
                state: Mutex::new(State {
                    next: 1,
                    ..State::default()
                }),
                wake: writer,
                reader: Mutex::new(Some(reader)),
            }
        });
        THREAD.call_once(|| {
            if std::thread::Builder::new()
                .spawn(|| serve(reactor))
                .is_err()
            {
                super::fail(b"cannot start the event thread");
            }
        });
        reactor
    }

    fn serve(reactor: &'static Reactor) {
        let mut reader = reactor
            .reader
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .expect("the event thread starts once");
        let mut ready = Vec::new();
        loop {
            let timeout = {
                let mut state = reactor.lock();
                while let Some(&Reverse((_, token))) = state.timers.peek() {
                    if state.waiters.contains_key(&token) {
                        break;
                    }
                    state.timers.pop();
                }
                state
                    .timers
                    .peek()
                    .map(|Reverse((when, _))| when.saturating_duration_since(Instant::now()))
            };
            ready.clear();
            reactor.poller.wait(timeout, &mut ready);
            let mut woken = Vec::new();
            let mut nudged = false;
            {
                let mut state = reactor.lock();
                for &token in &ready {
                    if token == WAKE_TOKEN {
                        nudged = true;
                    } else if let Some(operation) = state.waiters.remove(&token) {
                        woken.push(operation);
                    }
                }
                let now = Instant::now();
                while let Some(&Reverse((when, token))) = state.timers.peek() {
                    if when > now {
                        break;
                    }
                    state.timers.pop();
                    if let Some(operation) = state.waiters.remove(&token) {
                        woken.push(operation);
                    }
                }
            }
            if nudged {
                let mut buffer = [0u8; 64];
                let _ = reader.read(&mut buffer);
            }
            for operation in woken {
                operation.complete();
            }
        }
    }

    pub fn start_sleep(milliseconds: u64, waiter: Waiter) -> Arc<Operation> {
        let reactor = started();
        let operation = Operation::new(waiter);
        // A duration beyond the host Instant range must never finish early.
        let when = Instant::now().checked_add(Duration::from_millis(milliseconds));
        let earliest = {
            let mut state = reactor.lock();
            let token = state.next;
            state.next += 1;
            state.waiters.insert(token, Arc::clone(&operation));
            when.is_some_and(|when| {
                let earliest = state
                    .timers
                    .peek()
                    .is_none_or(|Reverse((first, _))| when < *first);
                state.timers.push(Reverse((when, token)));
                earliest
            })
        };
        if earliest {
            reactor.nudge();
        }
        operation
    }

    pub fn sleep(milliseconds: u64) {
        let slot = Arc::new(Slot::default());
        let _operation = start_sleep(milliseconds, Arc::clone(&slot).into());
        slot.park();
    }

    /// Registers one external wait for readiness or a deadline. Failed registration
    /// completes immediately so the caller's next system call reports the problem.
    pub fn start_wait_fd(
        fd: RawFd,
        write: bool,
        deadline: Option<Instant>,
        waiter: Waiter,
    ) -> Arc<Operation> {
        let reactor = started();
        let operation = Operation::new(waiter);
        let earliest = {
            let mut state = reactor.lock();
            let token = state.next;
            state.next += 1;
            state.waiters.insert(token, Arc::clone(&operation));
            // Keep registration and arming under the same lock so a due deadline
            // cannot consume the waiter before its descriptor has been armed.
            if reactor.poller.arm(fd, write, token).is_err() {
                state.waiters.remove(&token);
                drop(state);
                operation.complete();
                return operation;
            }
            deadline.is_some_and(|when| {
                let earliest = state
                    .timers
                    .peek()
                    .is_none_or(|Reverse((first, _))| when < *first);
                state.timers.push(Reverse((when, token)));
                earliest
            })
        };
        if earliest {
            reactor.nudge();
        }
        operation
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    use super::Descriptor;
    use super::{Operation, Waiter};
    use std::sync::Arc;
    use std::time::Duration;

    pub fn start_sleep(milliseconds: u64, waiter: Waiter) -> Arc<Operation> {
        let operation = Operation::new(waiter);
        let completion = Arc::clone(&operation);
        if std::thread::Builder::new()
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(milliseconds));
                completion.complete();
            })
            .is_err()
        {
            super::super::panic::fail(b"cannot start the timer thread");
        }
        operation
    }

    pub fn sleep(milliseconds: u64) {
        let _blocking = super::super::scheduler::BlockingGuard::enter();
        std::thread::sleep(Duration::from_millis(milliseconds));
    }

    pub fn start_wait_fd(
        _: Descriptor,
        _: bool,
        _: Option<std::time::Instant>,
        _: Waiter,
    ) -> Arc<Operation> {
        unreachable!("descriptors stay in blocking mode without an event queue")
    }
}

/// Whether descriptors can be waited on; otherwise sockets stay blocking.
pub(super) const POLLED: bool = cfg!(any(target_os = "linux", target_os = "macos"));

pub(super) fn sleep(milliseconds: u64) {
    imp::sleep(milliseconds);
}

/// Waits until `fd` is ready or `deadline` passes; the caller tries again to find out which.
pub(super) fn wait_fd(fd: Descriptor, write: bool, deadline: Option<std::time::Instant>) {
    let slot = Arc::new(super::fiber::Slot::default());
    let _operation = start_wait_fd(fd, write, deadline, Arc::clone(&slot).into());
    slot.park();
}

/// Starts a timer with either a fiber slot or the current poll task's waker.
pub(super) fn start_sleep(milliseconds: u64, waiter: Waiter) -> Arc<Operation> {
    imp::start_sleep(milliseconds, waiter)
}

/// Starts descriptor readiness/deadline registration without blocking a poll worker.
pub(super) fn start_wait_fd(
    fd: Descriptor,
    write: bool,
    deadline: Option<std::time::Instant>,
    waiter: Waiter,
) -> Arc<Operation> {
    imp::start_wait_fd(fd, write, deadline, waiter)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::task::{Wake, Waker};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::scheduler::{Poll, TestPool};

    struct Wakes(AtomicUsize);
    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn concurrent_completion_publishes_ready_and_wakes_once() {
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let operation = Operation::new(Waker::from(Arc::clone(&wakes)).into());
        let raw = Arc::into_raw(Arc::clone(&operation));
        // SAFETY: this raw Arc is owned and not consumed by a Pending poll.
        assert_eq!(unsafe { zore_reactor_poll(raw) }, 0);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let operation = Arc::clone(&operation);
                scope.spawn(move || operation.complete());
            }
        });
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        // SAFETY: Ready consumes exactly the one raw Arc reference above.
        assert_eq!(unsafe { zore_reactor_poll(raw) }, 1);
        assert_eq!(Arc::strong_count(&operation), 1);
    }

    #[test]
    fn sleep_poll_abi_handles_nonpositive_durations_and_real_timer_completion() {
        let pool = TestPool::new(1);
        let (done, completed) = mpsc::channel();
        let mut raw = 0usize;
        let mut started = None;
        pool.spawn(move |context| {
            if started.is_none() {
                for duration in [0, -1, i64::MIN] {
                    // SAFETY: context is current; immediate operations return null/Ready.
                    let operation =
                        unsafe { crate::sys::zore_native_time_sleep_start(duration, context) };
                    assert!(operation.is_null());
                    // SAFETY: null is the immediate-Ready representation.
                    assert_eq!(unsafe { zore_reactor_poll(operation) }, 1);
                }
                started = Some(Instant::now());
                // SAFETY: context is current; only the waker is retained.
                raw = unsafe { crate::sys::zore_native_time_sleep_start(20, context) } as usize;
            }
            // SAFETY: Pending preserves this raw reference; Ready consumes it once.
            if unsafe { zore_reactor_poll(raw as *const Operation) } == 0 {
                return Poll::Pending;
            }
            assert!(started.unwrap().elapsed() >= Duration::from_millis(20));
            done.send(()).unwrap();
            Poll::Ready
        });
        completed.recv_timeout(Duration::from_secs(10)).unwrap();
        pool.idle();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn descriptor_wake_before_pending_is_latched_and_storage_is_released() {
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;
        let pool = TestPool::new(1);
        let (mut reader, mut writer) = std::io::pipe().unwrap();
        let (registered, registration) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let fd = reader.as_raw_fd();
        let mut raw = 0usize;
        pool.spawn(move |context| {
            if raw == 0 {
                let operation = start_wait_fd(fd, false, None, context.waker().clone().into());
                let weak = Arc::downgrade(&operation);
                raw = Arc::into_raw(operation) as usize;
                // SAFETY: reader has no data yet and the raw Arc stays owned after Pending.
                assert_eq!(unsafe { zore_reactor_poll(raw as *const Operation) }, 0);
                registered.send(weak).unwrap();
                released.recv_timeout(Duration::from_secs(10)).unwrap();
                return Poll::Pending;
            }
            // SAFETY: the raw Arc remains owned until this Ready poll consumes it.
            assert_eq!(unsafe { zore_reactor_poll(raw as *const Operation) }, 1);
            done.send(()).unwrap();
            Poll::Ready
        });
        let weak = registration.recv_timeout(Duration::from_secs(10)).unwrap();
        writer.write_all(&[42]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !weak.upgrade().unwrap().ready.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "descriptor was not completed");
            std::thread::yield_now();
        }
        release.send(()).unwrap();
        completed.recv_timeout(Duration::from_secs(10)).unwrap();
        pool.idle();
        let deadline = Instant::now() + Duration::from_secs(10);
        while weak.upgrade().is_some() {
            assert!(
                Instant::now() < deadline,
                "completed registration retained storage"
            );
            std::thread::yield_now();
        }
        let mut byte = [0];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [42]);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn descriptor_deadline_and_failed_registration_complete_without_internal_blocking() {
        use std::os::fd::AsRawFd;
        let pool = TestPool::new(1);
        let (reader, _writer) = std::io::pipe().unwrap();
        let fd = reader.as_raw_fd();
        let (done, completed) = mpsc::channel();
        let mut raw = 0usize;
        let start = Instant::now();
        pool.spawn(move |context| {
            if raw == 0 {
                let failed = start_wait_fd(-1, false, None, context.waker().clone().into());
                // SAFETY: failed registration immediately completes the owned reference.
                assert_eq!(unsafe { zore_reactor_poll(Arc::into_raw(failed)) }, 1);
                raw = Arc::into_raw(start_wait_fd(
                    fd,
                    false,
                    Some(start + Duration::from_millis(20)),
                    context.waker().clone().into(),
                )) as usize;
            }
            // SAFETY: one serial poller owns this reference until Ready.
            if unsafe { zore_reactor_poll(raw as *const Operation) } == 0 {
                return Poll::Pending;
            }
            assert!(start.elapsed() >= Duration::from_millis(20));
            done.send(()).unwrap();
            Poll::Ready
        });
        completed.recv_timeout(Duration::from_secs(10)).unwrap();
        pool.idle();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn descriptor_readiness_and_deadline_races_wake_one_registration_once() {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        for _ in 0..100 {
            let (reader, mut writer) = std::io::pipe().unwrap();
            let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
            let operation = start_wait_fd(
                reader.as_raw_fd(),
                false,
                Some(Instant::now()),
                Waker::from(Arc::clone(&wakes)).into(),
            );
            writer.write_all(&[1]).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while wakes.0.load(Ordering::SeqCst) == 0 {
                assert!(Instant::now() < deadline, "readiness/deadline never woke");
                std::thread::yield_now();
            }
            assert!(operation.ready.load(Ordering::Acquire));
            assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
            // SAFETY: one owned reference, completed by readiness or the deadline.
            assert_eq!(unsafe { zore_reactor_poll(Arc::into_raw(operation)) }, 1);
        }
    }
}
