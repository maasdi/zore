use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::blocking;
use super::strconv::ValueError;
use super::sys::{ErrorOut, text_of};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::staged_tree_unix::Tree;
#[cfg(windows)]
use super::staged_tree_windows::Tree;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
struct Tree;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl Tree {
    fn create(_: &str, _: usize) -> std::io::Result<Self> {
        Err(unsupported())
    }
    fn mkdir(&mut self, _: &str) -> std::io::Result<()> {
        Err(unsupported())
    }
    fn create_file(&mut self, _: &str, _: bool) -> std::io::Result<()> {
        Err(unsupported())
    }
    fn file(&mut self) -> std::io::Result<&mut std::fs::File> {
        Err(unsupported())
    }
    fn finish_file(&mut self) -> std::io::Result<()> {
        Err(unsupported())
    }
    fn commit(&mut self) -> std::io::Result<()> {
        Err(unsupported())
    }
    fn abort(&mut self) -> std::io::Result<()> {
        Err(unsupported())
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn unsupported() -> std::io::Error {
    std::io::Error::other("confined tree extraction is unsupported on this platform")
}

type Handle = Arc<Mutex<Tree>>;
static NEXT: AtomicU64 = AtomicU64::new(1);
static TABLE: Mutex<Option<HashMap<u64, Handle>>> = Mutex::new(None);

pub(crate) fn live_handles() -> usize {
    TABLE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map_or(0, HashMap::len)
}
fn lookup(id: i64) -> std::io::Result<Handle> {
    TABLE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .and_then(|t| t.get(&(id as u64)))
        .cloned()
        .ok_or_else(|| std::io::Error::other("not an open staged tree"))
}
fn remove(id: i64) -> Option<Handle> {
    TABLE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_mut()?
        .remove(&(id as u64))
}
fn operation(id: i64, action: fn(&mut Tree) -> std::io::Result<()>) -> Option<String> {
    blocking::run(move || {
        lookup(id)
            .and_then(|h| action(&mut h.lock().unwrap_or_else(|p| p.into_inner())))
            .err()
            .map(|e| format!("os.StagedTree: {e}"))
    })
}

/// # Safety
/// `out` is writable and `destination` is readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_tree_stage(
    out: *mut ValueError,
    destination: *const u8,
    len: i64,
    limit: i64,
) {
    // SAFETY: the compiler supplies valid string storage.
    let destination = unsafe { text_of(destination, len) };
    let result = blocking::run(move || {
        let limit = usize::try_from(limit)
            .ok()
            .filter(|&n| n <= 65534)
            .ok_or_else(|| std::io::Error::other("invalid node limit"))?;
        let tree = Tree::create(&destination, limit)?;
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        TABLE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_or_insert_with(HashMap::new)
            .insert(id, Arc::new(Mutex::new(tree)));
        Ok::<_, std::io::Error>(id as i64)
    });
    let value = match result {
        Ok(id) => ValueError::ok(id),
        Err(e) => ValueError::failed_text(&format!("os.StageTree: {e}")),
    };
    // SAFETY: the compiler supplies writable result storage.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` is writable and `name` is readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_tree_mkdir(
    out: *mut ErrorOut,
    id: i64,
    name: *const u8,
    len: i64,
) {
    // SAFETY: the compiler supplies valid string storage.
    let name = unsafe { text_of(name, len) };
    let failure = blocking::run(move || {
        lookup(id)
            .and_then(|h| h.lock().unwrap_or_else(|p| p.into_inner()).mkdir(&name))
            .err()
            .map(|e| format!("os.StagedTree.Mkdir: {e}"))
    });
    // SAFETY: the compiler supplies writable result storage.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// `out` is writable and `name` is readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_tree_create(
    out: *mut ErrorOut,
    id: i64,
    name: *const u8,
    len: i64,
    executable: bool,
) {
    // SAFETY: the compiler supplies valid string storage.
    let name = unsafe { text_of(name, len) };
    let failure = blocking::run(move || {
        lookup(id)
            .and_then(|h| {
                h.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .create_file(&name, executable)
            })
            .err()
            .map(|e| format!("os.StagedTree.Create: {e}"))
    });
    // SAFETY: the compiler supplies writable result storage.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// `out` is writable and `data` is readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_tree_write(
    out: *mut ValueError,
    id: i64,
    data: *const u8,
    len: i64,
) {
    // SAFETY: the compiler supplies valid slice storage.
    let data = unsafe { super::bytes(data, len) }.to_vec();
    let result = blocking::run(move || {
        lookup(id).and_then(|h| {
            h.lock()
                .unwrap_or_else(|p| p.into_inner())
                .file()?
                .write(&data)
        })
    });
    let result = match result {
        Ok(n) => ValueError::ok(n as i64),
        Err(e) => ValueError::failed_text(&format!("os.StagedTree.Write: {e}")),
    };
    // SAFETY: the compiler supplies writable result storage.
    unsafe { out.write(result) };
}

/// # Safety
/// `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_tree_finish_file(out: *mut ErrorOut, id: i64) {
    let failure = operation(id, Tree::finish_file);
    // SAFETY: the compiler supplies writable result storage.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}
fn close(id: i64, commit: bool) -> Option<String> {
    let handle = remove(id)?;
    blocking::run(move || {
        let mut tree = handle.lock().unwrap_or_else(|p| p.into_inner());
        if commit {
            if let Err(original) = tree.commit() {
                return Some(match tree.abort() {
                    Ok(()) => format!("os.StagedTree.Commit: {original}"),
                    Err(cleanup) => {
                        format!("os.StagedTree.Commit: {original}; cleanup failed: {cleanup}")
                    }
                });
            }
            None
        } else {
            tree.abort()
                .err()
                .map(|e| format!("os.StagedTree.Abort: {e}"))
        }
    })
}
/// # Safety
/// `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_tree_commit(out: *mut ErrorOut, id: i64) {
    let failure = close(id, true);
    // SAFETY: the compiler supplies writable result storage.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}
/// # Safety
/// `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_tree_abort(out: *mut ErrorOut, id: i64) {
    let failure = close(id, false);
    // SAFETY: the compiler supplies writable result storage.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}
#[unsafe(no_mangle)]
pub extern "C" fn zore_native_os_tree_drop(id: i64) {
    let _ = close(id, false);
}
