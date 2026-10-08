use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::alloc::{zore_alloc, zore_free};
use super::fiber::Slot;
use super::scheduler::Context;
use super::waiter::Waiter;

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

    /// Frees the storage without touching the value, which another owner still holds.
    fn free(self) {
        // SAFETY: the storage came from `zore_alloc(size)`.
        unsafe { zore_free(self.data, self.size as i64) };
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
    /// Which case of a `select` completed.
    case: usize,
}

struct Waiting {
    waiter: Waiter,
    claimed: AtomicBool,
    exchange: Mutex<Exchange>,
}

impl Waiting {
    fn new(waiter: Waiter) -> Arc<Self> {
        Arc::new(Self {
            waiter,
            claimed: AtomicBool::new(false),
            exchange: Mutex::new(Exchange {
                message: None,
                outcome: Outcome::Pending,
                case: 0,
            }),
        })
    }

    fn exchange(&self) -> MutexGuard<'_, Exchange> {
        self.exchange
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Only the first caller may complete a waiting task, whichever of its channels acts first.
    fn claim(&self) -> bool {
        !self.claimed.swap(true, Ordering::SeqCst)
    }

    fn complete(&self, case: usize, outcome: Outcome, message: Option<Message>) {
        let mut exchange = self.exchange();
        exchange.outcome = outcome;
        exchange.case = case;
        if message.is_some() {
            exchange.message = message;
        }
        drop(exchange);
        self.waiter.wake();
    }
}

/// One place in a channel's queue where a task waits; a sender's entry carries its value.
struct Entry {
    waiting: Arc<Waiting>,
    case: usize,
    message: Option<Message>,
}

#[derive(Default)]
struct Queues {
    closed: bool,
    buffer: VecDeque<Message>,
    receivers: VecDeque<Entry>,
    senders: VecDeque<Entry>,
}

/// The next entry whose task nobody else has completed.
fn pop_live(queue: &mut VecDeque<Entry>) -> Option<Entry> {
    while let Some(entry) = queue.pop_front() {
        if entry.waiting.claim() {
            return Some(entry);
        }
        if let Some(message) = entry.message {
            message.free();
        }
    }
    None
}

enum SendTry {
    Done,
    Closed,
    NotReady,
}

enum ReceiveTry {
    Got(Message),
    Closed,
    Empty,
}

fn try_send(queues: &mut Queues, capacity: usize, make: &dyn Fn() -> Message) -> SendTry {
    if queues.closed {
        return SendTry::Closed;
    }
    if let Some(receiver) = pop_live(&mut queues.receivers) {
        receiver
            .waiting
            .complete(receiver.case, Outcome::Delivered, Some(make()));
        return SendTry::Done;
    }
    if queues.buffer.len() < capacity {
        queues.buffer.push_back(make());
        return SendTry::Done;
    }
    SendTry::NotReady
}

