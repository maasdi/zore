use std::sync::{Condvar, Mutex, MutexGuard};

#[derive(Default)]
struct State {
    woken: bool,
    counted: bool,
}

#[derive(Default)]
pub(super) struct Slot {
    state: Mutex<State>,
    ready: Condvar,
    internal: bool,
}

impl Slot {
    pub(super) fn internal() -> Self {
        Self {
            internal: true,
            ..Self::default()
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn wake(&self) {
        let mut state = self.lock();
        state.woken = true;
        if std::mem::take(&mut state.counted) {
            super::deadlock::remove_blocked();
        }
        drop(state);
        self.ready.notify_all();
    }

    pub(super) fn park(&self) {
        let mut state = self.lock();
        if state.woken {
            return;
        }
        if self.internal && !state.counted {
            state.counted = true;
            super::deadlock::add_blocked();
            super::deadlock::check();
        }
        let _blocking = super::scheduler::BlockingGuard::enter();
        while !state.woken {
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier, mpsc};
    use std::time::Duration;

    use super::*;

    #[test]
    fn concurrent_wakes_racing_with_park_are_latched() {
        for internal in [false, true] {
            for _ in 0..100 {
                crate::task::started();
                let slot = Arc::new(if internal {
                    Slot::internal()
                } else {
                    Slot::default()
                });
                let barrier = Arc::new(Barrier::new(5));
                let (done, completed) = mpsc::channel();
                std::thread::scope(|scope| {
                    scope.spawn(|| {
                        barrier.wait();
                        slot.park();
                        slot.park();
                        done.send(()).unwrap();
                    });
                    for _ in 0..4 {
                        scope.spawn(|| {
                            barrier.wait();
                            for _ in 0..10 {
                                slot.wake();
                            }
                        });
                    }
                    completed.recv_timeout(Duration::from_secs(10)).unwrap();
                });
                let state = slot.lock();
                assert!(state.woken);
                assert!(!state.counted);
                crate::task::finished();
            }
        }
    }
}
