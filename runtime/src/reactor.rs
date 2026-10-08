//! Timers and descriptor readiness. One thread waits on the operating system's event queue and
//! wakes the tasks whose timer is due or whose descriptor is ready.

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
    use super::super::waiter::Waiter;

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
        waiters: HashMap<u64, Waiter>,
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
                let state = reactor.lock();
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
                    } else if let Some(slot) = state.waiters.remove(&token) {
                        woken.push(slot);
                    }
                }
                let now = Instant::now();
                while let Some(&Reverse((when, token))) = state.timers.peek() {
                    if when > now {
                        break;
                    }
                    state.timers.pop();
                    if let Some(slot) = state.waiters.remove(&token) {
                        woken.push(slot);
                    }
                }
            }
            if nudged {
                let mut buffer = [0u8; 64];
                let _ = reader.read(&mut buffer);
            }
            for slot in woken {
                slot.wake();
            }
        }
    }

    pub fn sleep(milliseconds: u64) {
        let reactor = started();
        let slot = Arc::new(Slot::default());
        let when = Instant::now() + Duration::from_millis(milliseconds);
        let earliest = {
            let mut state = reactor.lock();
            let token = state.next;
            state.next += 1;
            state.waiters.insert(token, Arc::clone(&slot).into());
            let earliest = state
                .timers
                .peek()
                .is_none_or(|Reverse((first, _))| when < *first);
            state.timers.push(Reverse((when, token)));
            earliest
        };
        if earliest {
            reactor.nudge();
        }
        slot.park();
    }

    /// Waits until `fd` can be read, or written when `write` is set; a failed registration
    /// returns at once so the caller's next system call reports the problem.
    pub fn wait_fd(fd: RawFd, write: bool, deadline: Option<Instant>) {
        let reactor = started();
        let slot = Arc::new(Slot::default());
        let (token, earliest) = {
            let mut state = reactor.lock();
            let token = state.next;
            state.next += 1;
            state.waiters.insert(token, Arc::clone(&slot).into());
            let earliest = deadline.is_some_and(|when| {
                let first = state
                    .timers
                    .peek()
                    .is_none_or(|Reverse((first, _))| when < *first);
                state.timers.push(Reverse((when, token)));
                first
            });
            (token, earliest)
        };
        if reactor.poller.arm(fd, write, token).is_err() {
            reactor.lock().waiters.remove(&token);
            return;
        }
        if earliest {
            reactor.nudge();
        }
        slot.park();
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    use super::Descriptor;
    use std::time::Duration;

    pub fn sleep(milliseconds: u64) {
        std::thread::sleep(Duration::from_millis(milliseconds));
    }

    pub fn wait_fd(_: Descriptor, _: bool, _: Option<std::time::Instant>) {
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
    imp::wait_fd(fd, write, deadline);
}
