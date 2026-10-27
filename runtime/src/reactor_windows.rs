use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::ops::Deref;
use std::os::windows::io::{AsRawSocket, RawSocket};
use std::sync::{Arc, Mutex, MutexGuard, Once, OnceLock};
use std::time::{Duration, Instant};

use super::{Operation, Waiter};

const POLLRDNORM: i16 = 0x0100;
const POLLWRNORM: i16 = 0x0010;
const POLLERR: i16 = 0x0001;
const POLLHUP: i16 = 0x0002;
const POLLNVAL: i16 = 0x0004;

#[repr(C)]
struct PollDescriptor {
    fd: RawSocket,
    events: i16,
    revents: i16,
}

#[link(name = "ws2_32")]
unsafe extern "system" {
    fn WSAPoll(descriptors: *mut PollDescriptor, count: u32, timeout: i32) -> i32;
}

pub(crate) struct Registration {
    operation: Arc<Operation>,
    token: Option<u64>,
}

impl Deref for Registration {
    type Target = Arc<Operation>;

    fn deref(&self) -> &Self::Target {
        &self.operation
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(token) = self.token {
            let reactor = started();
            let removed = reactor.lock().entries.remove(&token);
            drop(removed);
            reactor.nudge();
        }
    }
}

struct Entry {
    fd: RawSocket,
    write: bool,
    deadline: Option<Instant>,
    operation: Arc<Operation>,
    _owner: Arc<dyn Send + Sync>,
}

#[derive(Default)]
struct State {
    next: u64,
    entries: HashMap<u64, Arc<Entry>>,
}

impl State {
    fn insert(&mut self, entry: Entry) -> u64 {
        self.next = self.next.checked_add(1).unwrap_or_else(|| {
            fail(b"socket readiness registration tokens exhausted");
        });
        self.entries.insert(self.next, Arc::new(entry));
        self.next
    }

    fn snapshot(&self) -> Vec<(u64, Arc<Entry>)> {
        self.entries
            .iter()
            .map(|(&token, entry)| (token, Arc::clone(entry)))
            .collect()
    }

    fn settle(
        &mut self,
        snapshot: &[(u64, Arc<Entry>)],
        descriptors: &[PollDescriptor],
        now: Instant,
    ) -> Vec<Arc<Entry>> {
        let mut completed = Vec::new();
        for ((token, entry), descriptor) in snapshot.iter().zip(descriptors) {
            if (descriptor.revents != 0 || entry.deadline.is_some_and(|deadline| deadline <= now))
                && let Some(entry) = self.entries.remove(token)
            {
                completed.push(entry);
            }
        }
        completed
    }
}

struct Reactor {
    state: Mutex<State>,
    wake_reader: TcpStream,
    wake_writer: TcpStream,
}

