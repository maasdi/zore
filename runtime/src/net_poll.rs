use super::*;
use crate::waiter::Waiter;

pub struct Operation {
    handle: Option<Arc<Handle>>,
    kind: Kind,
    pending: Vec<u8>,
    buffer: Vec<u8>,
    waiting: Option<Arc<reactor::Operation>>,
    deadline: Option<Instant>,
}

enum Kind {
    Accept,
    Read {
        max: Option<usize>,
        bytes: bool,
    },
    Write {
        data: Vec<u8>,
        sent: usize,
        bytes: bool,
    },
}

enum Output {
    Value(ValueError),
    Text(StringError),
    Bytes(ByteArrayError),
    Error(ErrorOut),
}

impl Operation {
    fn new(id: i64, kind: Kind) -> Self {
        let handle = lookup(id);
        let mut pending = Vec::new();
        let mut buffer = Vec::new();
        if let Kind::Read { max: Some(max), .. } = kind
            && let Some(Handle::Connection(connection)) = handle.as_deref()
        {
            pending = std::mem::take(
                &mut *connection
                    .pending
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
            buffer.resize(max, 0);
        }
        Self {
            handle,
            kind,
            pending,
            buffer,
            waiting: None,
            deadline: None,
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Accept => "net.Accept",
            Kind::Read { bytes: false, .. } => "net.Read",
            Kind::Read { bytes: true, .. } => "net.ReadBytes",
            Kind::Write { bytes: false, .. } => "net.Write",
            Kind::Write { bytes: true, .. } => "net.WriteBytes",
        }
    }

    fn failed(&self, message: &str) -> Output {
        match self.kind {
            Kind::Accept => Output::Value(ValueError::failed_text(message)),
            Kind::Read { bytes: false, .. } => Output::Text(StringError::failed(message)),
            Kind::Read { bytes: true, .. } => Output::Bytes(ByteArrayError::failed(message)),
            Kind::Write { .. } => Output::Error(ErrorOut::failed(message)),
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
        if matches!(self.kind, Kind::Read { max: None, .. }) {
            return Some(self.failed(&format!("{}: max must be positive", self.name())));
        }
        let Some(handle) = self.handle.clone() else {
            return Some(self.failed(&format!(
                "{}: not an open {}",
                self.name(),
                if matches!(self.kind, Kind::Accept) {
                    "listener"
                } else {
                    "connection"
                }
            )));
        };
        loop {
            let (fd, write, timeout, attempted) = match (&mut self.kind, handle.as_ref()) {
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
                        &listener.timeout,
                        result,
                    )
                }
                (
                    Kind::Read {
                        max: Some(max),
                        bytes,
                    },
                    Handle::Connection(connection),
                ) => {
                    if *bytes && !self.pending.is_empty() {
                        let count = (*max).min(self.pending.len());
                        let output = Output::Bytes(ByteArrayError::ok(&self.pending[..count]));
                        self.pending.drain(..count);
                        return Some(output);
                    }
                    let mut stream = &connection.stream;
                    let result = match stream.read(&mut self.buffer) {
                        Ok(0) if self.pending.is_empty() => return Some(self.failed("EOF")),
                        Ok(0) => {
                            self.pending.clear();
                            return Some(self.failed("net.Read: invalid UTF-8"));
                        }
                        Ok(count) if *bytes => Ok(Some(Output::Bytes(ByteArrayError::ok(
                            &self.buffer[..count],
                        )))),
                        Ok(count) => {
                            self.pending.extend_from_slice(&self.buffer[..count]);
                            match std::str::from_utf8(&self.pending) {
                                Ok(text) => {
                                    let output = Output::Text(StringError::ok(text));
                                    self.pending.clear();
                                    Ok(Some(output))
                                }
                                Err(error) if error.error_len().is_some() => {
                                    self.pending.clear();
                                    return Some(self.failed("net.Read: invalid UTF-8"));
                                }
                                Err(error) if error.valid_up_to() > 0 => {
                                    let count = error.valid_up_to();
                                    let text = std::str::from_utf8(&self.pending[..count]).unwrap();
                                    let output = Output::Text(StringError::ok(text));
                                    self.pending.drain(..count);
                                    Ok(Some(output))
                                }
                                Err(_) => Ok(None),
                            }
                        }
                        Err(error) => Err(error),
                    };
                    (
                        connection.stream.descriptor(),
                        false,
                        &connection.timeout,
                        result,
                    )
                }
                (Kind::Write { data, sent, .. }, Handle::Connection(connection)) => {
                    if *sent == data.len() {
                        return Some(Output::Error(ErrorOut::ok()));
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
                        &connection.timeout,
                        result,
                    )
                }
                _ => {
                    return Some(self.failed(&format!(
                        "{}: not an open {}",
                        self.name(),
                        if matches!(self.kind, Kind::Accept) {
                            "listener"
                        } else {
                            "connection"
                        }
                    )));
                }
            };
            match attempted {
                Ok(Some(output)) => return Some(output),
                Ok(None) => {
                    self.deadline = None;
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if self.deadline.is_none() {
                        self.deadline = u64::try_from(timeout.load(Ordering::SeqCst))
                            .ok()
                            .filter(|&ms| ms > 0)
                            .and_then(|ms| Instant::now().checked_add(Duration::from_millis(ms)));
                    }
                    if self
                        .deadline
                        .is_some_and(|deadline| Instant::now() >= deadline)
                    {
                        return Some(self.failed(&format!("{}: timed out", self.name())));
                    }
                    self.waiting = Some(reactor::start_wait_fd(fd, write, self.deadline, waiter));
                    return None;
                }
                Err(error) => return Some(self.failed(&format!("{}: {error}", self.name()))),
            }
        }
    }

