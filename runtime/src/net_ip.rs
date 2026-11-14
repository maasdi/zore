use std::net::IpAddr;

use crate::sys::{StringError, text_of};

fn parse(text: &str) -> Option<IpAddr> {
    if text.len() > 45 {
        return None;
    }
    text.parse().ok()
}

/// # Safety
/// Output is writable and text addresses len readable UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_ip_text(out: *mut StringError, text: *const u8, len: i64) {
    let text = unsafe { text_of(text, len) };
    let result = match parse(&text) {
        Some(address) => StringError::ok(&address.to_string()),
        None => StringError::failed("net.ParseIP: invalid IP address"),
    };
    unsafe { out.write(result) };
}

/// # Safety
/// Both strings address their indicated readable UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_ip_compare(
    left: *const u8,
    left_len: i64,
    right: *const u8,
    right_len: i64,
) -> i64 {
    let left = parse(&unsafe { text_of(left, left_len) });
    let right = parse(&unsafe { text_of(right, right_len) });
    match left.cmp(&right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
