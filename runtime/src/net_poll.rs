use super::*;
use crate::reactor::Pollable;
use crate::waiter::Waiter;
use std::io::{Read, Write};

pub struct Operation {
    handle: Option<Arc<Handle>>,
    kind: Kind,
    buffer: Vec<u8>,
    waiting: Option<Arc<reactor::Operation>>,
}

enum Kind {
    Accept,
    Read,
    Write { data: Vec<u8>, sent: usize },
}

enum Output {
    Value(ValueError),
    Bytes(ByteArrayError),
}

impl Operation {
    fn new(id: i64, kind: Kind, max: usize) -> Self {
        Self {
            handle: lookup(id),
            kind,
            buffer: vec![0; max],
            waiting: None,
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Accept => "net.Accept",
            Kind::Read => "net.Read",
            Kind::Write { .. } => "net.Write",
        }
    }

    fn failed(&self, message: &str) -> Output {
        match self.kind {
            Kind::Accept => Output::Value(ValueError::failed_text(message)),
            Kind::Read => Output::Bytes(ByteArrayError::failed(message)),
            Kind::Write { sent, .. } => {
                Output::Value(ValueError::failed_with(sent as i64, message))
            }
        }
    }

    fn poll(&mut self, waiter: Waiter) -> Option<Output> {
        if self
            .waiting
            .as_ref()
            .is_some_and(|operation| !operation.is_ready())
        {
            return None;
        }
        self.waiting = None;
        let Some(handle) = self.handle.clone() else {
            return Some(self.not_open());
        };
        loop {
            let (fd, write, deadline, attempted) = match (&mut self.kind, handle.as_ref()) {
                (Kind::Accept, Handle::Listener(listener)) => {
                    let result = listener
                        .socket
                        .accept()
                        .and_then(|(stream, _)| prepare(&stream).map(|()| stream))
                        .map(|stream| {
                            Some(Output::Value(ValueError::ok(insert(connection(stream)))))
                        });
                    (
                        listener.socket.descriptor(),
                        false,
                        &listener.deadline,
                        result,
                    )
                }
                (Kind::Read, Handle::Connection(connection)) => {
                    if self.buffer.is_empty() {
                        return Some(Output::Bytes(ByteArrayError::ok(&[])));
                    }
                    let mut stream = &connection.stream;
                    let result = match stream.read(&mut self.buffer) {
                        Ok(0) => return Some(self.failed("EOF")),
                        Ok(count) => Ok(Some(Output::Bytes(ByteArrayError::ok(
                            &self.buffer[..count],
                        )))),
                        Err(error) => Err(error),
                    };
                    (
                        connection.stream.descriptor(),
                        false,
                        &connection.read_deadline,
                        result,
                    )
                }
                (Kind::Write { data, sent }, Handle::Connection(connection)) => {
                    if *sent == data.len() {
                        return Some(Output::Value(ValueError::ok(*sent as i64)));
                    }
                    let mut stream = &connection.stream;
                    let result = match stream.write(&data[*sent..]) {
                        Ok(0) => Err(std::io::Error::from(ErrorKind::WriteZero)),
                        Ok(count) => {
                            *sent += count;
                            Ok(None)
                        }
                        Err(error) => Err(error),
                    };
                    (
                        connection.stream.descriptor(),
                        true,
                        &connection.write_deadline,
                        result,
                    )
                }
                _ => return Some(self.not_open()),
            };
            match attempted {
                Ok(Some(output)) => return Some(output),
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    let deadline = deadline_of(deadline);
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        return Some(self.failed(&format!("{}: timed out", self.name())));
                    }
                    self.waiting = Some(reactor::start_wait_fd(fd, write, deadline, waiter));
                    return None;
                }
                Err(error) => return Some(self.failed(&format!("{}: {error}", self.name()))),
            }
        }
    }

    fn not_open(&self) -> Output {
        self.failed(&format!(
            "{}: not an open {}",
            self.name(),
            if matches!(self.kind, Kind::Accept) {
                "listener"
            } else {
                "connection"
            }
        ))
    }
}

fn blocking(mut operation: Operation) -> Output {
    loop {
        let slot = Arc::new(crate::slot::Slot::default());
        if let Some(output) = operation.poll(Arc::clone(&slot).into()) {
            return output;
        }
        slot.park();
    }
}

fn max_of(max: i64) -> usize {
    usize::try_from(max).unwrap_or(0)
}

pub(super) fn accept(id: i64) -> ValueError {
    let Output::Value(output) = blocking(Operation::new(id, Kind::Accept, 0)) else {
        unreachable!()
    };
    output
}

