//! Notices when every task is waiting on another task and nothing outside can wake any of them.
//!
//! A task that waits on a channel or on another task is counted as blocked. A task that waits on
//! a timer, a descriptor, or a helper thread is not, because the clock or the system can still
//! wake it. When all live tasks are blocked, none of them can ever run again.

use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
struct Counts {
    running: usize,
    blocked: usize,
}

// Read both counts under one lock: combining independent atomic snapshots can report a
// deadlock while a task is waking and another finishes on a different worker.
static COUNTS: Mutex<Counts> = Mutex::new(Counts {
    running: 0,
    blocked: 0,
});

fn counts() -> MutexGuard<'static, Counts> {
    COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn started() {
    counts().running += 1;
}

pub(super) fn finished() {
    counts().running -= 1;
}

pub(super) fn running() -> usize {
    counts().running
}

pub(super) fn add_blocked() {
    counts().blocked += 1;
}

pub(super) fn remove_blocked() {
    counts().blocked -= 1;
}

/// Ends the process when no live task can make progress; the initial task counts as live.
pub(super) fn check() {
    let counts = counts();
    let live = counts.running + 1;
    if counts.blocked >= live {
        super::panic::deadlock();
    }
}
