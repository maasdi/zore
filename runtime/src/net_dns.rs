use std::collections::HashSet;
use std::net::ToSocketAddrs;

use crate::sys::{StringError, text_of};

fn lookup(family: i64, host: &str) -> Result<String, String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for endpoint in (host, 0)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
    {
        let address = endpoint.ip();
        if ((family == 4 && !address.is_ipv4()) || (family == 6 && !address.is_ipv6()))
            || !seen.insert(address)
        {
            continue;
        }
        output.push(address.to_string());
    }
    if output.is_empty() {
        return Err("no suitable address found".into());
    }
    Ok(output.join("\n"))
}

/// # Safety
/// Output is writable and host addresses host_len readable UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_lookup_text(
    out: *mut StringError,
    family: i64,
    host: *const u8,
    host_len: i64,
) {
    let host = unsafe { text_of(host, host_len) };
    let result = crate::blocking::run(move || {
        #[cfg(any(test, zore_test_hooks))]
        let held = test_enter();
        let result = lookup(family, &host);
        #[cfg(any(test, zore_test_hooks))]
        test_complete(held);
        result
    });
    let result = match result {
        Ok(addresses) => StringError::ok(&addresses),
        Err(error) => StringError::failed(&format!("net.LookupIP: {error}")),
    };
    unsafe { out.write(result) };
}

#[cfg(any(test, zore_test_hooks))]
#[derive(Default)]
struct LookupGate {
    held: bool,
    entered: bool,
    completed: bool,
}

#[cfg(any(test, zore_test_hooks))]
static GATE: (std::sync::Mutex<LookupGate>, std::sync::Condvar) = (
    std::sync::Mutex::new(LookupGate {
        held: false,
        entered: false,
        completed: false,
    }),
    std::sync::Condvar::new(),
);

#[cfg(any(test, zore_test_hooks))]
pub(super) fn test_gate(phase: i64) {
    let (mutex, changed) = &GATE;
    let mut state = mutex.lock().unwrap();
    match phase {
        0 => {
            *state = LookupGate {
                held: true,
                entered: false,
                completed: false,
            }
        }
        1 => {
            while !state.entered {
                state = changed.wait(state).unwrap();
            }
        }
        2 => {
            state.held = false;
            changed.notify_all();
            while !state.completed {
                state = changed.wait(state).unwrap();
            }
        }
        _ => panic!("invalid lookup gate phase"),
    }
}

#[cfg(any(test, zore_test_hooks))]
fn test_enter() -> bool {
    let (mutex, changed) = &GATE;
    let mut state = mutex.lock().unwrap();
    let held = state.held;
    if held {
        state.entered = true;
        changed.notify_all();
        while state.held {
            state = changed.wait(state).unwrap();
        }
    }
    held
}

#[cfg(any(test, zore_test_hooks))]
fn test_complete(held: bool) {
    if held {
        let (mutex, changed) = &GATE;
        mutex.lock().unwrap().completed = true;
        changed.notify_all();
    }
}