pub(super) fn read(id: i64, max: i64) -> ByteArrayError {
    let Output::Bytes(output) = blocking(Operation::new(id, Kind::Read, max_of(max))) else {
        unreachable!()
    };
    output
}

pub(super) fn write(id: i64, data: &[u8]) -> ValueError {
    let kind = Kind::Write {
        data: data.to_vec(),
        sent: 0,
    };
    let Output::Value(output) = blocking(Operation::new(id, kind, 0)) else {
        unreachable!()
    };
    output
}

/// # Safety
/// `context` must be valid for this call; the returned operation has one serial poller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_accept_start(
    id: i64,
    _context: *mut crate::scheduler::Context,
) -> *mut Operation {
    Box::into_raw(Box::new(Operation::new(id, Kind::Accept, 0)))
}

/// # Safety
/// `context` must be valid for this call; the returned operation has one serial poller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_read_start(
    id: i64,
    max: i64,
    _context: *mut crate::scheduler::Context,
) -> *mut Operation {
    Box::into_raw(Box::new(Operation::new(id, Kind::Read, max_of(max))))
}

/// # Safety
/// `data` holds `len` readable bytes and context is live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_write_start(
    id: i64,
    data: *const u8,
    len: i64,
    _context: *mut crate::scheduler::Context,
) -> *mut Operation {
    let data = unsafe { crate::bytes(data, len) }.to_vec();
    Box::into_raw(Box::new(Operation::new(
        id,
        Kind::Write { data, sent: 0 },
        0,
    )))
}

