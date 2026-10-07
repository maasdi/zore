//! Where a task's code runs. Where stack switching is available, tasks are fibers on small
//! private stacks scheduled over a few worker threads; elsewhere each task gets a thread.

use std::sync::{Arc, Condvar, MutexGuard};

use super::task::Shared;

type Job = Box<dyn FnOnce() + Send>;

fn wait_on_condvar(shared: &Shared) {
    let mut state = shared.lock();
    while !state.finished {
        state = block(&shared.finished, state);
    }
}

fn block<'a, T>(condvar: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    condvar
        .wait(guard)
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(all(target_arch = "x86_64", any(target_os = "linux", target_os = "macos")))]
mod imp {
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::sync::{Condvar, Mutex, MutexGuard, Once};

    use super::super::panic::{self, PanicState};
    use super::{Arc, Job, Shared, block, wait_on_condvar};

    const STACK_BYTES: usize = 256 << 10;
    const GUARD_BYTES: usize = 4096;

    /// Saves the callee-saved registers on the current stack, records its pointer in `save`, and
    /// continues on the stack at `to`, which holds registers saved the same way.
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
    #[unsafe(naked)]
    unsafe extern "C" fn start() {
        std::arch::naked_asm!("mov rdi, r12", "call r13", "ud2")
    }

    unsafe extern "C" {
        fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
        fn mprotect(addr: *mut u8, len: usize, prot: i32) -> i32;
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

    fn stack_top(base: usize) -> usize {
        base + GUARD_BYTES + STACK_BYTES
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
                GUARD_BYTES + STACK_BYTES,
                PROT_READ_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            );
            if base as isize == -1 || mprotect(base, GUARD_BYTES, PROT_NONE) != 0 {
                super::super::panic::fail(b"cannot start a task: out of stack space");
            }
            base as usize
        }
    }

    /// Lays out a stack that `switch` can resume into `start`, which calls `enter(fiber)`.
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
                        state.waiter = Some(Waiter(Ptr(fiber)));
                    }
                }
                Request::None => enqueue(Ptr(fiber)),
            }
        }
    }
}

#[cfg(not(all(target_arch = "x86_64", any(target_os = "linux", target_os = "macos"))))]
mod imp {
    use super::{Arc, Job, Shared, wait_on_condvar};

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

    pub(crate) fn wait_for(shared: &Arc<Shared>) {
        wait_on_condvar(shared);
    }
}

pub(super) use imp::{Waiter, spawn, wait_for, wake};
