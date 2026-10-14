use std::sync::{Arc, Mutex};

use super::{Code, Handle, PollCode, wait_for};

struct Child {
    handle: Mutex<Option<Box<Handle>>>,
}

pub struct Scope {
    children: Vec<Arc<Child>>,
}

pub struct ScopedHandle {
    child: Arc<Child>,
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_scope_new() -> *mut Scope {
    Box::into_raw(Box::new(Scope {
        children: Vec::new(),
    }))
}

impl Scope {
    fn register(&mut self, handle: *mut Handle) -> *mut ScopedHandle {
        // SAFETY: registration receives the unique newly spawned handle.
        let handle = unsafe { Box::from_raw(handle) };
        let child = Arc::new(Child {
            handle: Mutex::new(Some(handle)),
        });
        self.children.push(Arc::clone(&child));
        Box::into_raw(Box::new(ScopedHandle { child }))
    }
}

/// # Safety
/// Scope must be live and exclusively accessed; task callbacks own the allocated block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_scope_spawn(
    scope: *mut Scope,
    entry: Code,
    drop_results: Code,
    block: *mut u8,
    size: i64,
) -> *mut ScopedHandle {
    let handle = unsafe { super::zore_task_spawn(entry, drop_results, block, size) };
    unsafe { &mut *scope }.register(handle)
}

/// # Safety
/// Scope must be live and exclusively accessed; polling callbacks own the allocated block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_scope_spawn_poll(
    scope: *mut Scope,
    entry: PollCode,
    drop_results: Code,
    block: *mut u8,
    size: i64,
) -> *mut ScopedHandle {
    let handle = unsafe { super::zore_task_spawn_poll(entry, drop_results, block, size) };
    unsafe { &mut *scope }.register(handle)
}

/// # Safety
/// Consume one live scoped handle, with its matching result-block size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_scope_wait(handle: *mut ScopedHandle, size: i64) -> *mut u8 {
    if handle.is_null() {
        super::super::panic::raise(b"wait on a nil scoped task");
        return super::zeroed_block(size);
    }
    let handle = unsafe { Box::from_raw(handle) };
    let child = handle
        .child
        .handle
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    match child {
        Some(child) => unsafe { super::zore_task_wait(Box::into_raw(child), size) },
        None => {
            super::super::panic::raise(b"wait on a completed scoped task");
            super::zeroed_block(size)
        }
    }
}