/// # Safety
/// Poll exclusively with a live context and writable matching output; Ready consumes operation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_poll(
    operation: *mut Operation,
    context: *mut crate::scheduler::Context,
    output: *mut u8,
) -> u8 {
    let waiting = unsafe { &mut *operation };
    let waiter = unsafe { &*context }.waker().clone().into();
    let Some(result) = waiting.poll(waiter) else {
        return 0;
    };
    unsafe {
        match result {
            Output::Value(value) => output.cast::<ValueError>().write(value),
            Output::Bytes(value) => output.cast::<ByteArrayError>().write(value),
        }
        drop(Box::from_raw(operation));
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::{Poll, TestPool};
    use std::sync::mpsc;
    use std::task::{Wake, Waker};

    fn pair() -> (i64, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        prepare(&server).unwrap();
        (insert(connection(server)), client)
    }

    struct Signal(mpsc::Sender<()>);

    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.0.send(()).unwrap();
        }
    }

    fn signal() -> (Waiter, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        (Waker::from(Arc::new(Signal(tx))).into(), rx)
    }

    fn error(output: Output) -> String {
        let (data, len) = match output {
            Output::Bytes(value) => {
                assert_eq!(value.array.len, 0);
                (value.message, value.message_len)
            }
            Output::Value(value) => (value.message, value.message_len),
        };
        let text = String::from_utf8_lossy(unsafe { crate::bytes(data, len) }).into_owned();
        crate::string::zore_string_release(data, len);
        text
    }

    #[test]
    fn pending_reads_register_once_and_release_the_only_worker() {
        let _serial = crate::string::serial();
        let pool = TestPool::new(1);
        let (id, mut peer) = pair();
        let mut operation = 0usize;
        pool.spawn(move |context| unsafe {
            if operation == 0 {
                operation = zore_native_net_read_start(id, 1, context) as usize;
                let mut output = std::mem::MaybeUninit::<ByteArrayError>::uninit();
                assert_eq!(
                    zore_native_net_poll(
                        operation as *mut Operation,
                        context,
                        output.as_mut_ptr().cast()
                    ),
                    0
                );
                let registration = Arc::as_ptr(
                    (&*(operation as *const Operation))
                        .waiting
                        .as_ref()
                        .unwrap(),
                );
                for _ in 0..20 {
                    assert_eq!(
                        zore_native_net_poll(
                            operation as *mut Operation,
                            context,
                            output.as_mut_ptr().cast()
                        ),
                        0
                    );
                    assert_eq!(
                        registration,
                        Arc::as_ptr(
                            (&*(operation as *const Operation))
                                .waiting
                                .as_ref()
                                .unwrap()
                        )
                    );
                }
                Poll::Pending
            } else {
                let mut output = std::mem::MaybeUninit::<ByteArrayError>::uninit();
                if zore_native_net_poll(
                    operation as *mut Operation,
                    context,
                    output.as_mut_ptr().cast(),
                ) == 0
                {
                    return Poll::Pending;
                }
                let output = output.assume_init();
                assert_eq!(output.failed, 0);
                assert_eq!(crate::bytes(output.array.data, output.array.len), b"\xff");
                crate::alloc::zore_free(output.array.data, output.array.cap);
                Poll::Ready
            }
        });
        pool.spawn(move |_| {
            peer.write_all(b"\xff").unwrap();
            Poll::Ready
        });
        pool.idle();
        zore_native_net_close_handle(id);
    }

    #[test]
    fn a_passed_deadline_times_out_without_losing_later_bytes() {
        let _serial = crate::string::serial();
        let (id, mut peer) = pair();
        let handle = lookup(id).unwrap();
        let Handle::Connection(connection) = handle.as_ref() else {
            panic!()
        };
        connection.read_deadline.store(1, Ordering::SeqCst);
        let mut operation = Operation::new(id, Kind::Read, 4);
        let (waiter, _) = signal();
        assert_eq!(
            error(operation.poll(waiter).unwrap()),
            "net.Read: timed out"
        );
        connection.read_deadline.store(0, Ordering::SeqCst);
        peer.write_all(b"ok").unwrap();
        let output = read(id, 4);
        assert_eq!(output.failed, 0);
        assert_eq!(
            unsafe { crate::bytes(output.array.data, output.array.len) },
            b"ok"
        );
        unsafe { crate::alloc::zore_free(output.array.data, output.array.cap) };
        zore_native_net_close_handle(id);
    }

    #[test]
    fn partial_writes_resume_without_resending_completed_bytes() {
        let _serial = crate::string::serial();
        let pool = TestPool::new(1);
        let (id, mut peer) = pair();
        let bytes = 16 * 1024 * 1024;
        let data = vec![0x5a; bytes];
        let (started, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            rx.recv_timeout(Duration::from_secs(10)).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut received = Vec::new();
            peer.read_to_end(&mut received).unwrap();
            assert_eq!(received.len(), bytes);
            assert!(received.iter().all(|&byte| byte == 0x5a));
        });
        let mut operation = 0usize;
        pool.spawn(move |context| unsafe {
            if operation == 0 {
                operation =
                    zore_native_net_write_start(id, data.as_ptr(), data.len() as i64, context)
                        as usize;
                let mut output = std::mem::MaybeUninit::<ValueError>::uninit();
                assert_eq!(
                    zore_native_net_poll(
                        operation as *mut Operation,
                        context,
                        output.as_mut_ptr().cast()
                    ),
                    0
                );
                let Kind::Write { sent, .. } = (&*(operation as *const Operation)).kind else {
                    panic!()
                };
                assert!(sent > 0 && sent < bytes);
                for _ in 0..20 {
                    assert_eq!(
                        zore_native_net_poll(
                            operation as *mut Operation,
                            context,
                            output.as_mut_ptr().cast()
                        ),
                        0
                    );
                }
                started.send(()).unwrap();
                Poll::Pending
            } else {
                let mut output = std::mem::MaybeUninit::<ValueError>::uninit();
                if zore_native_net_poll(
                    operation as *mut Operation,
                    context,
                    output.as_mut_ptr().cast(),
                ) == 0
                {
                    return Poll::Pending;
                }
                let output = output.assume_init();
                assert_eq!((output.failed, output.value), (0, bytes as i64));
                zore_native_net_close_handle(id);
                Poll::Ready
            }
        });
        pool.idle();
        reader.join().unwrap();
    }

    #[test]
    fn invalid_handles_and_empty_reads_complete_without_registration() {
        let _serial = crate::string::serial();
        let (waiter, _) = signal();
        let mut accept = Operation::new(0, Kind::Accept, 0);
        assert_eq!(
            error(accept.poll(waiter.clone()).unwrap()),
            "net.Accept: not an open listener"
        );
        let mut read = Operation::new(0, Kind::Read, 1);
        assert_eq!(
            error(read.poll(waiter.clone()).unwrap()),
            "net.Read: not an open connection"
        );
        let mut write = Operation::new(
            0,
            Kind::Write {
                data: Vec::new(),
                sent: 0,
            },
            0,
        );
        assert_eq!(
            error(write.poll(waiter.clone()).unwrap()),
            "net.Write: not an open connection"
        );
        let (id, _peer) = pair();
        let mut empty = Operation::new(id, Kind::Read, 0);
        let Output::Bytes(output) = empty.poll(waiter).unwrap() else {
            panic!()
        };
        assert_eq!((output.failed, output.array.len), (0, 0));
        zore_native_net_close_handle(id);
    }
}
