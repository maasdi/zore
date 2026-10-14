use super::sys::{StringError, text_of};

/// # Safety
/// `out` is writable and the path addresses `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_filepath_absolute(
    out: *mut StringError,
    data: *const u8,
    len: i64,
) {
    let path = unsafe { text_of(data, len) };
    let normalized = path.replace('\\', "/");
    let result = if normalized.starts_with("//?/") || normalized.starts_with("//./") {
        Err("filepath: Windows device and verbatim paths are unsupported".into())
    } else {
        std::path::absolute(if path.is_empty() { "." } else { &path })
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|error| format!("filepath.Abs: {error}"))
    };
    unsafe { out.write(StringError::from_text(result)) };
}