fn try_receive(queues: &mut Queues) -> ReceiveTry {
    if let Some(message) = queues.buffer.pop_front() {
        if let Some(mut sender) = pop_live(&mut queues.senders) {
            if let Some(moved) = sender.message.take() {
                queues.buffer.push_back(moved);
            }
            sender
                .waiting
                .complete(sender.case, Outcome::Delivered, None);
        }
        return ReceiveTry::Got(message);
    }
    if let Some(mut sender) = pop_live(&mut queues.senders) {
        let message = sender.message.take();
        sender
            .waiting
            .complete(sender.case, Outcome::Delivered, None);
        if let Some(message) = message {
            return ReceiveTry::Got(message);
        }
    }
    if queues.closed {
        ReceiveTry::Closed
    } else {
        ReceiveTry::Empty
    }
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

/// Destroys a value that could not be sent, then raises the panic for sending on a closed channel.
///
/// # Safety
/// `value` must hold one live value that `destroy` can drop.
unsafe fn fail_send(value: *mut u8, destroy: Option<Destroy>) {
    if let Some(destroy) = destroy {
        // SAFETY: guaranteed by the caller.
        unsafe { destroy(value) };
    }
    super::panic::raise(SEND_CLOSED);
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
    // SAFETY: guaranteed by the caller.
    let Some(channel) = (unsafe { channel.as_ref() }) else {
        // SAFETY: guaranteed by the caller.
        unsafe { fail_send(value, destroy) };
        return;
    };
    // SAFETY: `value` holds `size` bytes.
    let make = || unsafe { Message::copy_of(value, channel.size) };
    let mut queues = channel.lock();
    match try_send(&mut queues, channel.capacity, &make) {
        SendTry::Done => return,
        SendTry::Closed => {
            drop(queues);
            // SAFETY: guaranteed by the caller.
            unsafe { fail_send(value, destroy) };
            return;
        }
        SendTry::NotReady => {}
    }
    let slot = Arc::new(Slot::internal());
    let waiting = Waiting::new(Arc::clone(&slot).into());
    queues.senders.push_back(Entry {
        waiting: Arc::clone(&waiting),
        case: 0,
        message: Some(make()),
    });
    drop(queues);
    slot.park();
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
    match try_receive(&mut queues) {
        ReceiveTry::Got(message) => {
            drop(queues);
            // SAFETY: `out` holds `size` bytes.
            unsafe { message.move_to(out) };
            return true;
        }
        ReceiveTry::Closed => return zero(),
        ReceiveTry::Empty => {}
    }
    let slot = Arc::new(Slot::internal());
    let waiting = Waiting::new(Arc::clone(&slot).into());
    queues.receivers.push_back(Entry {
        waiting: Arc::clone(&waiting),
        case: 0,
        message: None,
    });
    drop(queues);
    slot.park();
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
    for entry in receivers.into_iter().chain(senders) {
        if entry.waiting.claim() {
            entry
                .waiting
                .complete(entry.case, Outcome::Closed, entry.message);
        } else if let Some(message) = entry.message {
            message.free();
        }
    }
}

/// One case of a `select`, written by the compiler and completed by `zore_select`.
#[repr(C)]
pub struct SelectCase {
    channel: *const Channel,
    send: u8,
    /// The value to send, or where a received value goes.
    value: *mut u8,
    size: i64,
    destroy: Option<Destroy>,
    /// Set for a receive: whether a value arrived.
    received: u8,
}

static ROTATE: AtomicUsize = AtomicUsize::new(0);

/// Finishes a case that could proceed: reports the result of a receive or fails a send.
///
/// # Safety
/// The case's pointers must be valid, as `zore_select` requires.
unsafe fn settle_receive(case: &mut SelectCase, message: Option<Message>) {
    case.received = u8::from(message.is_some());
    match message {
        // SAFETY: `value` holds `size` bytes.
        Some(message) => unsafe { message.move_to(case.value) },
        // SAFETY: `value` holds `size` bytes.
        None => unsafe {
            case.value
                .write_bytes(0, usize::try_from(case.size).unwrap_or(0))
        },
    }
}

/// Performs one case that can proceed and returns its index, or waits for one; with a default and
/// nothing ready it returns -1. A send case that is not chosen keeps its value with the caller.
///
/// # Safety
/// `cases` must point to `count` cases whose channels are null or live handles, whose send values
/// are live, and whose receive buffers hold `size` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_select(cases: *mut SelectCase, count: i64, has_default: bool) -> i64 {
    let count = usize::try_from(count).unwrap_or(0);
    // SAFETY: guaranteed by the caller.
    let cases = unsafe { std::slice::from_raw_parts_mut(cases, count) };
    let slot = Arc::new(Slot::internal());
    match unsafe { start_select(cases, has_default, Arc::clone(&slot).into()) } {
        Ok(index) => index,
        Err(waiting) => {
            slot.park();
            // SAFETY: cases and their storage stay live throughout the blocking call.
            unsafe { finish_select(cases, &waiting) }
        }
    }
}

