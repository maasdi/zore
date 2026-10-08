//! Where a task's code runs. Where stack switching is available, tasks are fibers on small
//! private stacks scheduled over a few worker threads; elsewhere each task gets a thread.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::task::Shared;

type Job = Box<dyn FnOnce() + Send>;

fn wait_on_condvar(shared: &Shared) {
    let mut state = shared.lock();
    while !state.finished {
        state = block(&shared.finished, state);
    }
}

fn block<'a, T>(condvar: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    let _blocking = super::scheduler::BlockingGuard::enter();
    condvar
        .wait(guard)
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
struct SlotState {
    woken: bool,
    parked: Option<Waiter>,
    /// Counted as blocked for deadlock detection until woken.
    counted: bool,
}

/// One task's place to sleep until another task wakes it; it serves a single wait.
#[derive(Default)]
pub(super) struct Slot {
    state: Mutex<SlotState>,
    ready: Condvar,
    /// Only another task can wake it, so waiting on it can deadlock.
    internal: bool,
}

impl Slot {
    /// A slot that only another task can wake.
    pub(super) fn internal() -> Self {
        Self {
            internal: true,
            ..Self::default()
        }
    }

    fn lock(&self) -> MutexGuard<'_, SlotState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn wake(&self) {
        let mut state = self.lock();
        state.woken = true;
        let parked = state.parked.take();
        if std::mem::take(&mut state.counted) {
            super::deadlock::remove_blocked();
        }
        drop(state);
        self.ready.notify_all();
        if let Some(waiter) = parked {
            wake(waiter);
        }
    }

    /// A fiber parks so its worker runs other tasks; any other thread blocks.
    pub(super) fn park(self: &Arc<Self>) {
        if self.internal {
            let mut state = self.lock();
            if state.woken {
                return;
            }
            state.counted = true;
            super::deadlock::add_blocked();
            drop(state);
            super::deadlock::check();
        }
        if imp::park(self) {
            return;
        }
        let mut state = self.lock();
        while !state.woken {
            state = block(&self.ready, state);
        }
    }

    /// Called by a worker after the fiber has left: true when it was woken meanwhile.
    #[cfg(all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        any(target_os = "linux", target_os = "macos")
    ))]
    fn settle(&self, waiter: Waiter) -> bool {
        let mut state = self.lock();
        if state.woken {
            return true;
        }
        state.parked = Some(waiter);
        false
    }

    #[cfg(all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        any(target_os = "linux", target_os = "macos")
    ))]
    fn is_woken(&self) -> bool {
        self.lock().woken
    }
}

#[cfg(all(
    any(target_arch = "x86_64", target_arch = "aarch64"),
    any(target_os = "linux", target_os = "macos")
))]
mod imp {
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::sync::{Condvar, Mutex, MutexGuard, Once, OnceLock};

    use super::super::panic::{self, PanicState};
    use super::{Arc, Job, Shared, Slot, block, wait_on_condvar};

    const STACK_BYTES: usize = 256 << 10;

    /// Saves the callee-saved registers on the current stack, records its pointer in `save`, and
    /// continues on the stack at `to`, which holds registers saved the same way.
    #[cfg(target_arch = "x86_64")]
    #[unsafe(naked)]
    unsafe extern "C" fn switch(save: *mut *mut u8, to: *mut u8) {
        std::arch::naked_asm!(
            "push rbp",
            "push rbx",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "mov [rdi], rsp",
            "mov rsp, rsi",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbx",
            "pop rbp",
            "ret",
        )
    }

    /// A new stack first returns here, with the fiber in `r12` and its entry function in `r13`.
    #[cfg(target_arch = "x86_64")]
    #[unsafe(naked)]
    unsafe extern "C" fn start() {
        std::arch::naked_asm!("mov rdi, r12", "call r13", "ud2")
    }

