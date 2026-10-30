use std::sync::Arc;

use super::channel::{Channel, ReceiveWait};
use super::reactor::{self, Timer};
use super::scheduler::Context;
use super::slot::Slot;
use super::waiter::Waiter;

pub struct DeadlineWait {
    receive: ReceiveWait,
    timer: Option<Timer>,
}

impl DeadlineWait {
    /// # Safety
    /// The handle must be null or a live bool channel.
    unsafe fn start(channel: *const Channel, deadline: i64, waiter: Waiter) -> Self {
        let receive = unsafe { ReceiveWait::start(channel, waiter.clone()) };
        let remaining = deadline.saturating_sub(super::sys::zore_native_time_now());
        let timer = if receive.finish_if(false).is_none() && remaining > 0 {
            Some(reactor::start_timer(
                (remaining as u64).div_ceil(1_000_000),
                waiter,
            ))
        } else {
            None
        };
        Self { receive, timer }
    }

    fn result(&self) -> Option<bool> {
        self.receive
            .finish_if(self.timer.as_ref().is_none_or(Timer::ready))
    }
}

/// # Safety
/// The handle must be null or a live bool channel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_context_wait_until(
    channel: *const Channel,
    deadline: i64,
) -> bool {
    let slot = Arc::new(Slot::default());
    let operation = unsafe { DeadlineWait::start(channel, deadline, Arc::clone(&slot).into()) };
    if let Some(expired) = operation.result() {
        return expired;
    }
    slot.park();
    operation
        .result()
        .expect("deadline or channel completion wakes the slot")
}

/// # Safety
/// The channel must be null or live; context must be current.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_context_wait_until_start(
    channel: *const Channel,
    deadline: i64,
    context: *mut Context,
) -> *mut DeadlineWait {
    let waiter = unsafe { &*context }.waker().clone().into();
    Box::into_raw(Box::new(unsafe {
        DeadlineWait::start(channel, deadline, waiter)
    }))
}

/// # Safety
/// Poll one owned operation serially with current context and a writable byte; Ready consumes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_context_wait_until_poll(
    operation: *mut DeadlineWait,
    _context: *mut Context,
    out: *mut u8,
) -> u8 {
    let Some(expired) = (unsafe { &*operation }).result() else {
        return 0;
    };
    let operation = unsafe { Box::from_raw(operation) };
    drop(operation);
    unsafe { out.write(u8::from(expired)) };
    1
}

/// # Safety
/// The handle must be null or a live bool channel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_time_wait_until(
    channel: *const Channel,
    deadline: i64,
) -> bool {
    unsafe { zore_native_context_wait_until(channel, deadline) }
}

/// # Safety
/// The channel must be null or live; context must be current.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_time_wait_until_start(
    channel: *const Channel,
    deadline: i64,
    context: *mut Context,
) -> *mut DeadlineWait {
    unsafe { zore_native_context_wait_until_start(channel, deadline, context) }
}

/// # Safety
/// Poll one owned operation serially with current context and a writable byte; Ready consumes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_time_wait_until_poll(
    operation: *mut DeadlineWait,
    context: *mut Context,
    out: *mut u8,
) -> u8 {
    unsafe { zore_native_context_wait_until_poll(operation, context, out) }
}

fn deadline_after(remaining: i64) -> i64 {
    super::sys::zore_native_time_now().saturating_add(remaining)
}

/// # Safety
/// The handle must be null or a live bool channel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_context_wait_for(
    channel: *const Channel,
    remaining: i64,
) -> bool {
    unsafe { zore_native_context_wait_until(channel, deadline_after(remaining)) }
}

/// # Safety
/// The channel must be null or live; context must be current.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_context_wait_for_start(
    channel: *const Channel,
    remaining: i64,
    context: *mut Context,
) -> *mut DeadlineWait {
    unsafe { zore_native_context_wait_until_start(channel, deadline_after(remaining), context) }
}

/// # Safety
/// Poll one owned operation serially with current context and a writable byte; Ready consumes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_context_wait_for_poll(
    operation: *mut DeadlineWait,
    context: *mut Context,
    out: *mut u8,
) -> u8 {
    unsafe { zore_native_context_wait_until_poll(operation, context, out) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{zore_channel_close, zore_channel_make};
    use crate::scheduler::{Poll, TestPool};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn plain_deadline_wait_handles_closed_channels_and_elapsed_deadlines() {
        for closed in [false, true] {
            let raw = zore_channel_make(1, 0, None);
            // SAFETY: the initial owned reference becomes this Arc.
            let channel = unsafe { Arc::from_raw(raw) };
            if closed {
                unsafe { zore_channel_close(raw) };
            }
            let deadline = if closed {
                crate::sys::zore_native_time_now() + 3_600_000_000_000
            } else {
                0
            };
            let result = unsafe { zore_native_context_wait_until(raw, deadline) };
            assert_eq!(result, !closed);
            assert_eq!(Arc::strong_count(&channel), 1);
        }
    }

    #[test]
    fn pending_deadline_poll_wakes_on_cancel_and_consumes_every_owned_reference() {
        let pool = TestPool::new(1);
        let raw = zore_channel_make(1, 0, None);
        // SAFETY: the initial owned reference becomes this Arc.
        let channel = unsafe { Arc::from_raw(raw) };
        let task_channel = Arc::clone(&channel);
        let (registered, registration) = mpsc::channel();
        let (done, completion) = mpsc::channel();
        let mut operation = 0usize;
        pool.spawn(move |context| {
            let first = operation == 0;
            if first {
                operation = unsafe {
                    zore_native_context_wait_until_start(
                        Arc::as_ptr(&task_channel),
                        crate::sys::zore_native_time_now() + 3_600_000_000_000,
                        context,
                    )
                } as usize;
            }
            let mut expired = 255u8;
            let ready = unsafe {
                zore_native_context_wait_until_poll(
                    operation as *mut DeadlineWait,
                    context,
                    &mut expired,
                )
            };
            if first {
                assert_eq!(ready, 0);
                assert_eq!(expired, 255);
                registered.send(()).unwrap();
                Poll::Pending
            } else {
                assert_eq!(ready, 1);
                assert_eq!(expired, 0);
                done.send(()).unwrap();
                Poll::Ready
            }
        });
        registration.recv_timeout(Duration::from_secs(10)).unwrap();
        unsafe { zore_channel_close(raw) };
        completion.recv_timeout(Duration::from_secs(10)).unwrap();
        pool.idle();
        assert_eq!(Arc::strong_count(&channel), 1);
    }
}
