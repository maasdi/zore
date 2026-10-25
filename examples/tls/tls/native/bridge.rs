use super::native;
use super::strconv::ValueError;
use super::sys::{ByteArrayError, ErrorOut};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

unsafe extern "C" {
    fn zore_tls_init() -> i32;
    fn zore_tls_config(
        server: i32,
        cert: *const u8,
        cert_len: usize,
        key: *const u8,
        key_len: usize,
        versions: i32,
        error: *mut c_char,
    ) -> *mut c_void;
    fn zore_tls_config_free(config: *mut c_void);
    fn zore_tls_session(
        config: *mut c_void,
        name: *const c_char,
        error: *mut c_char,
    ) -> *mut c_void;
    fn zore_tls_session_free(session: *mut c_void);
    fn zore_tls_handshake(session: *mut c_void, error: *mut c_char) -> i32;
    fn zore_tls_read(session: *mut c_void, data: *mut u8, length: usize, error: *mut c_char)
    -> i32;
    fn zore_tls_write(
        session: *mut c_void,
        data: *const u8,
        length: usize,
        error: *mut c_char,
    ) -> i32;
    fn zore_tls_feed(
        session: *mut c_void,
        data: *const u8,
        length: usize,
        error: *mut c_char,
    ) -> i32;
    fn zore_tls_pending(session: *mut c_void) -> usize;
    fn zore_tls_output(session: *mut c_void, data: *mut u8) -> i32;
    fn zore_tls_status(session: *mut c_void) -> i32;
    fn zore_tls_version(session: *mut c_void) -> i32;
    fn zore_tls_shutdown(session: *mut c_void, error: *mut c_char) -> i32;
    fn zore_tls_system_roots(
        path: *const c_char,
        length: *mut usize,
        error: *mut c_char,
    ) -> *mut u8;
    fn zore_tls_free(pointer: *mut c_void);
}

// The provider's PSA key store and random generator are process-global without threading support.
fn provider() -> MutexGuard<'static, ()> {
    static PROVIDER: Mutex<()> = Mutex::new(());
    PROVIDER.lock().unwrap_or_else(|e| e.into_inner())
}
fn initialize() -> Result<(), String> {
    static INIT: OnceLock<i32> = OnceLock::new();
    let status = *INIT.get_or_init(|| {
        let _provider = provider();
        unsafe { zore_tls_init() }
    });
    if status == 0 {
        Ok(())
    } else {
        Err(format!("TLS provider initialization failed ({status})"))
    }
}

