use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::strconv::ValueError;
use super::sys::ErrorOut;

unsafe extern "C" {
    fn zore_adler32_create() -> *mut c_void;
    fn zore_adler32_write(state: *mut c_void, data: *const u8, length: u64) -> i32;
    fn zore_adler32_sum(state: *const c_void) -> u32;
    fn zore_adler32_free(state: *mut c_void);
}

static LIVE_STATES: AtomicUsize = AtomicUsize::new(0);

struct State(NonNull<c_void>);

// C state has no thread affinity and every access holds its resource mutex.
unsafe impl Send for State {}

impl Drop for State {
    fn drop(&mut self) {
        // SAFETY: this is the unique owner of the C allocation.
        unsafe { zore_adler32_free(self.0.as_ptr()) };
        LIVE_STATES.fetch_sub(1, Ordering::Relaxed);
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

fn create() -> Result<i64, &'static str> {
    let mut registry = registry()
        .lock()
        .map_err(|_| "native adler32: registry unavailable")?;
    let next = registry
        .next
        .checked_add(1)
        .ok_or("native adler32: token space exhausted")?;
    // SAFETY: the registered C constructor takes no arguments.
    let state = NonNull::new(unsafe { zore_adler32_create() })
        .ok_or("native adler32: allocation failed")?;
    LIVE_STATES.fetch_add(1, Ordering::Relaxed);
    let token = registry.next;
    registry.next = next;
    registry
        .states
        .insert(token, Arc::new(Mutex::new(State(state))));
    Ok(token)
}

fn state(token: i64) -> Result<Arc<Mutex<State>>, &'static str> {
    registry()
        .lock()
        .map_err(|_| "native adler32: registry unavailable")?
        .states
        .get(&token)
        .cloned()
        .ok_or("native adler32: invalid or closed handle")
}

pub(super) fn live_states() -> usize {
    LIVE_STATES.load(Ordering::Relaxed)
}

/// # Safety
/// out must point to writable ValueError storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_adler32_native_create(out: *mut ValueError) {
    let result = match create() {
        Ok(token) => ValueError::ok(token),
        Err(message) => ValueError::failed_text(message),
    };
    // SAFETY: the generated caller supplies writable result storage.
    unsafe { out.write(result) };
}

/// # Safety
/// out is writable and data holds length readable bytes when length is positive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_adler32_native_write(
    out: *mut ErrorOut,
    token: i64,
    data: *const u8,
    length: i64,
) {
    let result = (|| {
        if !(0..=65536).contains(&length) || (length != 0 && data.is_null()) {
            return Err("native adler32: write requires at most 65536 readable bytes");
        }
        let state = state(token)?;
        let state = state
            .lock()
            .map_err(|_| "native adler32: resource unavailable")?;
        // SAFETY: the state is live and locked, and the caller lends the validated buffer.
        let status = unsafe { zore_adler32_write(state.0.as_ptr(), data, length as u64) };
        if status != 0 {
            return Err("native adler32: invalid input");
        }
        Ok(())
    })();
    let result = match result {
        Ok(()) => ErrorOut::ok(),
        Err(message) => ErrorOut::failed(message),
    };
    // SAFETY: the generated caller supplies writable result storage.
    unsafe { out.write(result) };
}

/// # Safety
/// out must point to writable ValueError storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_adler32_native_sum(out: *mut ValueError, token: i64) {
    let result = state(token).and_then(|state| {
        let state = state
            .lock()
            .map_err(|_| "native adler32: resource unavailable")?;
        // SAFETY: this lease owns a live state and holds its mutex.
        Ok(unsafe { zore_adler32_sum(state.0.as_ptr()) } as i64)
    });
    let result = match result {
        Ok(value) => ValueError::ok(value),
        Err(message) => ValueError::failed_text(message),
    };
    // SAFETY: the generated caller supplies writable result storage.
    unsafe { out.write(result) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_adler32_native_close(token: i64) {
    let removed = registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .states
        .remove(&token);
    drop(removed);
}
