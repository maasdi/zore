//! The code behind `zore/net`. Handles are numbers that are never reused, so a stale handle
//! can only fail.

use std::collections::HashMap;
use std::io::ErrorKind;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use super::reactor::Pollable;
use super::strconv::ValueError;
use super::string::StringOut;
use super::sys::{ByteArrayError, ErrorOut, instant_of, text_of};
use super::{blocking, reactor};

struct Connection {
    stream: TcpStream,
    /// `time.Now` readings after which reads and writes give up; zero or less means none.
    read_deadline: AtomicI64,
    write_deadline: AtomicI64,
}

struct Listener {
    socket: TcpListener,
    deadline: AtomicI64,
}

enum Handle {
    Listener(Listener),
    Connection(Connection),
}

static NEXT: AtomicU64 = AtomicU64::new(1);
static TABLE: Mutex<Option<HashMap<u64, Arc<Handle>>>> = Mutex::new(None);

fn table() -> MutexGuard<'static, Option<HashMap<u64, Arc<Handle>>>> {
    TABLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn insert(handle: Handle) -> i64 {
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    table()
        .get_or_insert_with(HashMap::new)
        .insert(id, Arc::new(handle));
    id as i64
}

fn lookup(id: i64) -> Option<Arc<Handle>> {
    table().as_ref()?.get(&u64::try_from(id).ok()?).cloned()
}

fn remove(id: i64) -> Option<Arc<Handle>> {
    table().as_mut()?.remove(&u64::try_from(id).ok()?)
}

fn connection(stream: TcpStream) -> Handle {
    Handle::Connection(Connection {
        stream,
        read_deadline: AtomicI64::new(0),
        write_deadline: AtomicI64::new(0),
    })
}

fn prepare(stream: &TcpStream) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_nonblocking(reactor::POLLED)
}

fn deadline_of(reading: &AtomicI64) -> Option<Instant> {
    instant_of(reading.load(Ordering::SeqCst))
}

/// The addresses `address` names, keeping only IPv4 for family 4 and IPv6 for family 6.
fn resolve(family: i64, address: &str) -> std::io::Result<Vec<SocketAddr>> {
    let candidates: Vec<SocketAddr> = address
        .to_socket_addrs()?
        .filter(|candidate| match family {
            4 => candidate.is_ipv4(),
            6 => candidate.is_ipv6(),
            _ => true,
        })
        .collect();
    if candidates.is_empty() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "no suitable address found",
        ));
    }
    Ok(candidates)
}

