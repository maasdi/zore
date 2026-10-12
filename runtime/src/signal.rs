use super::strconv::ValueError;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Clone, Copy)]
    #[repr(C)]
    struct Action {
        handler: usize,
        #[cfg(target_os = "linux")]
        mask: [usize; 128 / std::mem::size_of::<usize>()],
        #[cfg(target_os = "macos")]
        mask: u32,
        flags: i32,
        #[cfg(target_os = "linux")]
        restorer: usize,
    }
    impl Action {
        fn new(handler: usize) -> Self {
            Self {
                handler,
                #[cfg(target_os = "linux")]
                mask: [0; 128 / std::mem::size_of::<usize>()],
                #[cfg(target_os = "macos")]
                mask: 0,
                #[cfg(target_os = "linux")]
                flags: 0x10000000,
                #[cfg(target_os = "macos")]
                flags: 2,
                #[cfg(target_os = "linux")]
                restorer: 0,
            }
        }
    }
    unsafe extern "C" {
        fn sigaction(signal: i32, action: *const Action, previous: *mut Action) -> i32;
    }
    const SIGNALS: [i32; 2] = [2, 15];
    static GENERATIONS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
    extern "C" fn notify(signal: i32) {
        match signal {
            2 => {
                GENERATIONS[0].fetch_add(1, Ordering::Relaxed);
            }
            15 => {
                GENERATIONS[1].fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
    struct Subscription {
        mask: i64,
        generations: [u64; 2],
    }
    struct Registry {
        next: i64,
        subscribers: HashMap<i64, Subscription>,
        references: [usize; 2],
        previous: [Option<Action>; 2],
    }
    static REGISTRY: std::sync::LazyLock<Mutex<Registry>> = std::sync::LazyLock::new(|| {
        Mutex::new(Registry {
            next: 1,
            subscribers: HashMap::new(),
            references: [0; 2],
            previous: [None; 2],
        })
    });
    impl Registry {
        fn release(&mut self, mask: i64) {
            for (index, &signal) in SIGNALS.iter().enumerate() {
                if mask & (1 << index) == 0 {
                    continue;
                }
                self.references[index] -= 1;
                if self.references[index] == 0 {
                    let action = self.previous[index].take().expect("installed disposition");
                    // SAFETY: the saved disposition and supported signal remain valid.
                    let restored = unsafe { sigaction(signal, &action, std::ptr::null_mut()) };
                    assert_eq!(restored, 0, "cannot restore signal disposition");
                }
            }
        }
    }
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
        let mut installed = 0;
        for (index, &signal) in SIGNALS.iter().enumerate() {
            if mask & (1 << index) == 0 {
                continue;
            }
            if registry.references[index] == 0 {
                let action = Action::new(notify as *const () as usize);
                let mut previous = Action::new(0);
                // SAFETY: both records have the platform sigaction layout and live storage.
                if unsafe { sigaction(signal, &action, &mut previous) } != 0 {
                    let error = std::io::Error::last_os_error();
                    registry.release(installed);
                    return Err(format!("signal.NotifyContext: {error}"));
                }
                registry.previous[index] = Some(previous);
            }
            registry.references[index] += 1;
            installed |= 1 << index;
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
            registry.release(subscription.mask);
        }
    }
    pub(super) fn live() -> usize {
        REGISTRY
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .subscribers
            .len()
    }
}

/// # Safety
/// The caller provides writable result storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_signal_subscribe(out: *mut ValueError, mask: i64) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let result = match platform::subscribe(mask) {
        Ok(id) => ValueError::ok(id),
        Err(error) => ValueError::failed_text(&error),
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let result = {
        let _ = mask;
        ValueError::failed_text("signal.NotifyContext: unsupported host")
    };
    // SAFETY: the caller supplies writable result storage.
    unsafe { out.write(result) };
}
#[unsafe(no_mangle)]
pub extern "C" fn zore_native_signal_raised(id: i64) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        platform::raised(id)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = id;
        false
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn zore_native_signal_unsubscribe(id: i64) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    platform::unsubscribe(id);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = id;
}
pub(super) fn live_subscriptions() -> usize {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        platform::live()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}