struct State {
    pointer: NonNull<c_void>,
    config: bool,
}
// Provider objects have no thread affinity; sessions are used under their mutex, configs are immutable.
unsafe impl Send for State {}
impl Drop for State {
    fn drop(&mut self) {
        let _provider = provider();
        unsafe {
            if self.config {
                zore_tls_config_free(self.pointer.as_ptr())
            } else {
                zore_tls_session_free(self.pointer.as_ptr())
            }
        }
        native::resource_released();
    }
}
struct Registry {
    next: i64,
    states: BTreeMap<i64, Arc<Mutex<State>>>,
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            next: 1,
            states: BTreeMap::new(),
        })
    })
}
fn insert(pointer: NonNull<c_void>, config: bool) -> Result<i64, String> {
    native::resource_created();
    let state = Arc::new(Mutex::new(State { pointer, config }));
    let mut registry = registry().lock().map_err(|_| "TLS registry unavailable")?;
    let token = registry.next;
    registry.next = token.checked_add(1).ok_or("TLS token space exhausted")?;
    registry.states.insert(token, state);
    Ok(token)
}
fn state(token: i64) -> Result<Arc<Mutex<State>>, String> {
    registry()
        .lock()
        .map_err(|_| "TLS registry unavailable")?
        .states
        .get(&token)
        .cloned()
        .ok_or_else(|| "TLS invalid or closed handle".into())
}
fn message(error: &[c_char; 512]) -> String {
    unsafe { CStr::from_ptr(error.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
fn value(result: Result<i64, String>) -> ValueError {
    match result {
        Ok(n) => ValueError::ok(n),
        Err(e) => ValueError::failed_text(&e),
    }
}
unsafe fn input<'a>(data: *const u8, length: i64, limit: i64) -> Result<&'a [u8], String> {
    if !(0..=limit).contains(&length) || (length != 0 && data.is_null()) {
        return Err("TLS invalid byte input length".into());
    }
    Ok(if length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, length as usize) }
    })
}
fn operation(
    token: i64,
    call: impl FnOnce(*mut c_void, *mut c_char) -> i32,
) -> Result<i64, String> {
    let state = state(token)?;
    let state = state.lock().map_err(|_| "TLS resource unavailable")?;
    if state.config {
        return Err("TLS expected session handle".into());
    }
    let _provider = provider();
    let mut error = [0; 512];
    let result = call(state.pointer.as_ptr(), error.as_mut_ptr());
    if result == -1 {
        Err(message(&error))
    } else {
        Ok(i64::from(result))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_config(
    out: *mut ValueError,
    server: bool,
    cert: *const u8,
    cert_len: i64,
    key: *const u8,
    key_len: i64,
    versions: i64,
) {
    let _blocking = super::scheduler::BlockingGuard::enter();
    let result = (|| {
        let cert = unsafe { input(cert, cert_len, 1048576) }?;
        let key = unsafe { input(key, key_len, 1048576) }?;
        let versions = i32::try_from(versions).map_err(|_| "TLS invalid protocol version range")?;
        initialize()?;
        let mut error = [0; 512];
        let pointer = {
            let _provider = provider();
            NonNull::new(unsafe {
                zore_tls_config(
                    i32::from(server),
                    cert.as_ptr(),
                    cert.len(),
                    key.as_ptr(),
                    key.len(),
                    versions,
                    error.as_mut_ptr(),
                )
            })
        }
        .ok_or_else(|| message(&error))?;
        insert(pointer, true)
    })();
    unsafe { out.write(value(result)) };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_session(
    out: *mut ValueError,
    token: i64,
    name: *const u8,
    name_len: i64,
) {
    let _blocking = super::scheduler::BlockingGuard::enter();
    let result = (|| {
        let name = unsafe { input(name, name_len, 253) }?;
        let name = CString::new(name).map_err(|_| "TLS hostname contains NUL")?;
        let state = state(token)?;
        let config = {
            let config = state.lock().map_err(|_| "TLS config unavailable")?;
            if !config.config {
                return Err("TLS expected configuration handle".into());
            }
            config.pointer
        };
        initialize()?;
        let mut error = [0; 512];
        // The configuration is immutable after creation and `state` keeps it alive.
        let pointer = {
            let _provider = provider();
            NonNull::new(unsafe {
                zore_tls_session(config.as_ptr(), name.as_ptr(), error.as_mut_ptr())
            })
        }
        .ok_or_else(|| message(&error))?;
        insert(pointer, false)
    })();
    unsafe { out.write(value(result)) };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_handshake(out: *mut ValueError, token: i64) {
    unsafe { out.write(value(operation(token, |p, e| zore_tls_handshake(p, e)))) };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_write(
    out: *mut ValueError,
    token: i64,
    data: *const u8,
    length: i64,
) {
    let result = unsafe { input(data, length, 16384) }.and_then(|bytes| {
        operation(token, |p, e| unsafe {
            zore_tls_write(p, bytes.as_ptr(), bytes.len(), e)
        })
    });
    unsafe { out.write(value(result)) };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_feed(
    out: *mut ErrorOut,
    token: i64,
    data: *const u8,
    length: i64,
) {
    let result = unsafe { input(data, length, 65536) }.and_then(|bytes| {
        operation(token, |p, e| unsafe {
            zore_tls_feed(p, bytes.as_ptr(), bytes.len(), e)
        })
    });
    unsafe {
        out.write(match result {
            Ok(_) => ErrorOut::ok(),
            Err(e) => ErrorOut::failed(&e),
        })
    };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_read(
    out: *mut ByteArrayError,
    token: i64,
    max: i64,
) {
    let result = (|| {
        if !(0..=16384).contains(&max) {
            return Err("TLS invalid read size".into());
        }
        let mut data = vec![0; max as usize];
        let count = operation(token, |p, e| unsafe {
            zore_tls_read(p, data.as_mut_ptr(), data.len(), e)
        })?;
        data.truncate(count.max(0) as usize);
        Ok(data)
    })();
    unsafe { out.write(ByteArrayError::from_bytes(result)) };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_output(out: *mut ByteArrayError, token: i64) {
    let mut data = Vec::new();
    let result = operation(token, |p, _| unsafe {
        data.resize(zore_tls_pending(p), 0);
        zore_tls_output(p, data.as_mut_ptr())
    })
    .map(|count| {
        data.truncate(count as usize);
        data
    });
    unsafe { out.write(ByteArrayError::from_bytes(result)) };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_shutdown(out: *mut ErrorOut, token: i64) {
    let result = operation(token, |p, e| unsafe { zore_tls_shutdown(p, e) });
    unsafe {
        out.write(match result {
            Ok(_) => ErrorOut::ok(),
            Err(e) => ErrorOut::failed(&e),
        })
    };
}
#[unsafe(no_mangle)]
pub extern "C" fn zore_native_tls_native_close(token: i64) {
    let removed = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .states
        .remove(&token);
    drop(removed);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_version(out: *mut ValueError, token: i64) {
    unsafe { out.write(value(operation(token, |p, _| zore_tls_version(p)))) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_status(out: *mut ValueError, token: i64) {
    unsafe { out.write(value(operation(token, |p, _| zore_tls_status(p)))) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_tls_native_system_roots(
    out: *mut ByteArrayError,
    path: *const u8,
    path_len: i64,
) {
    let _blocking = super::scheduler::BlockingGuard::enter();
    let result = (|| {
        let path = unsafe { input(path, path_len, 4096) }?;
        let path = CString::new(path).map_err(|_| "TLS path contains NUL")?;
        initialize()?;
        let mut length = 0;
        let mut error = [0; 512];
        let pointer = {
            let _provider = provider();
            unsafe { zore_tls_system_roots(path.as_ptr(), &mut length, error.as_mut_ptr()) }
        };
        if pointer.is_null() {
            return Err(message(&error));
        }
        let roots = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
        unsafe { zore_tls_free(pointer.cast()) };
        Ok(roots)
    })();
    unsafe { out.write(ByteArrayError::from_bytes(result)) };
}
