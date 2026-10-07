use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::alloc::{zore_alloc, zore_free};
use super::fiber::Slot;

type Destroy = unsafe extern "C" fn(*mut u8);

static LIVE: AtomicUsize = AtomicUsize::new(0);

pub(super) fn live_channels() -> usize {
    LIVE.load(Ordering::SeqCst)
}

/// A queued value, held in storage as aligned as any Zore value needs.
struct Message {
    data: *mut u8,
    size: usize,
}

// SAFETY: a message is owned by whichever queue or task currently holds it.
unsafe impl Send for Message {}

impl Message {
    /// # Safety
    /// `value` must point to `size` readable bytes.
    unsafe fn copy_of(value: *const u8, size: usize) -> Self {
        let data = zore_alloc(size as i64);
        // SAFETY: both ranges hold `size` bytes and do not overlap.
        unsafe { std::ptr::copy_nonoverlapping(value, data, size) };
        Self { data, size }
    }

    /// Moves the bytes out and frees the storage.
    ///
    /// # Safety
    /// `out` must point to `size` writable bytes.
    unsafe fn move_to(self, out: *mut u8) {
        // SAFETY: both ranges hold `size` bytes and do not overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(self.data, out, self.size);
            zore_free(self.data, self.size as i64);
        }
    }

    /// Destroys the value it holds, then frees the storage.
    fn discard(self, destroy: Option<Destroy>) {
        if let Some(destroy) = destroy {
            // SAFETY: the message holds one live value of the type `destroy` handles.
            unsafe { destroy(self.data) };
        }
        // SAFETY: the storage came from `zore_alloc(size)`.
        unsafe { zore_free(self.data, self.size as i64) };
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    Pending,
    Delivered,
    Closed,
}

struct Exchange {
    message: Option<Message>,
    outcome: Outcome,
}

/// A task blocked in a send or a receive.
struct Waiting {
    slot: Arc<Slot>,
    exchange: Mutex<Exchange>,
}

impl Waiting {
    fn new(message: Option<Message>) -> Arc<Self> {
        Arc::new(Self {
            slot: Arc::new(Slot::default()),
            exchange: Mutex::new(Exchange {
                message,
                outcome: Outcome::Pending,
            }),
        })
    }

    fn exchange(&self) -> MutexGuard<'_, Exchange> {
        self.exchange
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn finish(&self, outcome: Outcome, message: Option<Message>) {
        let mut exchange = self.exchange();
        exchange.outcome = outcome;
        if message.is_some() {
            exchange.message = message;
        }
        drop(exchange);
        self.slot.wake();
    }
}

#[derive(Default)]
struct Queues {
    closed: bool,
    buffer: VecDeque<Message>,
    receivers: VecDeque<Arc<Waiting>>,
    senders: VecDeque<Arc<Waiting>>,
}

pub struct Channel {
    size: usize,
    capacity: usize,
    destroy: Option<Destroy>,
    queues: Mutex<Queues>,
}

impl Channel {
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.queues
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
        let queues = self
            .queues
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for message in queues.buffer.drain(..) {
            message.discard(self.destroy);
        }
    }
}

const SEND_CLOSED: &[u8] = b"send on a closed channel";
const CLOSE_CLOSED: &[u8] = b"close of a closed channel";

/// A channel for values of `size` bytes holding up to `capacity` of them; `destroy` drops one.
#[unsafe(no_mangle)]
pub extern "C" fn zore_channel_make(
    size: i64,
    capacity: i64,
    destroy: Option<Destroy>,
) -> *const Channel {
    let Ok(capacity) = usize::try_from(capacity) else {
        super::panic::raise(b"negative channel capacity");
        return std::ptr::null();
    };
    LIVE.fetch_add(1, Ordering::SeqCst);
    Arc::into_raw(Arc::new(Channel {
        size: usize::try_from(size).unwrap_or(0),
        capacity,
        destroy,
        queues: Mutex::new(Queues::default()),
    }))
}

/// Another handle now refers to the channel.
///
/// # Safety
/// `channel` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_channel_retain(channel: *const Channel) {
    if !channel.is_null() {
        // SAFETY: guaranteed by the caller.
        unsafe { Arc::increment_strong_count(channel) };
    }
}

/// A handle is gone; the last one destroys the values still buffered.
///
/// # Safety
/// `channel` must be null or a live handle that is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_channel_release(channel: *const Channel) {
    if !channel.is_null() {
        // SAFETY: guaranteed by the caller.
        unsafe { Arc::decrement_strong_count(channel) };
    }
}