/// # Safety
/// Case pointers and channel handles must remain valid through completion.
unsafe fn start_select(
    cases: &mut [SelectCase],
    has_default: bool,
    waiter: Waiter,
) -> Result<i64, Arc<Waiting>> {
    let count = cases.len();
    let mut channels: Vec<&Channel> = cases
        // SAFETY: guaranteed by the caller.
        .iter()
        .filter_map(|case| unsafe { case.channel.as_ref() })
        .collect();
    channels.sort_by_key(|channel| *channel as *const Channel as usize);
    channels.dedup_by(|a, b| std::ptr::eq(*a, *b));
    let position = |channel: &Channel| {
        channels
            .iter()
            .position(|other| std::ptr::eq(*other, channel))
            .expect("every case's channel was collected")
    };
    let mut guards: Vec<MutexGuard<'_, Queues>> =
        channels.iter().map(|channel| channel.lock()).collect();

    let start = ROTATE.fetch_add(1, Ordering::Relaxed) % count.max(1);
    for step in 0..count {
        let index = (start + step) % count;
        let case = &mut cases[index];
        // SAFETY: guaranteed by the caller.
        let Some(channel) = (unsafe { case.channel.as_ref() }) else {
            drop(guards);
            if case.send == 0 {
                // SAFETY: the buffer holds `size` bytes.
                unsafe { settle_receive(case, None) };
            } else {
                // SAFETY: the case holds a live value.
                unsafe { fail_send(case.value, case.destroy) };
            }
            return Ok(index as i64);
        };
        let queues = &mut guards[position(channel)];
        if case.send == 0 {
            let message = match try_receive(queues) {
                ReceiveTry::Got(message) => Some(message),
                ReceiveTry::Closed => None,
                ReceiveTry::Empty => continue,
            };
            drop(guards);
            // SAFETY: the buffer holds `size` bytes.
            unsafe { settle_receive(case, message) };
            return Ok(index as i64);
        }
        let (value, size) = (case.value, channel.size);
        // SAFETY: `value` holds `size` bytes.
        let make = || unsafe { Message::copy_of(value, size) };
        match try_send(queues, channel.capacity, &make) {
            SendTry::Done => return Ok(index as i64),
            SendTry::Closed => {
                drop(guards);
                // SAFETY: the case holds a live value.
                unsafe { fail_send(case.value, case.destroy) };
                return Ok(index as i64);
            }
            SendTry::NotReady => {}
        }
    }
    if has_default {
        return Ok(-1);
    }

    let waiting = Waiting::new(waiter);
    for (index, case) in cases.iter().enumerate() {
        // SAFETY: guaranteed by the caller.
        let Some(channel) = (unsafe { case.channel.as_ref() }) else {
            continue;
        };
        let queues = &mut guards[position(channel)];
        let entry = Entry {
            waiting: Arc::clone(&waiting),
            case: index,
            // SAFETY: a send case holds `size` readable bytes.
            message: (case.send != 0)
                .then(|| unsafe { Message::copy_of(case.value, channel.size) }),
        };
        if case.send == 0 {
            queues.receivers.push_back(entry);
        } else {
            queues.senders.push_back(entry);
        }
    }
    drop(guards);
    Err(waiting)
}

/// # Safety
/// Cases must be the same live records supplied to `start_select`.
unsafe fn finish_select(cases: &mut [SelectCase], waiting: &Arc<Waiting>) -> i64 {
    for channel in cases.iter().filter_map(|case| {
        // SAFETY: the caller keeps every channel handle live until completion.
        unsafe { case.channel.as_ref() }
    }) {
        let mut queues = channel.lock();
        let queues = &mut *queues;
        for queue in [&mut queues.receivers, &mut queues.senders] {
            let mut kept = VecDeque::new();
            for entry in queue.drain(..) {
                if Arc::ptr_eq(&entry.waiting, waiting) {
                    if let Some(message) = entry.message {
                        message.free();
                    }
                } else {
                    kept.push_back(entry);
                }
            }
            *queue = kept;
        }
    }

    let (index, outcome, message) = {
        let mut exchange = waiting.exchange();
        (exchange.case, exchange.outcome, exchange.message.take())
    };
    let case = &mut cases[index];
    if case.send == 0 {
        // SAFETY: the buffer holds `size` bytes.
        unsafe { settle_receive(case, message) };
    } else if outcome == Outcome::Closed {
        if let Some(message) = message {
            message.free();
        }
        // SAFETY: the case holds a live value.
        unsafe { fail_send(case.value, case.destroy) };
    }
    index as i64
}

