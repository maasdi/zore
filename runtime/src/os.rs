//! The code behind `zore/os`. File handles are numbers that are never reused, so a stale handle
//! can only fail; 1, 2, and 3 are standard input, output, and error.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::blocking;
use super::strconv::ValueError;
use super::string::StringOut;
use super::sys::{ByteArrayError, ErrorOut, StringError, WordArray, WordArrayError, text_of};

enum Handle {
    Stdin,
    Stdout,
    Stderr,
    File(std::fs::File),
}

static NEXT: AtomicU64 = AtomicU64::new(4);
static TABLE: Mutex<Option<HashMap<u64, Arc<Handle>>>> = Mutex::new(None);

fn table() -> MutexGuard<'static, Option<HashMap<u64, Arc<Handle>>>> {
    TABLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lookup(id: i64) -> Option<Arc<Handle>> {
    match id {
        1 => Some(Arc::new(Handle::Stdin)),
        2 => Some(Arc::new(Handle::Stdout)),
        3 => Some(Arc::new(Handle::Stderr)),
        _ => table().as_ref()?.get(&u64::try_from(id).ok()?).cloned(),
    }
}

fn insert(file: std::fs::File) -> i64 {
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    table()
        .get_or_insert_with(HashMap::new)
        .insert(id, Arc::new(Handle::File(file)));
    id as i64
}

fn remove(id: i64) -> Option<Arc<Handle>> {
    let id = u64::try_from(id).ok().filter(|&id| id > 3)?;
    table().as_mut()?.remove(&id)
}

fn is_standard(id: i64) -> bool {
    (1..=3).contains(&id)
}

/// # Safety
/// `out` must be writable and the path must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_read_file(
    out: *mut ByteArrayError,
    path: *const u8,
    path_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let result = blocking::run(move || {
        std::fs::read(&path).map_err(|error| format!("os.ReadFile: {error}"))
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ByteArrayError::from_bytes(result)) };
}

fn create_with(path: &str, perm: i64) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(u32::try_from(perm & 0o7777).unwrap_or(0o644));
    }
    #[cfg(not(unix))]
    let _ = perm;
    options.open(path)
}

