use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
    fn Sleep(milliseconds: u32);
}

const CTRL_C_EVENT: u32 = 0;
const CTRL_BREAK_EVENT: u32 = 1;
const CTRL_CLOSE_EVENT: u32 = 2;
const CTRL_LOGOFF_EVENT: u32 = 5;
const CTRL_SHUTDOWN_EVENT: u32 = 6;
const TERMINATION_GRACE_MILLIS: u32 = 5_000;

static SELECTION: AtomicU32 = AtomicU32::new(0);
static GENERATIONS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

extern "system" fn notify(event: u32) -> i32 {
    notify_with_timeout(event, TERMINATION_GRACE_MILLIS)
}

fn notify_with_timeout(event: u32, milliseconds: u32) -> i32 {
    let index = match event {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => 0,
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => 1,
        _ => return 0,
    };
    if SELECTION.load(Ordering::Relaxed) & (1 << index) == 0 {
        return 0;
    }
    GENERATIONS[index].fetch_add(1, Ordering::Relaxed);
    if index == 1 {
        // Returning from a close handler lets Windows terminate the process.
        unsafe { Sleep(milliseconds) };
    }
    1
}

struct Subscription {
    mask: i64,
    generations: [u64; 2],
}

struct Registry {
    next: i64,
    subscribers: HashMap<i64, Subscription>,
    references: [usize; 2],
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| {
    Mutex::new(Registry {
        next: 1,
        subscribers: HashMap::new(),
        references: [0; 2],
    })
});

pub(super) fn subscribe(mask: i64) -> Result<i64, String> {
    if !(1..=3).contains(&mask) {
        return Err("signal.NotifyContext: empty or invalid selection".into());
    }
    let mut registry = REGISTRY.lock().unwrap_or_else(|error| error.into_inner());
    let next = registry
        .next
        .checked_add(1)
        .ok_or("signal.NotifyContext: subscription identifiers exhausted")?;
    let generations = GENERATIONS
        .each_ref()
        .map(|counter| counter.load(Ordering::Relaxed));
    let previous_selection = SELECTION.load(Ordering::Relaxed);
    SELECTION.store(previous_selection | mask as u32, Ordering::Relaxed);
    if registry.subscribers.is_empty() {
        // SAFETY: the callback has static lifetime and the Windows handler ABI.
        if unsafe { SetConsoleCtrlHandler(Some(notify), 1) } == 0 {
            let error = std::io::Error::last_os_error();
            SELECTION.store(previous_selection, Ordering::Relaxed);
            return Err(format!("signal.NotifyContext: {error}"));
        }
    }
    for (index, references) in registry.references.iter_mut().enumerate() {
        if mask & (1 << index) != 0 {
            *references += 1;
        }
    }
    let id = registry.next;
    registry.next = next;
    registry
        .subscribers
        .insert(id, Subscription { mask, generations });
    Ok(id)
}

pub(super) fn raised(id: i64) -> bool {
    let registry = REGISTRY.lock().unwrap_or_else(|error| error.into_inner());
    registry.subscribers.get(&id).is_some_and(|subscription| {
        GENERATIONS.iter().enumerate().any(|(index, counter)| {
            subscription.mask & (1 << index) != 0
                && counter.load(Ordering::Relaxed) != subscription.generations[index]
        })
    })
}

pub(super) fn unsubscribe(id: i64) {
    let mut registry = REGISTRY.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(subscription) = registry.subscribers.remove(&id) {
        let mut selection = 0;
        for (index, references) in registry.references.iter_mut().enumerate() {
            if subscription.mask & (1 << index) != 0 {
                *references -= 1;
            }
            if *references > 0 {
                selection |= 1 << index;
            }
        }
        SELECTION.store(selection, Ordering::Relaxed);
        if registry.subscribers.is_empty() {
            // SAFETY: this removes only our own statically registered callback.
            let removed = unsafe { SetConsoleCtrlHandler(Some(notify), 0) };
            assert_ne!(removed, 0, "cannot remove console signal handler");
        }
    }
}

pub(super) fn live() -> usize {
    REGISTRY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .subscribers
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn console_termination_callbacks_publish_and_wait_without_holding_registry_locks() {
        assert!(subscribe(0).is_err());
        assert!(subscribe(4).is_err());
        assert_eq!(live(), 0);
        for event in [CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT] {
            let interrupt = subscribe(1).unwrap();
            assert_eq!(notify_with_timeout(event, 0), 0);
            let terminate = subscribe(2).unwrap();
            let overlapping = subscribe(2).unwrap();
            assert!(!raised(terminate));
            let started = Instant::now();
            let handler = std::thread::spawn(move || notify_with_timeout(event, 100));
            while !raised(terminate) {
                assert!(started.elapsed() < Duration::from_secs(5));
                std::thread::yield_now();
            }
            assert!(raised(overlapping));
            assert!(!raised(interrupt));
            unsubscribe(terminate);
            unsubscribe(terminate);
            unsubscribe(overlapping);
            unsubscribe(interrupt);
            assert_eq!(live(), 0);
            assert_eq!(handler.join().unwrap(), 1);
            assert!(started.elapsed() >= Duration::from_millis(100));
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(notify_with_timeout(event, 0), 0);
        }
        assert_eq!(notify_with_timeout(99, 0), 0);
        let id = subscribe(1).unwrap();
        assert!(!raised(id));
        unsubscribe(id);
    }
}
