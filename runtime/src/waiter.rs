use std::sync::Arc;
use std::task::Waker;

use super::slot::Slot;

#[derive(Clone)]
pub(super) enum Waiter {
    Slot(Arc<Slot>),
    Task(Waker),
}

impl Waiter {
    pub(super) fn wake(&self) {
        match self {
            Self::Slot(slot) => slot.wake(),
            Self::Task(waker) => waker.wake_by_ref(),
        }
    }
}

impl From<Arc<Slot>> for Waiter {
    fn from(slot: Arc<Slot>) -> Self {
        Self::Slot(slot)
    }
}

impl From<Waker> for Waiter {
    fn from(waker: Waker) -> Self {
        Self::Task(waker)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::scheduler::{Poll, TestPool};

    #[test]
    fn slot_wake_before_park_is_not_lost() {
        let slot = Arc::new(Slot::default());
        let waiter = Waiter::from(Arc::clone(&slot));
        waiter.wake();
        slot.park();
    }

    #[test]
    fn cloned_waiters_wake_a_blocked_worker_or_a_poll_task() {
        let pool = TestPool::new(1);
        let (registered, rx) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let mut first = true;
        pool.spawn(move |context| {
            if std::mem::take(&mut first) {
                registered
                    .send(Waiter::from(context.waker().clone()))
                    .unwrap();
                context.pending_internal()
            } else {
                done.send(()).unwrap();
                Poll::Ready
            }
        });
        let poll_waiter = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let slot = Arc::new(Slot::default());
        let slot_waiter = Waiter::from(Arc::clone(&slot));
        let (done, finished_worker) = mpsc::channel();
        pool.spawn(move |_| {
            slot.park();
            done.send(()).unwrap();
            Poll::Ready
        });
        for waiter in [slot_waiter, poll_waiter] {
            waiter.clone().wake();
        }
        finished.recv_timeout(Duration::from_secs(10)).unwrap();
        finished_worker
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        pool.idle();
    }
}