/// # Safety
/// `out` must be writable; the path and the bytes must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_write_file(
    out: *mut ErrorOut,
    path: *const u8,
    path_len: i64,
    data: *const u8,
    data_len: i64,
    perm: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (path, data) = unsafe {
        (
            text_of(path, path_len),
            super::bytes(data, data_len).to_vec(),
        )
    };
    let failure = blocking::run(move || {
        create_with(&path, perm)
            .and_then(|mut file| file.write_all(&data))
            .err()
            .map(|error| format!("os.WriteFile: {error}"))
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// `out` must be writable and the path must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_mkdir_all(
    out: *mut ErrorOut,
    path: *const u8,
    path_len: i64,
    perm: i64,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let failure = blocking::run(move || {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(u32::try_from(perm & 0o7777).unwrap_or(0o755));
        }
        #[cfg(not(unix))]
        let _ = perm;
        builder
            .create(&path)
            .err()
            .map(|error| format!("os.MkdirAll: {error}"))
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// `out` must be writable and the path must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_remove(out: *mut ErrorOut, path: *const u8, path_len: i64) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let failure = blocking::run(move || {
        let removed = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => std::fs::remove_dir(&path),
            _ => std::fs::remove_file(&path),
        };
        removed.err().map(|error| format!("os.Remove: {error}"))
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// `out` must be writable and the path must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_remove_all(
    out: *mut ErrorOut,
    path: *const u8,
    path_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let failure = blocking::run(move || {
        let removed = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(&path),
            Ok(_) => std::fs::remove_file(&path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
        removed.err().map(|error| format!("os.RemoveAll: {error}"))
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// `out` must be writable and the key must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_getenv(out: *mut StringOut, key: *const u8, key_len: i64) {
    // SAFETY: guaranteed by the caller.
    let key = unsafe { text_of(key, key_len) };
    let value = if key.is_empty() || key.contains(['=', '\0']) {
        String::new()
    } else {
        std::env::var_os(&key)
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringOut::built(value.as_bytes())) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_getwd(out: *mut StringError) {
    let result = std::env::current_dir()
        .map(|dir| dir.to_string_lossy().into_owned())
        .map_err(|error| format!("os.Getwd: {error}"));
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringError::from_text(result)) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_args(out: *mut WordArray) {
    let args: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(WordArray::strings(&args)) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_os_exit(code: i64) {
    let _ = std::io::stdout().flush();
    std::process::exit(i32::try_from(code).unwrap_or(if code < 0 { i32::MIN } else { i32::MAX }));
}

/// # Safety
/// `out` must be writable and the path must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_dir_names(
    out: *mut WordArrayError,
    path: *const u8,
    path_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let result = blocking::run(move || -> std::io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let mut name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type()?.is_dir() {
                name.push('/');
            }
            names.push(name);
        }
        names.sort_by(|a, b| a.trim_end_matches('/').cmp(b.trim_end_matches('/')));
        Ok(names)
    });
    let value = match result {
        Ok(names) => WordArrayError::ok(WordArray::strings(&names)),
        Err(error) => WordArrayError::failed(&format!("os.ReadDir: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable and the path must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_stat_fields(
    out: *mut WordArrayError,
    path: *const u8,
    path_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let result = blocking::run(move || std::fs::metadata(&path));
    let value = match result {
        Ok(metadata) => {
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                i64::from(metadata.permissions().mode() & 0o7777)
            };
            #[cfg(not(unix))]
            let mode = if metadata.permissions().readonly() {
                0o444
            } else {
                0o666
            };
            let size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
            WordArrayError::ok(WordArray::ints(&[size, mode, i64::from(metadata.is_dir())]))
        }
        Err(error) => WordArrayError::failed(&format!("os.Stat: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable and the path must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_open(
    out: *mut ValueError,
    path: *const u8,
    path_len: i64,
    create: bool,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let result = blocking::run(move || {
        if create {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
        } else {
            std::fs::File::open(&path)
        }
    });
    let value = match result {
        Ok(file) => ValueError::ok(insert(file)),
        Err(error) => ValueError::failed_text(&format!(
            "{}: {error}",
            if create { "os.Create" } else { "os.Open" }
        )),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_read(out: *mut ByteArrayError, id: i64, max: i64) {
    let handle = lookup(id);
    let result = blocking::run(move || {
        let Some(handle) = handle else {
            return Err("os.Read: not an open file".to_string());
        };
        let Ok(max) = usize::try_from(max) else {
            return Err("os.Read: negative length".to_string());
        };
        if max == 0 {
            return Ok(Vec::new());
        }
        let mut buffer = vec![0u8; max];
        let read = match handle.as_ref() {
            Handle::Stdin => std::io::stdin().read(&mut buffer),
            Handle::File(file) => (&*file).read(&mut buffer),
            Handle::Stdout | Handle::Stderr => {
                return Err("os.Read: not open for reading".to_string());
            }
        };
        match read {
            Ok(0) => Err("EOF".to_string()),
            Ok(count) => {
                buffer.truncate(count);
                Ok(buffer)
            }
            Err(error) => Err(format!("os.Read: {error}")),
        }
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ByteArrayError::from_bytes(result)) };
}

fn write_all(id: i64, data: Vec<u8>) -> ValueError {
    let handle = lookup(id);
    let result = blocking::run(move || {
        let Some(handle) = handle else {
            return Err((0, "os.Write: not an open file".to_string()));
        };
        let mut sent = 0;
        while sent < data.len() {
            let written = match handle.as_ref() {
                Handle::Stdout => {
                    let mut stdout = std::io::stdout().lock();
                    stdout
                        .write(&data[sent..])
                        .and_then(|count| stdout.flush().map(|()| count))
                }
                Handle::Stderr => std::io::stderr().write(&data[sent..]),
                Handle::File(file) => (&*file).write(&data[sent..]),
                Handle::Stdin => {
                    return Err((sent, "os.Write: not open for writing".to_string()));
                }
            };
            match written {
                Ok(0) => return Err((sent, "os.Write: wrote no bytes".to_string())),
                Ok(count) => sent += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err((sent, format!("os.Write: {error}"))),
            }
        }
        Ok(sent)
    });
    match result {
        Ok(sent) => ValueError::ok(sent as i64),
        Err((sent, message)) => ValueError::failed_with(sent as i64, &message),
    }
}

/// # Safety
/// `out` must be writable and `data` must point to `data_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_write(
    out: *mut ValueError,
    id: i64,
    data: *const u8,
    data_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let data = unsafe { super::bytes(data, data_len) }.to_vec();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(write_all(id, data)) };
}

/// # Safety
/// `out` must be writable and the string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_write_string(
    out: *mut ValueError,
    id: i64,
    text: *const u8,
    text_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let data = unsafe { super::bytes(text, text_len) }.to_vec();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(write_all(id, data)) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_close(out: *mut ErrorOut, id: i64) {
    let result = if is_standard(id) {
        ErrorOut::ok()
    } else {
        match remove(id) {
            Some(handle) => {
                blocking::run(move || drop(handle));
                ErrorOut::ok()
            }
            None => ErrorOut::failed("os.Close: not an open file"),
        }
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(result) };
}

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_os_file_drop(id: i64) {
    drop(remove(id));
}