impl Reactor {
    fn new() -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let wake_writer = TcpStream::connect(listener.local_addr()?)?;
        let (wake_reader, _) = listener.accept()?;
        wake_reader.set_nonblocking(true)?;
        wake_writer.set_nonblocking(true)?;
        wake_writer.set_nodelay(true)?;
        Ok(Self {
            state: Mutex::new(State::default()),
            wake_reader,
            wake_writer,
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn nudge(&self) {
        loop {
            match (&self.wake_writer).write(&[1]) {
                Ok(1) => return,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                _ => fail(b"cannot wake the socket readiness thread"),
            }
        }
    }

    fn drain(&self) {
        let mut buffer = [0; 4096];
        match (&self.wake_reader).read(&mut buffer) {
            Ok(0) => fail(b"socket readiness wake connection closed"),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => fail(b"cannot drain the socket readiness wake connection"),
        }
    }
}

fn fail(message: &[u8]) -> ! {
    super::super::panic::fail(message)
}

#[cfg(any(test, zore_test_hooks))]
static THREADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

static REACTOR: OnceLock<Reactor> = OnceLock::new();

fn started() -> &'static Reactor {
    static THREAD: Once = Once::new();
    let reactor = REACTOR.get_or_init(|| {
        Reactor::new()
            .unwrap_or_else(|_| fail(b"cannot create the socket readiness wake connection"))
    });
    THREAD.call_once(|| {
        if std::thread::Builder::new()
            .name("zore-sockets".into())
            .spawn(|| serve(reactor))
            .is_err()
        {
            fail(b"cannot start the socket readiness thread");
        }
        #[cfg(any(test, zore_test_hooks))]
        THREADS.store(1, std::sync::atomic::Ordering::SeqCst);
    });
    reactor
}

fn timeout_milliseconds(timeout: Option<Duration>) -> i32 {
    timeout.map_or(-1, |timeout| {
        let milliseconds =
            timeout.as_millis() + u128::from(timeout.subsec_nanos() % 1_000_000 != 0);
        i32::try_from(milliseconds).unwrap_or(i32::MAX)
    })
}

fn serve(reactor: &'static Reactor) {
    loop {
        let snapshot = reactor.lock().snapshot();
        let timeout = snapshot
            .iter()
            .filter_map(|(_, entry)| entry.deadline)
            .min()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let mut descriptors = Vec::with_capacity(snapshot.len() + 1);
        descriptors.push(PollDescriptor {
            fd: reactor.wake_reader.as_raw_socket(),
            events: POLLRDNORM,
            revents: 0,
        });
        descriptors.extend(snapshot.iter().map(|(_, entry)| PollDescriptor {
            fd: entry.fd,
            events: if entry.write { POLLWRNORM } else { POLLRDNORM },
            revents: 0,
        }));
        let count = u32::try_from(descriptors.len())
            .unwrap_or_else(|_| fail(b"too many socket readiness registrations"));
        let result = unsafe {
            WSAPoll(
                descriptors.as_mut_ptr(),
                count,
                timeout_milliseconds(timeout),
            )
        };
        if result < 0 {
            fail(b"cannot wait for socket readiness");
        }
        let wake_events = descriptors[0].revents;
        if wake_events & (POLLERR | POLLHUP | POLLNVAL) != 0 {
            fail(b"socket readiness wake connection failed");
        }
        if wake_events != 0 {
            reactor.drain();
        }
        let completed = reactor
            .lock()
            .settle(&snapshot, &descriptors[1..], Instant::now());
        drop(snapshot);
        for entry in completed {
            entry.operation.complete();
        }
    }
}

pub(crate) fn start_wait_fd(
    fd: RawSocket,
    write: bool,
    deadline: Option<Instant>,
    waiter: Waiter,
    owner: Arc<dyn Send + Sync>,
) -> Registration {
    let operation = Operation::new(waiter);
    if fd == RawSocket::MAX {
        operation.complete();
        return Registration {
            operation,
            token: None,
        };
    }
    let reactor = started();
    let token = reactor.lock().insert(Entry {
        fd,
        write,
        deadline,
        operation: Arc::clone(&operation),
        _owner: owner,
    });
    reactor.nudge();
    Registration {
        operation,
        token: Some(token),
    }
}

#[cfg(any(test, zore_test_hooks))]
pub(crate) fn snapshot() -> (usize, usize, usize) {
    let Some(reactor) = REACTOR.get() else {
        return (0, 0, 0);
    };
    let state = reactor.lock();
    let deadlines = state
        .entries
        .values()
        .filter(|entry| entry.deadline.is_some())
        .count();
    (state.entries.len(), deadlines, deadlines)
}

#[cfg(any(test, zore_test_hooks))]
pub(crate) fn service_threads() -> usize {
    THREADS.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::task::{Wake, Waker};

    struct Signal {
        wakes: AtomicUsize,
        sender: mpsc::Sender<()>,
    }

    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.wakes.fetch_add(1, Ordering::SeqCst);
            let _ = self.sender.send(());
        }
    }

    fn signal() -> (Arc<Signal>, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        (
            Arc::new(Signal {
                wakes: AtomicUsize::new(0),
                sender,
            }),
            receiver,
        )
    }

    fn waiter(signal: &Arc<Signal>) -> Waiter {
        Waker::from(Arc::clone(signal)).into()
    }

    fn pair() -> (Arc<TcpStream>, TcpStream) {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();
        (Arc::new(stream), peer)
    }

    fn entry(owner: Arc<TcpStream>, deadline: Option<Instant>, signal: &Arc<Signal>) -> Entry {
        Entry {
            fd: owner.as_raw_socket(),
            write: false,
            deadline,
            operation: Operation::new(waiter(signal)),
            _owner: owner,
        }
    }

    #[test]
    fn millisecond_timeouts_round_up_and_cap_without_periodic_polling() {
        assert_eq!(timeout_milliseconds(None), -1);
        assert_eq!(timeout_milliseconds(Some(Duration::ZERO)), 0);
        assert_eq!(timeout_milliseconds(Some(Duration::from_nanos(1))), 1);
        assert_eq!(timeout_milliseconds(Some(Duration::from_millis(1))), 1);
        assert_eq!(
            timeout_milliseconds(Some(Duration::from_nanos(1_000_001))),
            2
        );
        assert_eq!(
            timeout_milliseconds(Some(Duration::from_secs(u64::MAX))),
            i32::MAX
        );
    }

    #[test]
    fn simultaneous_readiness_and_expiration_complete_once() {
        let (owner, _peer) = pair();
        let (signal, received) = signal();
        let now = Instant::now();
        let mut state = State::default();
        let token = state.insert(entry(owner, Some(now), &signal));
        let snapshot = state.snapshot();
        let descriptors = [PollDescriptor {
            fd: snapshot[0].1.fd,
            events: POLLRDNORM,
            revents: POLLRDNORM,
        }];
        let completed = state.settle(&snapshot, &descriptors, now);
        assert_eq!(completed.len(), 1);
        assert!(!state.entries.contains_key(&token));
        assert!(state.settle(&snapshot, &descriptors, now).is_empty());
        completed[0].operation.complete();
        completed[0].operation.complete();
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(signal.wakes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cancellation_preserves_inflight_socket_ownership_and_ignores_stale_tokens() {
        let (owner, _peer) = pair();
        let weak = Arc::downgrade(&owner);
        let (signal, _) = signal();
        let mut state = State::default();
        let token = state.insert(entry(owner, None, &signal));
        let snapshot = state.snapshot();
        drop(state.entries.remove(&token));
        assert!(weak.upgrade().is_some());
        let (replacement, _replacement_peer) = pair();
        let next = state.insert(entry(replacement, None, &signal));
        assert_ne!(token, next);
        let descriptors = [PollDescriptor {
            fd: snapshot[0].1.fd,
            events: POLLRDNORM,
            revents: POLLRDNORM,
        }];
        assert!(
            state
                .settle(&snapshot, &descriptors, Instant::now())
                .is_empty()
        );
        assert!(state.entries.contains_key(&next));
        drop(snapshot);
        assert!(weak.upgrade().is_none());
        assert_eq!(signal.wakes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn dropped_registration_removes_state_and_nudges_owner_release() {
        let (owner, _peer) = pair();
        let weak = Arc::downgrade(&owner);
        let (signal, _) = signal();
        let registration =
            start_wait_fd(owner.as_raw_socket(), false, None, waiter(&signal), owner);
        let token = registration.token.unwrap();
        drop(registration);
        assert!(!started().lock().entries.contains_key(&token));
        let (marker, _marker_peer) = pair();
        let (marker_signal, received) = self::signal();
        let registration = start_wait_fd(
            marker.as_raw_socket(),
            false,
            Some(Instant::now()),
            waiter(&marker_signal),
            marker,
        );
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(registration.is_ready());
        assert!(weak.upgrade().is_none());
        assert_eq!(signal.wakes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn more_than_sixty_four_sockets_share_one_service_and_deliver_readiness() {
        let (signal, received) = signal();
        let mut registrations = Vec::new();
        let mut peers = Vec::new();
        for _ in 0..96 {
            let (owner, peer) = pair();
            registrations.push(start_wait_fd(
                owner.as_raw_socket(),
                false,
                None,
                waiter(&signal),
                owner,
            ));
            peers.push(peer);
        }
        assert_eq!(service_threads(), 1);
        for peer in &mut peers {
            peer.write_all(&[42]).unwrap();
        }
        for _ in 0..registrations.len() {
            received.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        assert!(
            registrations
                .iter()
                .all(|registration| registration.is_ready())
        );
        assert_eq!(signal.wakes.load(Ordering::SeqCst), registrations.len());
        assert_eq!(service_threads(), 1);
        let state = started().lock();
        assert!(
            registrations
                .iter()
                .all(|registration| !state.entries.contains_key(&registration.token.unwrap()))
        );
    }

    #[test]
    fn invalid_socket_completes_immediately_for_retry() {
        let (signal, received) = signal();
        let registration =
            start_wait_fd(RawSocket::MAX, false, None, waiter(&signal), Arc::new(()));
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(registration.is_ready());
        assert!(registration.token.is_none());
        assert_eq!(signal.wakes.load(Ordering::SeqCst), 1);
    }
}