    /// Saves `x19`–`x30` and `d8`–`d15` in a 160-byte frame, records the stack pointer in `save`,
    /// and continues on the stack at `to`, which holds a frame saved the same way.
    #[cfg(target_arch = "aarch64")]
    #[unsafe(naked)]
    unsafe extern "C" fn switch(save: *mut *mut u8, to: *mut u8) {
        std::arch::naked_asm!(
            "sub sp, sp, #160",
            "stp x19, x20, [sp, #0]",
            "stp x21, x22, [sp, #16]",
            "stp x23, x24, [sp, #32]",
            "stp x25, x26, [sp, #48]",
            "stp x27, x28, [sp, #64]",
            "stp x29, x30, [sp, #80]",
            "stp d8, d9, [sp, #96]",
            "stp d10, d11, [sp, #112]",
            "stp d12, d13, [sp, #128]",
            "stp d14, d15, [sp, #144]",
            "mov x9, sp",
            "str x9, [x0]",
            "mov sp, x1",
            "ldp x19, x20, [sp, #0]",
            "ldp x21, x22, [sp, #16]",
            "ldp x23, x24, [sp, #32]",
            "ldp x25, x26, [sp, #48]",
            "ldp x27, x28, [sp, #64]",
            "ldp x29, x30, [sp, #80]",
            "ldp d8, d9, [sp, #96]",
            "ldp d10, d11, [sp, #112]",
            "ldp d12, d13, [sp, #128]",
            "ldp d14, d15, [sp, #144]",
            "add sp, sp, #160",
            "ret",
        )
    }

    /// A new stack first returns here, with the fiber in `x19` and its entry function in `x20`.
    #[cfg(target_arch = "aarch64")]
    #[unsafe(naked)]
    unsafe extern "C" fn start() {
        std::arch::naked_asm!("mov x0, x19", "blr x20", "brk #0")
    }

    unsafe extern "C" {
        fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
        fn mprotect(addr: *mut u8, len: usize, prot: i32) -> i32;
        fn getpagesize() -> i32;
    }

    const PROT_NONE: i32 = 0;
    const PROT_READ_WRITE: i32 = 3;
    const MAP_PRIVATE: i32 = 2;
    #[cfg(target_os = "linux")]
    const MAP_ANONYMOUS: i32 = 0x20;
    #[cfg(target_os = "macos")]
    const MAP_ANONYMOUS: i32 = 0x1000;

    enum Request {
        None,
        Finished,
        /// Wake me when this task finishes.
        Wait(Arc<Shared>),
        /// Wake me when this slot is woken.
        Park(Arc<Slot>),
    }

    struct Fiber {
        sp: *mut u8,
        stack: *mut u8,
        /// Where the worker that last resumed this fiber saved its own stack pointer.
        worker_sp: *mut *mut u8,
        panic: PanicState,
        job: Option<Job>,
        request: Request,
    }

    #[derive(Clone, Copy)]
    struct Ptr(*mut Fiber);

    // SAFETY: a fiber is run by one worker at a time, and queued or parked fibers are idle.
    unsafe impl Send for Ptr {}

    /// A fiber parked until a task finishes.
    #[derive(Clone, Copy)]
    pub(crate) struct Waiter(Ptr);

    static QUEUE: Mutex<VecDeque<Ptr>> = Mutex::new(VecDeque::new());
    static READY: Condvar = Condvar::new();
    static WORKERS: Once = Once::new();
    static STACKS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

    thread_local! {
        static CURRENT: Cell<*mut Fiber> = const { Cell::new(std::ptr::null_mut()) };
    }

    fn queue() -> MutexGuard<'static, VecDeque<Ptr>> {
        QUEUE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn enqueue(fiber: Ptr) {
        queue().push_back(fiber);
        READY.notify_one();
    }

    fn dequeue() -> Ptr {
        let mut queue = queue();
        loop {
            if let Some(fiber) = queue.pop_front() {
                return fiber;
            }
            queue = block(&READY, queue);
        }
    }

    fn start_workers() {
        WORKERS.call_once(|| {
            let count = std::thread::available_parallelism().map_or(2, |n| n.get().max(2));
            for _ in 0..count {
                let started = std::thread::Builder::new().spawn(|| {
                    loop {
                        // SAFETY: a queued fiber is idle, so this worker may run it.
                        unsafe { run(dequeue().0) };
                    }
                });
                if started.is_err() {
                    super::super::panic::fail(b"cannot start a worker thread");
                }
            }
        });
    }