/// # Safety
/// Consume one uniquely owned scoped handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_scope_handle_drop(handle: *mut ScopedHandle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// # Safety
/// Consume a live, exclusive scope or null before releasing any borrowed parent storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_scope_join(scope: *mut Scope) {
    if scope.is_null() {
        return;
    }
    let scope = unsafe { Box::from_raw(scope) };
    let mut completed = Vec::new();
    let mut first_panic = None;
    for child in &scope.children {
        let handle = child
            .handle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let Some(handle) = handle else {
            continue;
        };
        wait_for(&handle.shared);
        let failure = handle.shared.lock().panic.take();
        let panicked = failure.is_some();
        if first_panic.is_none() {
            first_panic = failure;
        }
        completed.push((handle, panicked));
    }
    if !super::super::panic::zore_panic_pending()
        && let Some(message) = first_panic
    {
        super::super::panic::raise(&message);
    }
    for (handle, panicked) in completed.into_iter().rev() {
        handle.block.discard(panicked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::{zore_alloc, zore_free};
    use crate::slot::Slot;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    struct Probe {
        drops: Arc<AtomicUsize>,
        gate: Option<Arc<Slot>>,
        failure: &'static [u8],
    }

    unsafe extern "C" fn entry(block: *mut u8) {
        let probe = unsafe { block.cast::<Probe>().read() };
        if let Some(gate) = &probe.gate {
            gate.park();
        }
        if !probe.failure.is_empty() {
            let failure = probe.failure;
            drop(probe);
            crate::panic::raise(failure);
        } else {
            unsafe { block.cast::<Probe>().write(probe) };
        }
    }

    unsafe extern "C" fn discard(block: *mut u8) {
        let probe = unsafe { block.cast::<Probe>().read() };
        probe.drops.fetch_add(1, Ordering::SeqCst);
    }

    fn spawn(scope: *mut Scope, probe: Probe) -> (*mut ScopedHandle, Arc<super::super::Shared>) {
        let size = std::mem::size_of::<Probe>() as i64;
        let block = zore_alloc(size);
        // SAFETY: the unique allocation transfers one Probe through entry and result destruction.
        unsafe { block.cast::<Probe>().write(probe) };
        let handle = unsafe { zore_scope_spawn(scope, entry, discard, block, size) };
        // SAFETY: this test retains the unique handle and scope until join or retrieval.
        let shared = unsafe { &*handle }
            .child
            .handle
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .shared
            .clone();
        (handle, shared)
    }

    fn completed(shared: &super::super::Shared) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = shared.lock();
        while !state.finished {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "child did not complete");
            state = shared.finished.wait_timeout(state, remaining).unwrap().0;
        }
    }

    #[test]
    fn scope_handle_drop_retains_registration_until_join_and_releases_every_record() {
        let scope = zore_scope_new();
        let drops = Arc::new(AtomicUsize::new(0));
        let (handle, shared) = spawn(
            scope,
            Probe {
                drops: drops.clone(),
                gate: None,
                failure: b"",
            },
        );
        let weak = Arc::downgrade(&unsafe { &*handle }.child);
        unsafe { zore_scope_handle_drop(handle) };
        completed(&shared);
        assert!(weak.upgrade().is_some());
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        unsafe { zore_scope_join(scope) };
        assert!(weak.upgrade().is_none());
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn scope_explicit_retrieval_transfers_the_result_once_and_join_does_not_discard_it_again() {
        let scope = zore_scope_new();
        let drops = Arc::new(AtomicUsize::new(0));
        let (handle, shared) = spawn(
            scope,
            Probe {
                drops: drops.clone(),
                gate: None,
                failure: b"",
            },
        );
        completed(&shared);
        let size = std::mem::size_of::<Probe>() as i64;
        let result = unsafe { zore_scope_wait(handle, size) };
        unsafe {
            discard(result);
            zore_free(result, size);
            zore_scope_join(scope);
        }
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn scope_failure_priority_uses_spawn_order_instead_of_completion_order() {
        let scope = zore_scope_new();
        let gate = Arc::new(Slot::default());
        let drops = Arc::new(AtomicUsize::new(0));
        let (first, first_state) = spawn(
            scope,
            Probe {
                drops: drops.clone(),
                gate: Some(gate.clone()),
                failure: b"first",
            },
        );
        let (second, second_state) = spawn(
            scope,
            Probe {
                drops,
                gate: None,
                failure: b"second",
            },
        );
        completed(&second_state);
        assert!(!first_state.lock().finished);
        gate.wake();
        completed(&first_state);
        unsafe {
            zore_scope_handle_drop(first);
            zore_scope_handle_drop(second);
            zore_scope_join(scope);
        }
        assert_eq!(crate::panic::take(), Some(b"first".to_vec()));
    }

    #[test]
    fn scope_parent_panic_survives_unobserved_child_panic_and_joined_result_cleanup() {
        let scope = zore_scope_new();
        let drops = Arc::new(AtomicUsize::new(0));
        let (failed, failure) = spawn(
            scope,
            Probe {
                drops: drops.clone(),
                gate: None,
                failure: b"child",
            },
        );
        let (ok, success) = spawn(
            scope,
            Probe {
                drops: drops.clone(),
                gate: None,
                failure: b"",
            },
        );
        completed(&failure);
        completed(&success);
        crate::panic::raise(b"parent");
        unsafe {
            zore_scope_handle_drop(failed);
            zore_scope_handle_drop(ok);
            zore_scope_join(scope);
        }
        assert_eq!(crate::panic::take(), Some(b"parent".to_vec()));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
