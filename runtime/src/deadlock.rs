//! Notices when every task is waiting on another task and nothing outside can wake any of them.
//!
//! A task that waits on a channel or on another task is counted as blocked. A task that waits on
//! a timer, a descriptor, or a helper thread is not, because the clock or the system can still
//! wake it. When all live tasks are blocked, none of them can ever run again.

use std::sync::atomic::{AtomicUsize, Ordering};

static BLOCKED: AtomicUsize = AtomicUsize::new(0);

pub(super) fn add_blocked() {
    BLOCKED.fetch_add(1, Ordering::SeqCst);
}

pub(super) fn remove_blocked() {
    BLOCKED.fetch_sub(1, Ordering::SeqCst);
}

/// Ends the process when no live task can make progress; the initial task counts as live.
pub(super) fn check() {
    let live = super::task::running() + 1;
    if BLOCKED.load(Ordering::SeqCst) >= live {
        super::panic::deadlock();
    }
}