    pub(crate) fn spawn(job: Job) {
        let fiber = Box::new(Fiber {
            sp: std::ptr::null_mut(),
            stack: std::ptr::null_mut(),
            worker_sp: std::ptr::null_mut(),
            panic: PanicState::default(),
            job: Some(job),
            request: Request::None,
        });
        start_workers();
        enqueue(Ptr(Box::into_raw(fiber)));
    }

    pub(crate) fn wake(waiter: Waiter) {
        enqueue(waiter.0);
    }

    #[inline(never)]
    fn current() -> *mut Fiber {
        CURRENT.with(Cell::get)
    }

    #[inline(never)]
    fn set_current(fiber: *mut Fiber) {
        CURRENT.with(|current| current.set(fiber));
    }

    /// A fiber waits by parking, so its worker runs other tasks; any other thread blocks.
    pub(crate) fn wait_for(shared: &Arc<Shared>) {
        let fiber = current();
        if fiber.is_null() {
            wait_on_condvar(shared);
            return;
        }
        if shared.lock().finished {
            return;
        }
        // SAFETY: `fiber` is the fiber running on this thread.
        unsafe { leave(fiber, Request::Wait(Arc::clone(shared))) };
    }

    /// Parks the running fiber on the slot; false when the caller is not a fiber.
    pub(crate) fn park(slot: &Arc<Slot>) -> bool {
        let fiber = current();
        if fiber.is_null() {
            return false;
        }
        if !slot.is_woken() {
            // SAFETY: `fiber` is the fiber running on this thread.
            unsafe { leave(fiber, Request::Park(Arc::clone(slot))) };
        }
        true
    }

    /// Hands control back to the worker; the fiber continues here when it is resumed.
    unsafe fn leave(fiber: *mut Fiber, request: Request) {
        // SAFETY: the caller runs on this fiber's stack, resumed by the worker that set `worker_sp`.
        unsafe {
            (*fiber).request = request;
            let worker = *(*fiber).worker_sp;
            switch(&mut (*fiber).sp, worker);
        }
    }

    extern "C" fn enter(fiber: *mut Fiber) -> ! {
        // SAFETY: a fiber's first run happens once, on its own stack.
        unsafe {
            let job = (*fiber).job.take().expect("a fiber starts with a job");
            job();
            leave(fiber, Request::Finished);
        }
        std::process::abort();
    }

    /// The guard is one page, whatever size the system uses.
    fn guard_bytes() -> usize {
        static PAGE: OnceLock<usize> = OnceLock::new();
        *PAGE.get_or_init(|| {
            // SAFETY: plain system call.
            usize::try_from(unsafe { getpagesize() }).unwrap_or(4096)
        })
    }

    fn stack_top(base: usize) -> usize {
        base + guard_bytes() + STACK_BYTES
    }

    fn allocate_stack() -> usize {
        let pooled = STACKS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop();
        if let Some(base) = pooled {
            return base;
        }
        // SAFETY: an anonymous private mapping; the lowest page becomes an inaccessible guard.
        unsafe {
            let base = mmap(
                std::ptr::null_mut(),
                guard_bytes() + STACK_BYTES,
                PROT_READ_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            );
            if base as isize == -1 || mprotect(base, guard_bytes(), PROT_NONE) != 0 {
                super::super::panic::fail(b"cannot start a task: out of stack space");
            }
            base as usize
        }
    }

    /// Lays out a stack that `switch` can resume into `start`, which calls `enter(fiber)`.
    #[cfg(target_arch = "x86_64")]
    fn prepare(base: usize, fiber: *mut Fiber) -> *mut u8 {
        let top = stack_top(base);
        let sp = top - 8 - 6 * 8;
        let entry: extern "C" fn(*mut Fiber) -> ! = enter;
        let slots = [
            0,
            0,
            entry as usize,
            fiber as usize,
            0,
            0,
            start as unsafe extern "C" fn() as usize,
        ];
        // SAFETY: the slots lie inside the fresh stack, below its top.
        unsafe {
            std::ptr::copy_nonoverlapping(slots.as_ptr(), sp as *mut usize, slots.len());
        }
        sp as *mut u8
    }