    fn restore_pending(&mut self) {
        if matches!(self.kind, Kind::Read { max: Some(_), .. })
            && let Some(Handle::Connection(connection)) = self.handle.as_deref()
        {
            *connection
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                std::mem::take(&mut self.pending);
        }
    }
}

fn blocking(id: i64, kind: Kind) -> Output {
    let mut operation = Operation::new(id, kind);
    loop {
        let slot = Arc::new(crate::slot::Slot::default());
        if let Some(output) = operation.poll(Arc::clone(&slot).into()) {
            operation.restore_pending();
            return output;
        }
        slot.park();
    }
}

pub(super) fn accept(id: i64) -> ValueError {
    let Output::Value(output) = blocking(id, Kind::Accept) else {
        unreachable!()
    };
    output
}

pub(super) fn read(id: i64, max: i64) -> StringError {
    let Output::Text(output) = blocking(
        id,
        Kind::Read {
            max: usize::try_from(max).ok().filter(|&max| max > 0),
            bytes: false,
        },
    ) else {
        unreachable!()
    };
    output
}

pub(super) fn read_bytes(id: i64, max: i64) -> ByteArrayError {
    let Output::Bytes(output) = blocking(
        id,
        Kind::Read {
            max: usize::try_from(max).ok().filter(|&max| max > 0),
            bytes: true,
        },
    ) else {
        unreachable!()
    };
    output
}

pub(super) fn write(id: i64, data: &[u8], bytes: bool) -> ErrorOut {
    let Output::Error(output) = blocking(
        id,
        Kind::Write {
            data: data.to_vec(),
            sent: 0,
            bytes,
        },
    ) else {
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
    Box::into_raw(Box::new(Operation::new(id, Kind::Accept)))
}

/// # Safety
/// `context` must be valid for this call; the returned operation has one serial poller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_read_start(
    id: i64,
    max: i64,
    _context: *mut crate::scheduler::Context,
) -> *mut Operation {
    Box::into_raw(Box::new(Operation::new(
        id,
        Kind::Read {
            max: usize::try_from(max).ok().filter(|&max| max > 0),
            bytes: false,
        },
    )))
}

/// # Safety
/// `context` must be valid for this call; the returned operation has one serial poller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_read_bytes_start(
    id: i64,
    max: i64,
    _context: *mut crate::scheduler::Context,
) -> *mut Operation {
    Box::into_raw(Box::new(Operation::new(
        id,
        Kind::Read {
            max: usize::try_from(max).ok().filter(|&max| max > 0),
            bytes: true,
        },
    )))
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
        Kind::Write {
            data,
            sent: 0,
            bytes: false,
        },
    )))
}

