// Copyright 2026 The Zore Authors
// SPDX-License-Identifier: Apache-2.0

//! The exported C ABI is internal to the compiler, not a source-language API.

#[path = "alloc.rs"]
mod alloc;
#[path = "blocking.rs"]
mod blocking;
#[path = "channel.rs"]
mod channel;
#[path = "deadlock.rs"]
mod deadlock;
#[path = "fiber.rs"]
mod fiber;
#[path = "io.rs"]
mod io;
#[path = "map.rs"]
mod map;
#[path = "mutex.rs"]
mod mutex;
#[path = "net.rs"]
mod net;
#[path = "panic.rs"]
mod panic;
#[path = "reactor.rs"]
mod reactor;
#[path = "strconv.rs"]
mod strconv;
#[path = "string.rs"]
mod string;
#[path = "strings.rs"]
mod strings;
#[path = "sys.rs"]
mod sys;
#[path = "task.rs"]
mod task;

pub fn finish() {
    let tasks_running = task::running() > 0;
    let leaked = string::live_buffers();
    if leaked > 0 && !tasks_running && std::env::var_os("ZORE_CHECK_LEAKS").is_some() {
        eprintln!("leak: {leaked} text buffers still owned at exit");
        std::process::exit(70);
    }
    let mutexes = mutex::live_mutexes();
    if mutexes > 0 && !tasks_running && std::env::var_os("ZORE_CHECK_LEAKS").is_some() {
        eprintln!("leak: {mutexes} mutexes still alive at exit");
        std::process::exit(70);
    }
    let channels = channel::live_channels();
    if channels > 0 && !tasks_running && std::env::var_os("ZORE_CHECK_LEAKS").is_some() {
        eprintln!("leak: {channels} channels still alive at exit");
        std::process::exit(70);
    }
    panic::finish();
    if !tasks_running {
        string::release_all();
    }
}

/// Borrows compiler-produced string storage for one runtime call.
///
/// # Safety
/// For a nonempty value, `data` must point to `len` initialized, immutable bytes
/// in one live allocation. The storage must outlive the returned borrow.
unsafe fn bytes<'a>(data: *const u8, len: i64) -> &'a [u8] {
    let len = usize::try_from(len)
        .ok()
        .filter(|&len| len <= isize::MAX as usize)
        .unwrap_or_else(|| panic::fail(b"invalid runtime string length"));
    // Empty Zore strings may use a null pointer; Rust empty slices may not.
    if len == 0 {
        return &[];
    }
    if data.is_null() {
        panic::fail(b"invalid runtime string pointer");
    }
    // SAFETY: the caller supplies live string storage for this borrow.
    unsafe { std::slice::from_raw_parts(data, len) }
}
