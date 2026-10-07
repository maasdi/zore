//! The code behind `zore/net`. Handles are numbers that are never reused, so a stale handle
//! can only fail.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::reactor::Pollable;
use super::strconv::ValueError;
use super::sys::{ErrorOut, StringError};
use super::{blocking, reactor};

struct Connection {
    stream: TcpStream,
    /// Bytes read but not yet returned: the start of a character that is not complete.
    pending: Mutex<Vec<u8>>,
}

enum Handle {
    Listener(TcpListener),
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

/// # Safety
/// The string must satisfy the storage rule of `bytes`.
unsafe fn text_of(data: *const u8, len: i64) -> String {
    // SAFETY: guaranteed by the caller.
    String::from_utf8_lossy(unsafe { super::bytes(data, len) }).into_owned()
}

fn prepare(stream: &TcpStream) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_nonblocking(reactor::POLLED)
}

/// Runs `attempt` until it stops reporting that it would block, waiting for the descriptor in between.
fn until_ready<T>(
    descriptor: &impl Pollable,
    write: bool,
    mut attempt: impl FnMut() -> std::io::Result<T>,
) -> std::io::Result<T> {
    loop {
        match attempt() {
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                reactor::wait_fd(descriptor.descriptor(), write);
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
    address: *const u8,
    address_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let address = unsafe { text_of(address, address_len) };
    let result: std::io::Result<TcpListener> = blocking::run(move || {
        let listener = TcpListener::bind(&address)?;
        listener.set_nonblocking(reactor::POLLED)?;
        Ok(listener)
    });
    let value = match result {
        Ok(listener) => ValueError::ok(insert(Handle::Listener(listener))),
        Err(error) => ValueError::failed_text(&format!("net.Listen: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_net_port(id: i64) -> i64 {
    match lookup(id).as_deref() {
        Some(Handle::Listener(listener)) => listener
            .local_addr()
            .map_or(-1, |address| i64::from(address.port())),
        _ => -1,
    }
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_accept(out: *mut ValueError, id: i64) {
    let value = match lookup(id).as_deref() {
        Some(Handle::Listener(listener)) => {
            let accepted = until_ready(listener, false, || listener.accept());
            match accepted.and_then(|(stream, _)| prepare(&stream).map(|()| stream)) {
                Ok(stream) => ValueError::ok(insert(Handle::Connection(Connection {
                    stream,
                    pending: Mutex::new(Vec::new()),
                }))),
                Err(error) => ValueError::failed_text(&format!("net.Accept: {error}")),
            }
        }
        _ => ValueError::failed_text("net.Accept: not an open listener"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_dial(
    out: *mut ValueError,
    address: *const u8,
    address_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let address = unsafe { text_of(address, address_len) };
    let result: std::io::Result<TcpStream> = blocking::run(move || {
        let stream = TcpStream::connect(&address)?;
        prepare(&stream)?;
        Ok(stream)
    });
    let value = match result {
        Ok(stream) => ValueError::ok(insert(Handle::Connection(Connection {
            stream,
            pending: Mutex::new(Vec::new()),
        }))),
        Err(error) => ValueError::failed_text(&format!("net.Dial: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// Reads more bytes into `pending` until it starts with a whole character.
fn read_text(connection: &Connection, max: usize) -> StringError {
    let mut pending = std::mem::take(
        &mut *connection
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    );
    let mut stream = &connection.stream;
    let mut chunk = vec![0u8; max];
    let result = loop {
        match until_ready(&connection.stream, false, || stream.read(&mut chunk)) {
            Ok(0) if pending.is_empty() => break StringError::failed("EOF"),
            Ok(0) => {
                pending.clear();
                break StringError::failed("net.Read: invalid UTF-8");
            }
            Ok(count) => pending.extend_from_slice(&chunk[..count]),
            Err(error) => break StringError::failed(&format!("net.Read: {error}")),
        }
        match std::str::from_utf8(&pending) {
            Ok(text) => {
                let result = StringError::ok(text);
                pending.clear();
                break result;
            }
            Err(error) if error.error_len().is_some() => {
                pending.clear();
                break StringError::failed("net.Read: invalid UTF-8");
            }
            Err(error) if error.valid_up_to() > 0 => {
                let whole = error.valid_up_to();
                let text = String::from_utf8_lossy(&pending[..whole]).into_owned();
                pending.drain(..whole);
                break StringError::ok(&text);
            }
            Err(_) => {}
        }
    };
    *connection
        .pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = pending;
    result
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_read(out: *mut StringError, id: i64, max: i64) {
    let result = match (lookup(id).as_deref(), usize::try_from(max)) {
        (_, Ok(0) | Err(_)) => StringError::failed("net.Read: max must be positive"),
        (Some(Handle::Connection(connection)), Ok(max)) => read_text(connection, max),
        _ => StringError::failed("net.Read: not an open connection"),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_write(
    out: *mut ErrorOut,
    id: i64,
    text: *const u8,
    text_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { super::bytes(text, text_len) };
    let result = match lookup(id).as_deref() {
        Some(Handle::Connection(connection)) => {
            let mut stream = &connection.stream;
            let mut sent = 0;
            let outcome = loop {
                if sent == bytes.len() {
                    break Ok(());
                }
                match until_ready(&connection.stream, true, || stream.write(&bytes[sent..])) {
                    Ok(0) => break Err(std::io::Error::from(ErrorKind::WriteZero)),
                    Ok(count) => sent += count,
                    Err(error) => break Err(error),
                }
            };
            match outcome {
                Ok(()) => ErrorOut::ok(),
                Err(error) => ErrorOut::failed(&format!("net.Write: {error}")),
            }
        }
        _ => ErrorOut::failed("net.Write: not an open connection"),
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

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_net_close_handle(id: i64) {
    let Ok(id) = u64::try_from(id) else {
        return;
    };
    let closed = table().as_mut().and_then(|handles| handles.remove(&id));
    drop(closed);
}