/// Moves the value at `value` into the channel, waiting for room or a receiver. A send on a
/// closed channel destroys the value with `destroy` and raises a panic.
///
/// # Safety
/// `channel` must be null or a live handle; `value` must hold one live value of the channel's type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_channel_send(
    channel: *const Channel,
    value: *mut u8,
    destroy: Option<Destroy>,
) {
    let discard_value = || {
        if let Some(destroy) = destroy {
            // SAFETY: the caller's value was not moved into the channel.
            unsafe { destroy(value) };
        }
        super::panic::raise(SEND_CLOSED);
    };
    // SAFETY: guaranteed by the caller.
    let Some(channel) = (unsafe { channel.as_ref() }) else {
        discard_value();
        return;
    };
    let mut queues = channel.lock();
    if queues.closed {
        drop(queues);
        discard_value();
        return;
    }
    // SAFETY: `value` holds `size` bytes.
    let message = unsafe { Message::copy_of(value, channel.size) };
    if let Some(receiver) = queues.receivers.pop_front() {
        receiver.finish(Outcome::Delivered, Some(message));
        return;
    }
    if queues.buffer.len() < channel.capacity {
        queues.buffer.push_back(message);
        return;
    }
    let waiting = Waiting::new(Some(message));
    queues.senders.push_back(Arc::clone(&waiting));
    drop(queues);
    waiting.slot.park();
    let mut exchange = waiting.exchange();
    if exchange.outcome == Outcome::Closed {
        let message = exchange.message.take();
        drop(exchange);
        if let Some(message) = message {
            message.discard(destroy);
        }
        super::panic::raise(SEND_CLOSED);
    }
}

/// Moves the next value into `out`, waiting if there is none, and says whether one arrived. A
/// closed channel with nothing left, or a zero-value channel, fills `out` with zero bytes.
///
/// # Safety
/// `channel` must be null or a live handle; `out` must point to `size` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_channel_receive(
    channel: *const Channel,
    out: *mut u8,
    size: i64,
) -> bool {
    let size = usize::try_from(size).unwrap_or(0);
    let zero = || {
        // SAFETY: `out` holds `size` bytes.
        unsafe { out.write_bytes(0, size) };
        false
    };
    // SAFETY: guaranteed by the caller.
    let Some(channel) = (unsafe { channel.as_ref() }) else {
        return zero();
    };
    let mut queues = channel.lock();
    if let Some(message) = queues.buffer.pop_front() {
        if let Some(sender) = queues.senders.pop_front() {
            let moved = sender.exchange().message.take();
            if let Some(moved) = moved {
                queues.buffer.push_back(moved);
            }
            sender.finish(Outcome::Delivered, None);
        }
        drop(queues);
        // SAFETY: `out` holds `size` bytes.
        unsafe { message.move_to(out) };
        return true;
    }
    if let Some(sender) = queues.senders.pop_front() {
        drop(queues);
        let message = sender.exchange().message.take();
        sender.finish(Outcome::Delivered, None);
        if let Some(message) = message {
            // SAFETY: `out` holds `size` bytes.
            unsafe { message.move_to(out) };
        }
        return true;
    }
    if queues.closed {
        return zero();
    }
    let waiting = Waiting::new(None);
    queues.receivers.push_back(Arc::clone(&waiting));
    drop(queues);
    waiting.slot.park();
    let message = waiting.exchange().message.take();
    match message {
        Some(message) => {
            // SAFETY: `out` holds `size` bytes.
            unsafe { message.move_to(out) };
            true
        }
        None => zero(),
    }
}

/// Closes the channel, waking everyone blocked on it.
///
/// # Safety
/// `channel` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_channel_close(channel: *const Channel) {
    // SAFETY: guaranteed by the caller.
    let Some(channel) = (unsafe { channel.as_ref() }) else {
        super::panic::raise(CLOSE_CLOSED);
        return;
    };
    let mut queues = channel.lock();
    if queues.closed {
        drop(queues);
        super::panic::raise(CLOSE_CLOSED);
        return;
    }
    queues.closed = true;
    let receivers: Vec<_> = queues.receivers.drain(..).collect();
    let senders: Vec<_> = queues.senders.drain(..).collect();
    drop(queues);
    for receiver in receivers {
        receiver.finish(Outcome::Closed, None);
    }
    for sender in senders {
        sender.finish(Outcome::Closed, None);
    }
}
