//! The code behind `std/os`. File handles are numbers that are never reused, so a stale handle
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

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_os_path_separator(value: u8) -> bool {
    std::path::is_separator(char::from(value))
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
    open_with(&mut options, path, perm)
}

fn open_with(
    options: &mut std::fs::OpenOptions,
    path: &str,
    perm: i64,
) -> std::io::Result<std::fs::File> {
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
pub unsafe extern "C" fn zore_native_os_lookup_env(
    out: *mut WordArray,
    key: *const u8,
    key_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let key = unsafe { text_of(key, key_len) };
    let values: Vec<String> = if key.is_empty() || key.contains(['=', '\0']) {
        Vec::new()
    } else {
        std::env::var_os(key)
            .map(|value| value.to_string_lossy().into_owned())
            .into_iter()
            .collect()
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(WordArray::strings(&values)) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_environ(out: *mut WordArray) {
    let mut entries: Vec<String> = std::env::vars_os()
        .map(|(key, value)| format!("{}={}", key.to_string_lossy(), value.to_string_lossy()))
        .collect();
    entries.sort();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(WordArray::strings(&entries)) };
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
            let kind = entry.file_type()?;
            let tag = if kind.is_symlink() {
                'l'
            } else if kind.is_dir() {
                'd'
            } else {
                'f'
            };
            names.push(format!("{tag}{}", entry.file_name().to_string_lossy()));
        }
        names.sort_by(|a, b| a[1..].cmp(&b[1..]));
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
    follow: bool,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let result = blocking::run(move || {
        if follow {
            std::fs::metadata(&path)
        } else {
            std::fs::symlink_metadata(&path)
        }
    });
    let value = match result {
        Ok(metadata) => WordArrayError::ok(WordArray::ints(&metadata_fields(&metadata))),
        Err(error) => WordArrayError::failed(&format!(
            "{}: {error}",
            if follow { "os.Stat" } else { "os.Lstat" }
        )),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

fn metadata_fields(metadata: &std::fs::Metadata) -> [i64; 6] {
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
    [
        i64::try_from(metadata.len()).unwrap_or(i64::MAX),
        mode,
        i64::from(metadata.is_dir()),
        metadata.modified().map_or(0, unix_seconds),
        i64::from(metadata.file_type().is_symlink()),
        i64::from(metadata.is_file()),
    ]
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_stat(out: *mut WordArrayError, id: i64) {
    let handle = lookup(id);
    let result = blocking::run(move || match handle.as_deref() {
        Some(Handle::File(file)) => file.metadata().map_err(|error| error.to_string()),
        Some(_) => Err("not a file on disk".to_string()),
        None => Err("not an open file".to_string()),
    });
    let value = match result {
        Ok(metadata) => WordArrayError::ok(WordArray::ints(&metadata_fields(&metadata))),
        Err(error) => WordArrayError::failed(&format!("os.File.Stat: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

fn unix_seconds(time: std::time::SystemTime) -> i64 {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Err(error) => {
            let duration = error.duration();
            -i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
                - i64::from(duration.subsec_nanos() != 0)
        }
    }
}

const WRITE_ONLY: i64 = 1;
const APPEND: i64 = 4;
const CREATE: i64 = 8;
const EXCLUSIVE: i64 = 16;
const TRUNCATE: i64 = 32;
const ALREADY_EXISTS: i64 = -1;

fn options_for(flags: i64) -> std::fs::OpenOptions {
    let access = flags & 3;
    let mut options = std::fs::OpenOptions::new();
    options
        .read(access != WRITE_ONLY)
        .write(access != 0)
        .append(flags & APPEND != 0)
        .truncate(flags & TRUNCATE != 0);
    if flags & EXCLUSIVE != 0 {
        options.create_new(true);
    } else {
        options.create(flags & CREATE != 0);
    }
    options
}

/// # Safety
/// `out` must be writable; the path and `op` must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_open_with(
    out: *mut ValueError,
    path: *const u8,
    path_len: i64,
    flags: i64,
    perm: i64,
    op: *const u8,
    op_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (path, op) = unsafe { (text_of(path, path_len), text_of(op, op_len)) };
    let result = blocking::run(move || open_with(&mut options_for(flags), &path, perm));
    let value = match result {
        Ok(file) => ValueError::ok(insert(file)),
        Err(error) => {
            let code = if error.kind() == std::io::ErrorKind::AlreadyExists {
                ALREADY_EXISTS
            } else {
                0
            };
            ValueError::failed_with(code, &format!("{op}: {error}"))
        }
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_seek(
    out: *mut ValueError,
    id: i64,
    offset: i64,
    whence: i64,
) {
    let handle = lookup(id);
    let result = blocking::run(move || {
        use std::io::{Seek, SeekFrom};
        let Some(handle) = handle else {
            return Err("os.Seek: not an open file".to_string());
        };
        let Handle::File(file) = handle.as_ref() else {
            return Err("os.Seek: not a seekable file".to_string());
        };
        let target = match whence {
            0 => match u64::try_from(offset) {
                Ok(position) => SeekFrom::Start(position),
                Err(_) => return Err("os.Seek: negative position".to_string()),
            },
            1 => SeekFrom::Current(offset),
            _ => SeekFrom::End(offset),
        };
        (&*file)
            .seek(target)
            .map(|position| i64::try_from(position).unwrap_or(i64::MAX))
            .map_err(|error| format!("os.Seek: {error}"))
    });
    let value = match result {
        Ok(position) => ValueError::ok(position),
        Err(message) => ValueError::failed_text(&message),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_sync(out: *mut ErrorOut, id: i64) {
    let handle = lookup(id);
    let failure = blocking::run(move || match handle.as_deref() {
        None => Some("os.Sync: not an open file".to_string()),
        Some(Handle::File(file)) => file
            .sync_all()
            .err()
            .map(|error| format!("os.Sync: {error}")),
        Some(Handle::Stdout) => std::io::stdout()
            .flush()
            .err()
            .map(|error| format!("os.Sync: {error}")),
        Some(Handle::Stderr | Handle::Stdin) => None,
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_temp_dir(out: *mut StringOut) {
    let dir = temp_dir();
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringOut::built(dir.as_bytes())) };
}

fn temp_dir() -> String {
    let dir = std::env::temp_dir().to_string_lossy().into_owned();
    if dir.is_empty() && cfg!(unix) {
        "/tmp".to_string()
    } else {
        dir
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn random_word() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(TEMP_COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u32(std::process::id());
    if let Ok(elapsed) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        hasher.write_u128(elapsed.as_nanos());
    }
    hasher.finish()
}

fn random_text() -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    let mut word = random_word();
    let mut text = String::with_capacity(12);
    for _ in 0..12 {
        text.push(char::from(ALPHABET[(word & 31) as usize]));
        word >>= 5;
    }
    text
}

/// # Safety
/// `out` must be writable; `dir`, `prefix`, and `suffix` must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_temp_path(
    out: *mut StringOut,
    dir: *const u8,
    dir_len: i64,
    prefix: *const u8,
    prefix_len: i64,
    suffix: *const u8,
    suffix_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (dir, prefix, suffix) = unsafe {
        (
            text_of(dir, dir_len),
            text_of(prefix, prefix_len),
            text_of(suffix, suffix_len),
        )
    };
    let mut path = if dir.is_empty() { temp_dir() } else { dir };
    if !path.is_empty() && !path.ends_with(std::path::is_separator) {
        path.push(std::path::MAIN_SEPARATOR);
    }
    path.push_str(&format!("{prefix}{}{suffix}", random_text()));
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(StringOut::built(path.as_bytes())) };
}

/// # Safety
/// `out` must be writable and the path must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_mkdir_new(
    out: *mut ValueError,
    path: *const u8,
    path_len: i64,
    perm: i64,
) {
    // SAFETY: guaranteed by the caller.
    let path = unsafe { text_of(path, path_len) };
    let result = blocking::run(move || {
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(u32::try_from(perm & 0o7777).unwrap_or(0o700));
            builder
        };
        #[cfg(not(unix))]
        let _ = perm;
        builder.create(&path)
    });
    let value = match result {
        Ok(()) => ValueError::ok(0),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ValueError::failed_with(1, &format!("os.MkdirTemp: {error}"))
        }
        Err(error) => ValueError::failed_text(&format!("os.MkdirTemp: {error}")),
    };
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(value) };
}

/// # Safety
/// `out` must be writable and both paths must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_rename(
    out: *mut ErrorOut,
    old_path: *const u8,
    old_len: i64,
    new_path: *const u8,
    new_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (old_path, new_path) = unsafe { (text_of(old_path, old_len), text_of(new_path, new_len)) };
    let failure = blocking::run(move || {
        std::fs::rename(&old_path, &new_path)
            .err()
            .map(|error| format!("os.Rename: {error}"))
    });
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(ErrorOut::from_failure(failure)) };
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

#[cfg(unix)]
fn close_reporting(file: std::fs::File) -> std::io::Result<()> {
    use std::os::fd::IntoRawFd;
    unsafe extern "C" {
        fn close(fd: i32) -> i32;
    }
    let fd = file.into_raw_fd();
    // SAFETY: the descriptor was released by its `File` and is closed exactly once; never retried.
    if unsafe { close(fd) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn close_reporting(file: std::fs::File) -> std::io::Result<()> {
    use std::os::windows::io::IntoRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    let handle = file.into_raw_handle();
    // SAFETY: the handle was released by its `File` and is closed exactly once.
    if unsafe { CloseHandle(handle) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(unix, windows)))]
fn close_reporting(file: std::fs::File) -> std::io::Result<()> {
    drop(file);
    Ok(())
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_os_file_close(out: *mut ErrorOut, id: i64) {
    let result = if is_standard(id) {
        ErrorOut::ok()
    } else {
        match remove(id) {
            Some(handle) => ErrorOut::from_failure(blocking::run(move || {
                let Ok(Handle::File(file)) = Arc::try_unwrap(handle) else {
                    return None;
                };
                close_reporting(file)
                    .err()
                    .map(|error| format!("os.Close: {error}"))
            })),
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

#[cfg(test)]
mod tests {
    use super::close_reporting;

    #[test]
    fn closing_a_file_reports_success() {
        let path = std::env::temp_dir().join(format!("zore-close-{}", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        assert!(close_reporting(file).is_ok());
        std::fs::remove_file(&path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn closing_reports_the_system_error() {
        use std::os::fd::FromRawFd;
        // SAFETY: a descriptor this large is never open, so `close` fails and touches nothing.
        let never_open = unsafe { std::fs::File::from_raw_fd(1 << 30) };
        let error = close_reporting(never_open).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(9));
    }
}