pub struct Selection {
    cases: *mut SelectCase,
    count: usize,
    waiting: Option<Arc<Waiting>>,
    ready: Option<i64>,
}

/// # Safety
/// Keep count valid cases and their channel/value storage pinned through Ready; context must be current.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_channel_start(
    cases: *mut SelectCase,
    count: i64,
    has_default: bool,
    context: *mut Context,
) -> *mut Selection {
    let count = usize::try_from(count).unwrap_or(0);
    let records = unsafe { std::slice::from_raw_parts_mut(cases, count) };
    let waiter = unsafe { &*context }.waker().clone().into();
    // SAFETY: records and their storage stay pinned in the caller's frame.
    let result = unsafe { start_select(records, has_default, waiter) };
    let (ready, waiting) = match result {
        Ok(index) => (Some(index), None),
        Err(waiting) => (None, Some(waiting)),
    };
    Box::into_raw(Box::new(Selection {
        cases,
        count,
        waiting,
        ready,
    }))
}

/// # Safety
/// Poll one live operation serially with pinned cases, current context, and writable i64 out; Ready consumes it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_channel_poll(
    operation: *mut Selection,
    context: *mut Context,
    out: *mut i64,
) -> u8 {
    let selection = unsafe { &*operation };
    if let Some(waiting) = &selection.waiting
        && waiting.exchange().outcome == Outcome::Pending
    {
        unsafe { &mut *context }.pending_internal();
        return 0;
    }
    // SAFETY: Ready consumes the operation exactly once.
    let selection = unsafe { Box::from_raw(operation) };
    let index = match selection.ready {
        Some(index) => index,
        None => {
            // SAFETY: the caller keeps the original records and their storage live.
            let records =
                unsafe { std::slice::from_raw_parts_mut(selection.cases, selection.count) };
            // SAFETY: these are the same records registered by start_select.
            unsafe { finish_select(records, selection.waiting.as_ref().unwrap()) }
        }
    };
    unsafe { out.write(index) };
    1
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::scheduler::{Poll, TestPool};

    #[test]
    fn receive_queue_wakes_both_slot_and_task_waiters_in_order() {
        let pool = TestPool::new(1);
        let queues = Arc::new(Mutex::new(Queues::default()));
        let slot = Arc::new(Slot::default());
        let fiber_waiting = Waiting::new(Arc::clone(&slot).into());
        queues.lock().unwrap().receivers.push_back(Entry {
            waiting: Arc::clone(&fiber_waiting),
            case: 0,
            message: None,
        });
        let task_queues = Arc::clone(&queues);
        let (registered, rx) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let mut waiting: Option<Arc<Waiting>> = None;
        pool.spawn(move |context| {
            if let Some(waiting) = &waiting {
                let message = waiting.exchange().message.take().unwrap();
                let mut value = 0i64;
                // SAFETY: this test queues one i64 message and supplies one i64 destination.
                unsafe { message.move_to((&mut value as *mut i64).cast()) };
                done.send(value).unwrap();
                return Poll::Ready;
            }
            let entry_waiting = Waiting::new(context.waker().clone().into());
            task_queues.lock().unwrap().receivers.push_back(Entry {
                waiting: Arc::clone(&entry_waiting),
                case: 0,
                message: None,
            });
            waiting = Some(entry_waiting);
            registered.send(()).unwrap();
            context.pending_internal()
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        for value in [10i64, 20] {
            // SAFETY: the copied value lives through the call and holds eight readable bytes.
            let make = || unsafe { Message::copy_of((&value as *const i64).cast(), 8) };
            assert!(matches!(
                try_send(&mut queues.lock().unwrap(), 0, &make),
                SendTry::Done
            ));
        }
        slot.park();
        let mut value = 0i64;
        // SAFETY: as above; the first receiver owns the first i64 message.
        unsafe {
            fiber_waiting
                .exchange()
                .message
                .take()
                .unwrap()
                .move_to((&mut value as *mut i64).cast());
        }
        assert_eq!(value, 10);
        assert_eq!(completed.recv_timeout(Duration::from_secs(10)).unwrap(), 20);
        pool.idle();
    }

    #[test]
    fn concurrent_select_completions_claim_one_task_waiter() {
        let pool = TestPool::new(2);
        let (registered, rx) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let mut waiting: Option<Arc<Waiting>> = None;
        pool.spawn(move |context| {
            if let Some(waiting) = &waiting {
                let exchange = waiting.exchange();
                assert!(exchange.outcome == Outcome::Closed);
                done.send(exchange.case).unwrap();
                return Poll::Ready;
            }
            let entry_waiting = Waiting::new(context.waker().clone().into());
            registered.send(Arc::clone(&entry_waiting)).unwrap();
            waiting = Some(entry_waiting);
            context.pending_internal()
        });
        let waiting = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        std::thread::scope(|scope| {
            for case in 0..8 {
                let waiting = Arc::clone(&waiting);
                scope.spawn(move || {
                    if waiting.claim() {
                        waiting.complete(case, Outcome::Closed, None);
                    }
                });
            }
        });
        assert!(completed.recv_timeout(Duration::from_secs(10)).unwrap() < 8);
        pool.idle();
    }
    struct TestSelection {
        values: Box<[i64]>,
        cases: Vec<SelectCase>,
        operation: *mut Selection,
    }

    // SAFETY: storage stays pinned with one poll owner; channel handles outlive it.
    unsafe impl Send for TestSelection {}

    impl TestSelection {
        fn new(channels: &[*const Channel], send: bool) -> Self {
            let mut values = vec![0; channels.len()].into_boxed_slice();
            let cases = channels
                .iter()
                .enumerate()
                .map(|(index, &channel)| SelectCase {
                    channel,
                    send: u8::from(send),
                    value: (&mut values[index] as *mut i64).cast(),
                    size: 8,
                    destroy: None,
                    received: 0,
                })
                .collect();
            Self {
                values,
                cases,
                operation: std::ptr::null_mut(),
            }
        }

        fn poll(&mut self, context: &mut Context) -> Option<usize> {
            if self.operation.is_null() {
                // SAFETY: records and values are stable for the lifetime of this test frame.
                self.operation = unsafe {
                    zore_channel_start(
                        self.cases.as_mut_ptr(),
                        self.cases.len() as i64,
                        false,
                        context,
                    )
                };
            }
            let mut chosen = -1;
            // SAFETY: the frame is polled serially and all channels remain live.
            if unsafe { zore_channel_poll(self.operation, context, &mut chosen) } == 0 {
                None
            } else {
                self.operation = std::ptr::null_mut();
                Some(usize::try_from(chosen).unwrap())
            }
        }
    }

    #[test]
    fn channel_poll_registers_once_and_latches_wake_before_pending() {
        let pool = TestPool::new(1);
        let channel = zore_channel_make(8, 0, None);
        let mut frame = TestSelection::new(&[channel], false);
        let (registered, registration) = mpsc::channel();
        let (released, release) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let mut first = true;
        pool.spawn(move |context| {
            if first {
                first = false;
                assert_eq!(frame.poll(context), None);
                assert_eq!(frame.poll(context), None);
                registered.send(()).unwrap();
                // Force the wake before Pending returns.
                release.recv_timeout(Duration::from_secs(10)).unwrap();
                return Poll::Pending;
            }
            assert_eq!(frame.poll(context), Some(0));
            assert_eq!(frame.cases[0].received, 1);
            done.send(frame.values[0]).unwrap();
            Poll::Ready
        });
        registration.recv_timeout(Duration::from_secs(10)).unwrap();
        // SAFETY: channel remains live; the sender supplies one i64 value.
        unsafe {
            assert_eq!((*channel).lock().receivers.len(), 1);
            let mut value = 42i64;
            zore_channel_send(channel, (&mut value as *mut i64).cast(), None);
        }
        released.send(()).unwrap();
        assert_eq!(completed.recv_timeout(Duration::from_secs(10)).unwrap(), 42);
        pool.idle();
        // SAFETY: the poll task has completed and no other handle uses this channel.
        unsafe { zore_channel_release(channel) };
    }

    #[test]
    fn channel_poll_concurrent_select_winners_remove_duplicate_registrations() {
        for _ in 0..100 {
            let pool = TestPool::new(2);
            let a = zore_channel_make(8, 1, None);
            let b = zore_channel_make(8, 1, None);
            let mut frame = TestSelection::new(&[a, a, b], false);
            let (registered, registration) = mpsc::channel();
            let (done, completed) = mpsc::channel();
            let mut first = true;
            pool.spawn(move |context| {
                if first {
                    first = false;
                    assert_eq!(frame.poll(context), None);
                    registered.send(()).unwrap();
                    return Poll::Pending;
                }
                let Some(index) = frame.poll(context) else {
                    return Poll::Pending;
                };
                assert_eq!(
                    frame.cases.iter().filter(|case| case.received != 0).count(),
                    1
                );
                done.send(frame.values[index]).unwrap();
                Poll::Ready
            });
            registration.recv_timeout(Duration::from_secs(10)).unwrap();
            std::thread::scope(|scope| {
                for (channel, value) in [(a as usize, 11i64), (b as usize, 29)] {
                    scope.spawn(move || {
                        let mut value = value;
                        // SAFETY: the live channel stores i64; capacity lets a losing send buffer without blocking.
                        unsafe {
                            zore_channel_send(
                                channel as *const Channel,
                                (&mut value as *mut i64).cast(),
                                None,
                            )
                        };
                    });
                }
            });
            let chosen = completed.recv_timeout(Duration::from_secs(10)).unwrap();
            pool.idle();
            let mut total = chosen;
            // SAFETY: all tasks completed; queues contain at most the losing sender's i64.
            unsafe {
                for channel in [a, b] {
                    let mut queues = (*channel).lock();
                    assert!(queues.receivers.is_empty());
                    assert!(queues.senders.is_empty());
                    if let Some(message) = queues.buffer.pop_front() {
                        let mut value = 0i64;
                        message.move_to((&mut value as *mut i64).cast());
                        total += value;
                    }
                    drop(queues);
                    zore_channel_release(channel);
                }
            }
            assert_eq!(total, 40);
        }
    }

    #[test]
    fn channel_poll_close_wakes_pending_send_and_receive() {
        for send in [false, true] {
            let pool = TestPool::new(1);
            let channel = zore_channel_make(8, 0, None);
            let mut frame = TestSelection::new(&[channel], send);
            let (registered, registration) = mpsc::channel();
            let (done, completed) = mpsc::channel();
            let mut first = true;
            pool.spawn(move |context| {
                if first {
                    first = false;
                    assert_eq!(frame.poll(context), None);
                    registered.send(()).unwrap();
                    return Poll::Pending;
                }
                assert_eq!(frame.poll(context), Some(0));
                assert_eq!(frame.cases[0].received, 0);
                if send {
                    assert_eq!(super::super::panic::take().as_deref(), Some(SEND_CLOSED));
                } else {
                    assert_eq!(frame.values[0], 0);
                }
                done.send(()).unwrap();
                Poll::Ready
            });
            registration.recv_timeout(Duration::from_secs(10)).unwrap();
            // SAFETY: the channel remains live until the task completes.
            unsafe { zore_channel_close(channel) };
            completed.recv_timeout(Duration::from_secs(10)).unwrap();
            pool.idle();
            // SAFETY: the poll task has completed and no other handle uses this channel.
            unsafe { zore_channel_release(channel) };
        }
    }
}