    /// Lays out a stack that `switch` can resume into `start`, which calls `enter(fiber)`.
    #[cfg(target_arch = "aarch64")]
    fn prepare(base: usize, fiber: *mut Fiber) -> *mut u8 {
        let top = stack_top(base);
        let sp = top - 160;
        let entry: extern "C" fn(*mut Fiber) -> ! = enter;
        let mut frame = [0usize; 20];
        frame[0] = fiber as usize;
        frame[1] = entry as usize;
        frame[11] = start as unsafe extern "C" fn() as usize;
        // SAFETY: the frame lies inside the fresh stack, below its top.
        unsafe {
            std::ptr::copy_nonoverlapping(frame.as_ptr(), sp as *mut usize, frame.len());
        }
        sp as *mut u8
    }

    /// Runs the fiber until it parks or finishes, then acts on what it asked for.
    unsafe fn run(fiber: *mut Fiber) {
        // SAFETY: the fiber is idle and owned by this worker until it switches back.
        unsafe {
            if (*fiber).stack.is_null() {
                let base = allocate_stack();
                (*fiber).stack = base as *mut u8;
                (*fiber).sp = prepare(base, fiber);
            }
            let mut saved: *mut u8 = std::ptr::null_mut();
            (*fiber).worker_sp = &mut saved;
            set_current(fiber);
            panic::swap_state(&mut (*fiber).panic);
            switch(&mut saved, (*fiber).sp);
            panic::swap_state(&mut (*fiber).panic);
            set_current(std::ptr::null_mut());
            match std::mem::replace(&mut (*fiber).request, Request::None) {
                Request::Finished => {
                    STACKS
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push((*fiber).stack as usize);
                    drop(Box::from_raw(fiber));
                }
                Request::Wait(shared) => {
                    let mut state = shared.lock();
                    if state.finished {
                        drop(state);
                        enqueue(Ptr(fiber));
                    } else {
                        let slot = Arc::new(Slot::internal());
                        // Task completion removes the join count; slot settlement must not count it twice.
                        slot.settle(Waiter(Ptr(fiber)));
                        state.waiter = Some(slot.into());
                    }
                }
                Request::Park(slot) => {
                    if slot.settle(Waiter(Ptr(fiber))) {
                        enqueue(Ptr(fiber));
                    }
                }
                Request::None => enqueue(Ptr(fiber)),
            }
        }
    }
}

#[cfg(not(all(
    any(target_arch = "x86_64", target_arch = "aarch64"),
    any(target_os = "linux", target_os = "macos")
)))]
mod imp {
    use super::{Arc, Job, Shared, Slot, wait_on_condvar};

    /// Never made: a thread waits on the task's condition variable.
    #[derive(Clone, Copy)]
    pub(crate) struct Waiter;

    pub(crate) fn spawn(job: Job) {
        let started = std::thread::Builder::new().stack_size(16 << 20).spawn(job);
        if started.is_err() {
            super::super::panic::fail(b"cannot start a task");
        }
    }

    pub(crate) fn wake(_: Waiter) {}

    pub(crate) fn park(_: &Arc<Slot>) -> bool {
        false
    }

    pub(crate) fn wait_for(shared: &Arc<Shared>) {
        wait_on_condvar(shared);
    }
}

/// Waits until the task finishes; the waiter counts as blocked for deadlock detection.
pub(super) fn wait_for(shared: &Arc<Shared>) {
    {
        let mut state = shared.lock();
        if state.finished {
            return;
        }
        state.counted = true;
        super::deadlock::add_blocked();
    }
    super::deadlock::check();
    imp::wait_for(shared);
}

pub(super) use imp::{Waiter, spawn, wake};