/// Runs `attempt` until it stops reporting that it would block, waiting for the descriptor in
/// between; gives up with a timeout error once `deadline` passes.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn until_ready<T>(
    descriptor: &impl Pollable,
    write: bool,
    deadline: &AtomicI64,
    mut attempt: impl FnMut() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let _blocking = (!reactor::POLLED).then(super::scheduler::BlockingGuard::enter);
    loop {
        match attempt() {
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                let deadline = deadline_of(deadline);
                if deadline.is_some_and(|when| Instant::now() >= when) {
                    return Err(std::io::Error::new(ErrorKind::TimedOut, "timed out"));
                }
                reactor::wait_fd(descriptor.descriptor(), write, deadline);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            other => return other,
        }
    }
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_listen(
    out: *mut ValueError,
    family: i64,
    address: *const u8,
    address_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let address = unsafe { text_of(address, address_len) };
    let result: std::io::Result<TcpListener> = blocking::run(move || {
        let listener = TcpListener::bind(&resolve(family, &address)?[..])?;
        listener.set_nonblocking(reactor::POLLED)?;
        Ok(listener)
    });
    let value = match result {
        Ok(socket) => ValueError::ok(insert(Handle::Listener(Listener {
            socket,
            deadline: AtomicI64::new(0),
        }))),
        Err(error) => ValueError::failed_text(&format!("net.Listen: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_accept(out: *mut ValueError, id: i64) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let value = poll::accept(id);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let value = match lookup(id).as_deref() {
        Some(Handle::Listener(listener)) => {
            let accepted = until_ready(&listener.socket, false, &listener.deadline, || {
                listener.socket.accept()
            });
            match accepted.and_then(|(stream, _)| prepare(&stream).map(|()| stream)) {
                Ok(stream) => ValueError::ok(insert(connection(stream))),
                Err(error) => ValueError::failed_text(&format!("net.Accept: {error}")),
            }
        }
        _ => ValueError::failed_text("net.Accept: not an open listener"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

fn connect(family: i64, address: &str, timeout: Option<Duration>) -> std::io::Result<TcpStream> {
    let candidates = resolve(family, address)?;
    let Some(timeout) = timeout else {
        return TcpStream::connect(&candidates[..]);
    };
    let mut last = std::io::Error::new(ErrorKind::InvalidInput, "no addresses to connect to");
    for candidate in candidates {
        match TcpStream::connect_timeout(&candidate, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_dial(
    out: *mut ValueError,
    family: i64,
    address: *const u8,
    address_len: i64,
    timeout: i64,
) {
    // SAFETY: guaranteed by the caller.
    let address = unsafe { text_of(address, address_len) };
    let timeout = u64::try_from(timeout)
        .ok()
        .filter(|&nanoseconds| nanoseconds > 0)
        .map(Duration::from_nanos);
    let result: std::io::Result<TcpStream> = blocking::run(move || {
        let stream = connect(family, &address, timeout)?;
        prepare(&stream)?;
        Ok(stream)
    });
    let value = match result {
        Ok(stream) => ValueError::ok(insert(connection(stream))),
        Err(error) => ValueError::failed_text(&format!("net.Dial: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

fn address_text(address: std::io::Result<SocketAddr>) -> String {
    address
        .map(|address| address.to_string())
        .unwrap_or_default()
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_local_addr(out: *mut StringOut, id: i64) {
    let text = match lookup(id).as_deref() {
        Some(Handle::Listener(listener)) => address_text(listener.socket.local_addr()),
        Some(Handle::Connection(connection)) => address_text(connection.stream.local_addr()),
        None => String::new(),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringOut::built(text.as_bytes())) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_remote_addr(out: *mut StringOut, id: i64) {
    let text = match lookup(id).as_deref() {
        Some(Handle::Connection(connection)) => address_text(connection.stream.peer_addr()),
        _ => String::new(),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringOut::built(text.as_bytes())) };
}

/// Sets the read deadline (`which` 1), the write deadline (2), or both (0).
///
/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_set_deadline(
    out: *mut ErrorOut,
    id: i64,
    which: i64,
    reading: i64,
) {
    let result = match lookup(id).as_deref() {
        Some(Handle::Connection(connection)) => {
            if which != 2 {
                connection.read_deadline.store(reading, Ordering::SeqCst);
            }
            if which != 1 {
                connection.write_deadline.store(reading, Ordering::SeqCst);
            }
            ErrorOut::ok()
        }
        Some(Handle::Listener(listener)) => {
            listener.deadline.store(reading, Ordering::SeqCst);
            ErrorOut::ok()
        }
        None => ErrorOut::failed("net.SetDeadline: not an open connection or listener"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_read(out: *mut ByteArrayError, id: i64, max: i64) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let result = poll::read(id, max);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let result = match (lookup(id).as_deref(), usize::try_from(max)) {
        (Some(Handle::Connection(_)), Ok(0)) => ByteArrayError::ok(&[]),
        (Some(Handle::Connection(connection)), Ok(max)) => {
            let mut stream = &connection.stream;
            let mut chunk = vec![0u8; max];
            match until_ready(&connection.stream, false, &connection.read_deadline, || {
                stream.read(&mut chunk)
            }) {
                Ok(0) => ByteArrayError::failed("EOF"),
                Ok(count) => ByteArrayError::ok(&chunk[..count]),
                Err(error) => ByteArrayError::failed(&format!("net.Read: {error}")),
            }
        }
        _ => ByteArrayError::failed("net.Read: not an open connection"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

/// # Safety
/// `out` must be writable and `data` must point to `data_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_write(
    out: *mut ValueError,
    id: i64,
    data: *const u8,
    data_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { super::bytes(data, data_len) };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let result = poll::write(id, bytes);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let result = match lookup(id).as_deref() {
        Some(Handle::Connection(connection)) => {
            let mut stream = &connection.stream;
            let mut sent = 0;
            let outcome = loop {
                if sent == bytes.len() {
                    break Ok(());
                }
                match until_ready(&connection.stream, true, &connection.write_deadline, || {
                    stream.write(&bytes[sent..])
                }) {
                    Ok(0) => break Err(std::io::Error::from(ErrorKind::WriteZero)),
                    Ok(count) => sent += count,
                    Err(error) => break Err(error),
                }
            };
            match outcome {
                Ok(()) => ValueError::ok(sent as i64),
                Err(error) => ValueError::failed_with(sent as i64, &format!("net.Write: {error}")),
            }
        }
        _ => ValueError::failed_text("net.Write: not an open connection"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_close_write(out: *mut ErrorOut, id: i64) {
    let result = match lookup(id).as_deref() {
        Some(Handle::Connection(connection)) => match connection.stream.shutdown(Shutdown::Write) {
            Ok(()) => ErrorOut::ok(),
            Err(error) => ErrorOut::failed(&format!("net.CloseWrite: {error}")),
        },
        _ => ErrorOut::failed("net.CloseWrite: not an open connection"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_close(out: *mut ErrorOut, id: i64) {
    let result = match remove(id) {
        Some(handle) => {
            drop(handle);
            ErrorOut::ok()
        }
        None => ErrorOut::failed("net.Close: not an open connection or listener"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_net_close_handle(id: i64) {
    drop(remove(id));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "net_poll.rs"]
mod poll;