/// # Safety
/// `data` holds `len` readable bytes and context is live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_write_bytes_start(
    id: i64,
    data: *const u8,
    len: i64,
    _context: *mut crate::scheduler::Context,
) -> *mut Operation {
    let data = unsafe { crate::bytes(data, len) }.to_vec();
    Box::into_raw(Box::new(Operation::new(
        id,
        Kind::Write {
            data,
            sent: 0,
            bytes: true,
        },
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
    waiting.restore_pending();
    unsafe {
        match result {
            Output::Value(value) => output.cast::<ValueError>().write(value),
            Output::Text(value) => output.cast::<StringError>().write(value),
            Output::Bytes(value) => output.cast::<ByteArrayError>().write(value),
            Output::Error(value) => output.cast::<ErrorOut>().write(value),
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
            Output::Text(value) => {
                assert_eq!(value.len, 0);
                (value.message, value.message_len)
            }
            Output::Bytes(value) => {
                assert_eq!(value.array.len, 0);
                (value.message, value.message_len)
            }
            Output::Value(value) => {
                assert_eq!(value.value, 0);
                (value.message, value.message_len)
            }
            Output::Error(value) => (value.message, value.message_len),
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
                operation = zore_native_net_read_bytes_start(id, 1, context) as usize;
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
    fn timed_out_text_preserves_incomplete_bytes_for_a_later_raw_read() {
        let _serial = crate::string::serial();
        let (id, mut peer) = pair();
        let handle = lookup(id).unwrap();
        let Handle::Connection(connection) = handle.as_ref() else {
            panic!()
        };
        let mut operation = Operation::new(
            id,
            Kind::Read {
                max: Some(1),
                bytes: false,
            },
        );
        let (waiter, rx) = signal();
        assert!(operation.poll(waiter.clone()).is_none());
        assert!(operation.pending.is_empty());
        assert!(operation.deadline.is_none());
        peer.write_all(b"\xe2").unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        connection.timeout.store(100, Ordering::SeqCst);
        assert!(operation.poll(waiter.clone()).is_none());
        let deadline = operation.deadline;
        assert!(deadline.is_some());
        assert_eq!(operation.pending, b"\xe2");
        for _ in 0..20 {
            assert!(operation.poll(waiter.clone()).is_none());
            assert_eq!(operation.deadline, deadline);
        }
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let output = operation.poll(waiter).unwrap();
        operation.restore_pending();
        assert_eq!(error(output), "net.Read: timed out");
        let mut raw = Operation::new(
            id,
            Kind::Read {
                max: Some(1),
                bytes: true,
            },
        );
        let (waiter, _) = signal();
        let Output::Bytes(output) = raw.poll(waiter).unwrap() else {
            panic!()
        };
        raw.restore_pending();
        assert_eq!(
            unsafe { crate::bytes(output.array.data, output.array.len) },
            b"\xe2"
        );
        unsafe {
            crate::alloc::zore_free(output.array.data, output.array.cap);
        }
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
                operation = zore_native_net_write_bytes_start(
                    id,
                    data.as_ptr(),
                    data.len() as i64,
                    context,
                ) as usize;
                let mut output = std::mem::MaybeUninit::<ErrorOut>::uninit();
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
                let mut output = std::mem::MaybeUninit::<ErrorOut>::uninit();
                if zore_native_net_poll(
                    operation as *mut Operation,
                    context,
                    output.as_mut_ptr().cast(),
                ) == 0
                {
                    return Poll::Pending;
                }
                assert_eq!(output.assume_init().failed, 0);
                zore_native_net_close_handle(id);
                Poll::Ready
            }
        });
        pool.idle();
        reader.join().unwrap();
    }

    #[test]
    fn invalid_handles_and_invalid_sizes_complete_without_registration() {
        let _serial = crate::string::serial();
        let (waiter, _) = signal();
        let mut accept = Operation::new(0, Kind::Accept);
        assert_eq!(
            error(accept.poll(waiter.clone()).unwrap()),
            "net.Accept: not an open listener"
        );
        let mut read = Operation::new(
            0,
            Kind::Read {
                max: None,
                bytes: false,
            },
        );
        assert_eq!(
            error(read.poll(waiter.clone()).unwrap()),
            "net.Read: max must be positive"
        );
        let mut write = Operation::new(
            0,
            Kind::Write {
                data: Vec::new(),
                sent: 0,
                bytes: true,
            },
        );
        assert_eq!(
            error(write.poll(waiter).unwrap()),
            "net.WriteBytes: not an open connection"
        );
    }
}
