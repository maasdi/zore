use std::alloc::Layout;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicIsize, AtomicPtr, AtomicU32, AtomicUsize, Ordering, fence};

use super::alloc::{alloc_aligned, free_aligned, out_of_memory};

/// Built text lives in a buffer: this header, then `capacity` bytes. No string reads
/// past `used`, so appending at the end never changes the text any existing string
/// shows. The buffer is freed when its last owner lets go.
#[repr(C)]
struct Header {
    /// How many strings read the buffer, with `PINNED` set once a package-level value
    /// holds it until the program ends, so it is not a leak.
    owners: AtomicUsize,
    used: AtomicUsize,
    capacity: usize,
    /// The error a wrapped error's text was made from, set before the text is shared.
    cause: AtomicPtr<Cause>,
}

struct Cause {
    data: *const u8,
    len: usize,
    text_len: usize,
}

const PINNED: usize = 1 << (usize::BITS - 1);
const HEADER: usize = size_of::<Header>();

const PAGE_BITS: u32 = 12;
const PAGE: usize = 1 << PAGE_BITS;
const LEAF_BITS: u32 = 14;
const MIDDLE_BITS: u32 = 14;
const ADDRESS_BITS: u32 = 48;
const ROOT_LEN: usize = 1 << (ADDRESS_BITS - PAGE_BITS - LEAF_BITS - MIDDLE_BITS);
const LEAF_LEN: usize = 1 << LEAF_BITS;
const MIDDLE_LEN: usize = 1 << MIDDLE_BITS;

/// Per page: zero, a `SLAB` entry with class and span index, or 1 + pages into a large buffer.
type Leaf = [AtomicU32; LEAF_LEN];
type Middle = [AtomicPtr<Leaf>; MIDDLE_LEN];

/// Read without a lock: pages are marked before their text is shared, cleared after its last owner.
static PAGES: [AtomicPtr<Middle>; ROOT_LEN] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; ROOT_LEN];

const SLAB: u32 = 1 << 31;
const CLASS_SHIFT: u32 = 8;
const INDEX_MASK: u32 = (1 << CLASS_SHIFT) - 1;

/// Small buffers, header included, are slots of one of these sizes, about a quarter apart.
const CLASSES: [usize; 38] = [
    48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024, 1280,
    1536, 1792, 2048, 2560, 3072, 3584, 4096, 5120, 6144, 7168, 8192, 10240, 12288, 14336, 16384,
    20480, 24576, 28672, 32768,
];
const LARGEST_SLOT: usize = CLASSES[CLASSES.len() - 1];

/// Slots are carved from spans of whole pages, each page knowing its index in the span.
const SPAN: usize = 64 * PAGE;

const RECIPROCAL_SHIFT: u32 = 40;

/// `offset * RECIPROCALS[class] >> RECIPROCAL_SHIFT` is `offset / CLASSES[class]` within a span.
const RECIPROCALS: [u64; CLASSES.len()] = {
    let mut reciprocals = [0; CLASSES.len()];
    let mut class = 0;
    while class < CLASSES.len() {
        reciprocals[class] = (1u64 << RECIPROCAL_SHIFT).div_ceil(CLASSES[class] as u64);
        class += 1;
    }
    reciprocals
};

fn slab_entry(class: usize, index: usize) -> u32 {
    SLAB | (class as u32) << CLASS_SHIFT | index as u32
}

fn slab_class(entry: u32) -> usize {
    ((entry & !SLAB) >> CLASS_SHIFT) as usize
}

/// The smallest class that holds `(size - 1) / 16 + 1` sixteen-byte units.
const CLASS_OF_UNITS: [u8; LARGEST_SLOT / 16] = {
    let mut table = [0; LARGEST_SLOT / 16];
    let mut units = 0;
    let mut class = 0;
    while units < table.len() {
        while CLASSES[class] < (units + 1) * 16 {
            class += 1;
        }
        table[units] = class as u8;
        units += 1;
    }
    table
};

fn class_for(size: usize) -> Option<usize> {
    (size <= LARGEST_SLOT).then(|| CLASS_OF_UNITS[(size.max(1) - 1) / 16] as usize)
}

/// Unpinned buffers counted per thread group, so tasks on different workers do not
/// contend for one counter; the sum is the count.
#[repr(align(128))]
struct Stripe(AtomicIsize);

const STRIPES: usize = 16;
static UNPINNED_BUFFERS: [Stripe; STRIPES] = [const { Stripe(AtomicIsize::new(0)) }; STRIPES];

fn count_unpinned(change: isize) {
    static NEXT_STRIPE: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static STRIPE: usize = NEXT_STRIPE.fetch_add(1, Ordering::Relaxed) % STRIPES;
    }
    let stripe = STRIPE.try_with(|stripe| *stripe).unwrap_or(0);
    UNPINNED_BUFFERS[stripe]
        .0
        .fetch_add(change, Ordering::Relaxed);
}

/// Tests that count buffers cannot overlap with any test that makes text.
#[cfg(test)]
pub(super) fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The node in `slot`, made zeroed first when `create` is set.
fn child<T>(slot: &AtomicPtr<T>, create: bool) -> Option<&'static T> {
    let mut node = slot.load(Ordering::Acquire);
    if node.is_null() {
        if !create {
            return None;
        }
        let layout = Layout::new::<T>();
        // SAFETY: the layout is nonzero, and all-zero bytes are valid empty atomics.
        let fresh = unsafe { std::alloc::alloc_zeroed(layout) }.cast::<T>();
        if fresh.is_null() {
            out_of_memory();
        }
        node = match slot.compare_exchange(
            std::ptr::null_mut(),
            fresh,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => fresh,
            Err(installed) => {
                // SAFETY: `fresh` came from `alloc_zeroed(layout)` and was never shared.
                unsafe { std::alloc::dealloc(fresh.cast(), layout) };
                installed
            }
        };
    }
    // SAFETY: installed nodes are never freed.
    Some(unsafe { &*node })
}

fn leaf(page: usize, create: bool) -> Option<&'static Leaf> {
    let root = PAGES.get(page >> (LEAF_BITS + MIDDLE_BITS))?;
    let middle = child(root, create)?;
    child(&middle[(page >> LEAF_BITS) % MIDDLE_LEN], create)
}

/// Sets each page of `size` bytes at `base` to `entry(offset)`, its offset in pages.
fn mark(base: usize, size: usize, entry: impl Fn(usize) -> u32) {
    let first = base >> PAGE_BITS;
    let count = size >> PAGE_BITS;
    let mut offset = 0;
    while offset < count {
        let page = first + offset;
        let Some(leaf) = leaf(page, true) else {
            unsupported_address();
        };
        let start = page % LEAF_LEN;
        let run = (LEAF_LEN - start).min(count - offset);
        for (step, slot) in leaf[start..start + run].iter().enumerate() {
            slot.store(entry(offset + step), Ordering::Relaxed);
        }
        offset += run;
    }
}

fn page_entry(address: usize) -> u32 {
    let page = address >> PAGE_BITS;
    leaf(page, false).map_or(0, |leaf| leaf[page % LEAF_LEN].load(Ordering::Relaxed))
}

fn unsupported_address() -> ! {
    use std::io::Write;
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(b"fatal: text storage outside the supported address range\n");
    let _ = stderr.flush();
    std::process::abort();
}

/// A live buffer, reached through a string that keeps it alive.
#[derive(Clone, Copy)]
struct Buffer(NonNull<Header>);

impl std::ops::Deref for Buffer {
    type Target = Header;

    fn deref(&self) -> &Header {
        // SAFETY: the buffer is live while a caller holds a string that reads it.
        unsafe { self.0.as_ref() }
    }
}

impl Buffer {
    fn base(self) -> *mut u8 {
        self.0.as_ptr().cast()
    }

    fn text(self) -> *mut u8 {
        self.base().wrapping_add(HEADER)
    }

    fn cause_text(&self) -> Option<&Cause> {
        // SAFETY: a cause lives as long as the buffer that points at it.
        unsafe { self.cause.load(Ordering::Acquire).as_ref() }
    }
}

/// The buffer that holds this text; none for static or empty text.
fn buffer_of(data: *const u8, len: usize) -> Option<Buffer> {
    if len == 0 {
        return None;
    }
    let address = data.addr();
    let entry = page_entry(address);
    let base = if entry & SLAB != 0 {
        let class = slab_class(entry);
        let span = ((address >> PAGE_BITS) - (entry & INDEX_MASK) as usize) << PAGE_BITS;
        let slot = ((address - span) as u64 * RECIPROCALS[class]) >> RECIPROCAL_SHIFT;
        span + slot as usize * CLASSES[class]
    } else if entry != 0 {
        ((address >> PAGE_BITS) + 1 - entry as usize) << PAGE_BITS
    } else {
        return None;
    };
    let header = data.cast_mut().wrapping_byte_sub(address - base).cast();
    NonNull::new(header).map(Buffer)
}

/// Free slots linked through their text, plus the uncarved span from `next` to `end`.
struct FreeList {
    head: *mut u8,
    len: usize,
    next: *mut u8,
    end: *mut u8,
}

// SAFETY: a list only links slots that nobody reads, and moves with whoever holds it.
unsafe impl Send for FreeList {}

impl FreeList {
    const EMPTY: Self = Self {
        head: std::ptr::null_mut(),
        len: 0,
        next: std::ptr::null_mut(),
        end: std::ptr::null_mut(),
    };

    fn push(&mut self, slot: *mut u8) {
        // SAFETY: a free slot is ours, with room for a link after its header.
        unsafe { slot.add(HEADER).cast::<*mut u8>().write(self.head) };
        self.head = slot;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<*mut u8> {
        let slot = NonNull::new(self.head)?.as_ptr();
        // SAFETY: a listed slot holds the link written by `push`.
        self.head = unsafe { slot.add(HEADER).cast::<*mut u8>().read() };
        self.len -= 1;
        Some(slot)
    }

    fn move_to(&mut self, other: &mut Self, count: usize) {
        for _ in 0..count {
            let Some(slot) = self.pop() else {
                return;
            };
            other.push(slot);
        }
    }
}

/// How many free slots of a class move between a thread and the shared lists at once.
const BATCH: usize = 64;

/// The free slots of a class that no thread holds, and the span remainders ended threads left.
struct Shared {
    free: FreeList,
    spans: Vec<(*mut u8, *mut u8)>,
}

// SAFETY: like `FreeList`, the remainders are untouched storage that nobody reads.
unsafe impl Send for Shared {}

impl Shared {
    fn take_span(&mut self, class: usize, list: &mut FreeList) {
        if has_room(&self.free, class) {
            (list.next, list.end) = (self.free.next, self.free.end);
            (self.free.next, self.free.end) = (std::ptr::null_mut(), std::ptr::null_mut());
        } else if let Some(span) = self.spans.pop() {
            (list.next, list.end) = span;
        }
    }
}

static SHARED: [std::sync::Mutex<Shared>; CLASSES.len()] = [const {
    std::sync::Mutex::new(Shared {
        free: FreeList::EMPTY,
        spans: Vec::new(),
    })
}; CLASSES.len()];

fn shared(class: usize) -> std::sync::MutexGuard<'static, Shared> {
    SHARED[class]
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Each thread keeps its own free slots, so most allocations and frees take no lock.
struct Cache([std::cell::UnsafeCell<FreeList>; CLASSES.len()]);

impl Drop for Cache {
    fn drop(&mut self) {
        for (class, list) in self.0.iter_mut().enumerate() {
            let list = list.get_mut();
            let mut shared = shared(class);
            let len = list.len;
            list.move_to(&mut shared.free, len);
            if has_room(list, class) {
                shared.spans.push((list.next, list.end));
            }
        }
    }
}

thread_local! {
    static CACHE: Cache = const { Cache([const { std::cell::UnsafeCell::new(FreeList::EMPTY) }; CLASSES.len()]) };
}

fn carve(list: &mut FreeList, class: usize) -> Option<*mut u8> {
    if !has_room(list, class) {
        return None;
    }
    let slot = list.next;
    list.next = slot.wrapping_add(CLASSES[class]);
    Some(slot)
}

fn has_room(list: &FreeList, class: usize) -> bool {
    !list.next.is_null() && list.end.addr() - list.next.addr() >= CLASSES[class]
}

/// A fresh zeroed span of `class`: every slot in it reads zero owners, so it counts as free.
fn new_span(class: usize, list: &mut FreeList) {
    let span = super::alloc::alloc_kept(SPAN, PAGE);
    mark(span.expose_provenance(), SPAN, |index| {
        slab_entry(class, index)
    });
    list.next = span;
    list.end = span.wrapping_add(SPAN);
}

fn alloc_slot(class: usize) -> *mut u8 {
    let slot = CACHE
        .try_with(|cache| {
            // SAFETY: only this thread reaches its cache, and nothing here reenters it.
            alloc_cached(class, unsafe { &mut *cache.0[class].get() })
        })
        .unwrap_or_else(|_| alloc_shared(class));
    super::alloc::count_live(CLASSES[class], true);
    slot
}

fn alloc_cached(class: usize, list: &mut FreeList) -> *mut u8 {
    if let Some(slot) = list.pop().or_else(|| carve(list, class)) {
        return slot;
    }
    let mut shared = shared(class);
    shared.free.move_to(list, BATCH);
    if list.len == 0 {
        shared.take_span(class, list);
    }
    drop(shared);
    if let Some(slot) = list.pop().or_else(|| carve(list, class)) {
        return slot;
    }
    new_span(class, list);
    carve(list, class).expect("a fresh span has a slot")
}

/// Allocates once this thread's cache is gone, holding the shared lock throughout.
fn alloc_shared(class: usize) -> *mut u8 {
    let mut guard = shared(class);
    let shared = &mut *guard;
    if let Some(slot) = shared.free.pop().or_else(|| carve(&mut shared.free, class)) {
        return slot;
    }
    if let Some(span) = shared.spans.pop() {
        (shared.free.next, shared.free.end) = span;
    } else {
        new_span(class, &mut shared.free);
    }
    carve(&mut shared.free, class).expect("a span has a slot")
}

fn free_slot(slot: *mut u8, class: usize) {
    super::alloc::count_live(CLASSES[class], false);
    let cached = CACHE.try_with(|cache| {
        // SAFETY: only this thread reaches its cache, and nothing here reenters it.
        let list = unsafe { &mut *cache.0[class].get() };
        list.push(slot);
        if list.len > 2 * BATCH {
            list.move_to(&mut shared(class).free, BATCH);
        }
    });
    if cached.is_err() {
        shared(class).free.push(slot);
    }
}

/// Adds a buffer with one owner holding `used` bytes of at least `capacity`, copied from the given pieces.
fn new_buffer(pieces: &[&[u8]], capacity: usize) -> *mut u8 {
    let wanted = HEADER
        .checked_add(capacity)
        .unwrap_or_else(|| out_of_memory());
    let (base, size) = match class_for(wanted) {
        Some(class) => (alloc_slot(class), CLASSES[class]),
        None => {
            let size = wanted
                .checked_next_multiple_of(PAGE)
                .filter(|size| size >> PAGE_BITS < SLAB as usize)
                .unwrap_or_else(|| out_of_memory());
            let base = alloc_aligned(size, PAGE);
            mark(base.expose_provenance(), size, |page| page as u32 + 1);
            (base, size)
        }
    };
    let data = base.wrapping_add(HEADER);
    let mut used = 0;
    for piece in pieces {
        // SAFETY: `data` has room for every piece and does not overlap them.
        unsafe { std::ptr::copy_nonoverlapping(piece.as_ptr(), data.add(used), piece.len()) };
        used += piece.len();
    }
    // SAFETY: `base` is aligned for a header and has room for one before the text.
    unsafe {
        base.cast::<Header>().write(Header {
            owners: AtomicUsize::new(1),
            used: AtomicUsize::new(used),
            capacity: size - HEADER,
            cause: AtomicPtr::new(std::ptr::null_mut()),
        })
    };
    count_unpinned(1);
    data
}

/// Frees a buffer nobody reads any more, and lets go of its cause when `release_cause` is set.
///
/// # Safety
/// No string may read the buffer afterwards.
unsafe fn free_buffer(buffer: Buffer, release_cause: bool) {
    if buffer.owners.load(Ordering::Relaxed) & PINNED == 0 {
        count_unpinned(-1);
    }
    let cause = buffer.cause.load(Ordering::Acquire);
    let size = HEADER + buffer.capacity;
    if let Some(class) = class_for(size) {
        buffer.owners.store(0, Ordering::Relaxed);
        free_slot(buffer.base(), class);
    } else {
        mark(buffer.base().addr(), size, |_| 0);
        // SAFETY: the buffer came from `alloc_aligned(size, PAGE)`, and nothing reads it now.
        unsafe { free_aligned(buffer.base(), size, PAGE) };
    }
    if !cause.is_null() {
        // SAFETY: the cause was boxed for this buffer alone.
        let cause = unsafe { Box::from_raw(cause) };
        if release_cause {
            release(cause.data, cause.len);
        }
    }
}

/// Where a compiler-produced `string` descriptor is written.
#[repr(C)]
pub struct StringOut {
    pub(super) data: *const u8,
    pub(super) len: i64,
}

impl StringOut {
    pub(super) fn empty() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }

    /// A string that reads `len` bytes of storage that another string already keeps alive.
    pub(super) fn shared(data: *const u8, len: usize) -> Self {
        if len == 0 {
            return Self::empty();
        }
        retain(data, len);
        Self {
            data,
            len: len as i64,
        }
    }

    pub(super) fn built(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::empty();
        }
        Self {
            data: new_buffer(&[bytes], bytes.len()),
            len: bytes.len() as i64,
        }
    }
}

/// Another owner now reads these bytes. Static text and empty text have no buffer.
pub(super) fn retain(data: *const u8, len: usize) {
    if let Some(buffer) = buffer_of(data, len) {
        buffer.owners.fetch_add(1, Ordering::Relaxed);
    }
}

/// One owner stops reading these bytes; the last one frees the buffer.
pub(super) fn release(data: *const u8, len: usize) {
    let Some(buffer) = buffer_of(data, len) else {
        return;
    };
    if buffer.owners.fetch_sub(1, Ordering::Release) & !PINNED != 1 {
        return;
    }
    fence(Ordering::Acquire);
    // SAFETY: that was the last owner.
    unsafe { free_buffer(buffer, true) };
}

/// A package-level value holds this string for the rest of the program.
#[unsafe(no_mangle)]
pub extern "C" fn zore_string_pin(data: *const u8, len: i64) {
    let Some(buffer) = buffer_of(data, usize::try_from(len).unwrap_or(0)) else {
        return;
    };
    if buffer.owners.fetch_or(PINNED, Ordering::Relaxed) & PINNED != 0 {
        return;
    }
    count_unpinned(-1);
    if let Some(cause) = buffer.cause_text() {
        zore_string_pin(cause.data, cause.len as i64);
    }
}

/// New text that remembers the error it wraps; empty text cannot remember one.
pub(super) fn with_cause(text: &[u8], cause: Option<(*const u8, usize)>) -> StringOut {
    let built = StringOut::built(text);
    let Some((cause, cause_len)) = cause.filter(|_| !text.is_empty()) else {
        return built;
    };
    retain(cause, cause_len);
    if let Some(buffer) = buffer_of(built.data, text.len()) {
        let cause = Box::new(Cause {
            data: cause,
            len: cause_len,
            text_len: text.len(),
        });
        buffer.cause.store(Box::into_raw(cause), Ordering::Release);
    }
    built
}

/// The text of the error that this exact text wraps, with a new owner.
pub(super) fn cause_of(data: *const u8, len: usize) -> Option<(*const u8, usize)> {
    let buffer = buffer_of(data, len)?;
    let cause = buffer
        .cause_text()
        .filter(|cause| std::ptr::eq(buffer.text(), data) && cause.text_len == len)?;
    retain(cause.data, cause.len);
    Some((cause.data, cause.len))
}

/// Another owner now reads this string.
#[unsafe(no_mangle)]
pub extern "C" fn zore_string_retain(data: *const u8, len: i64) {
    retain(data, usize::try_from(len).unwrap_or(0));
}

/// One owner stops reading this string.
#[unsafe(no_mangle)]
pub extern "C" fn zore_string_release(data: *const u8, len: i64) {
    release(data, usize::try_from(len).unwrap_or(0));
}

pub(super) fn live_buffers() -> usize {
    let total: isize = UNPINNED_BUFFERS
        .iter()
        .map(|stripe| stripe.0.load(Ordering::Relaxed))
        .sum();
    total.max(0) as usize
}

/// Frees every buffer; only for when no task can read text any more.
pub(super) fn release_all() {
    let mut bases = Vec::new();
    for (root, middle) in PAGES.iter().enumerate() {
        let Some(middle) = child(middle, false) else {
            continue;
        };
        for (index, leaf) in middle.iter().enumerate() {
            let Some(leaf) = child(leaf, false) else {
                continue;
            };
            let first = ((root << MIDDLE_BITS) + index) << LEAF_BITS;
            for (offset, entry) in leaf.iter().enumerate() {
                let entry = entry.load(Ordering::Relaxed);
                let base = (first + offset) << PAGE_BITS;
                if entry == 1 {
                    bases.push(base);
                } else if entry & SLAB != 0 && entry & INDEX_MASK == 0 {
                    let size = CLASSES[slab_class(entry)];
                    bases.extend((0..SPAN / size).map(|slot| base + slot * size));
                }
            }
        }
    }
    for base in bases {
        let header = std::ptr::with_exposed_provenance_mut::<Header>(base);
        let Some(header) = NonNull::new(header) else {
            continue;
        };
        let buffer = Buffer(header);
        if buffer.owners.load(Ordering::Relaxed) != 0 {
            // SAFETY: a slot or large buffer with owners holds a live header, and no task reads text now.
            unsafe { free_buffer(buffer, false) };
        }
    }
}

/// How `left + right` can reuse the buffer that holds `left`.
enum Append {
    /// `left` ends where its buffer's text ends, and there is room.
    InPlace,
    /// `left` ends there, but the buffer is full: build a larger one.
    Grow,
    /// `left` is not at the end of a buffer: build exactly enough.
    Copy,
}

fn plan_append(left: *const u8, left_len: usize, right_len: usize) -> Append {
    let Some(buffer) = buffer_of(left, left_len) else {
        return Append::Copy;
    };
    let Some(end) = (left.addr() + left_len).checked_sub(buffer.text().addr()) else {
        return Append::Copy;
    };
    if buffer.used.load(Ordering::Relaxed) != end {
        return Append::Copy;
    }
    if end + right_len > buffer.capacity {
        return Append::Grow;
    }
    let claimed =
        buffer
            .used
            .compare_exchange(end, end + right_len, Ordering::Relaxed, Ordering::Relaxed);
    if claimed.is_err() {
        return Append::Copy;
    }
    buffer.owners.fetch_add(1, Ordering::Relaxed);
    Append::InPlace
}

/// The two strings joined, with one owner for the caller; an empty operand shares the other's storage.
///
/// # Safety
/// `out` must be writable, and each string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_concat(
    out: *mut StringOut,
    a: *const u8,
    a_len: i64,
    b: *const u8,
    b_len: i64,
) {
    // SAFETY: guaranteed by the caller.
    let (left, right) = unsafe { (super::bytes(a, a_len), super::bytes(b, b_len)) };
    let joined = if right.is_empty() {
        retain(a, left.len());
        StringOut {
            data: a,
            len: a_len,
        }
    } else if left.is_empty() {
        retain(b, right.len());
        StringOut {
            data: b,
            len: b_len,
        }
    } else {
        let total = left.len() + right.len();
        match plan_append(a, left.len(), right.len()) {
            Append::InPlace => {
                // SAFETY: the buffer has room after `left`, which no string reads past.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        right.as_ptr(),
                        (a as *mut u8).add(left.len()),
                        right.len(),
                    )
                };
                StringOut {
                    data: a,
                    len: total as i64,
                }
            }
            Append::Grow => StringOut {
                data: new_buffer(&[left, right], total * 2),
                len: total as i64,
            },
            Append::Copy => StringOut {
                data: new_buffer(&[left, right], total),
                len: total as i64,
            },
        }
    };
    // SAFETY: `out` is writable.
    unsafe { out.write(joined) };
}

/// The one-character string for a scalar value.
///
/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_from_rune(out: *mut StringOut, rune: u32) {
    let character = char::from_u32(rune).unwrap_or(char::REPLACEMENT_CHARACTER);
    let mut buffer = [0; 4];
    let text = character.encode_utf8(&mut buffer);
    // SAFETY: `out` is writable.
    unsafe { out.write(StringOut::built(text.as_bytes())) };
}

/// Whether `position` starts a character or equals the length.
///
/// # Safety
/// The string must satisfy the storage rule of `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_is_boundary(data: *const u8, len: i64, position: i64) -> bool {
    // SAFETY: guaranteed by the caller.
    let text = unsafe { super::bytes(data, len) };
    let Ok(position) = usize::try_from(position) else {
        return false;
    };
    position == text.len() || text.get(position).is_some_and(|byte| byte & 0xC0 != 0x80)
}

pub(super) fn character_at(text: &[u8], position: usize) -> (char, usize) {
    let width = match text.get(position) {
        Some(lead) if lead & 0x80 == 0 => 1,
        Some(lead) if lead & 0xE0 == 0xC0 => 2,
        Some(lead) if lead & 0xF0 == 0xE0 => 3,
        Some(_) => 4,
        None => return (char::REPLACEMENT_CHARACTER, 1),
    };
    text.get(position..position + width)
        .and_then(|piece| std::str::from_utf8(piece).ok())
        .and_then(|piece| piece.chars().next())
        .map_or((char::REPLACEMENT_CHARACTER, 1), |character| {
            (character, width)
        })
}

/// The scalar value that starts at `position`.
///
/// # Safety
/// The string must satisfy the storage rule of `bytes`, and `position` must be a character start.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_rune_at(data: *const u8, len: i64, position: i64) -> u32 {
    // SAFETY: guaranteed by the caller.
    let text = unsafe { super::bytes(data, len) };
    character_at(text, usize::try_from(position).unwrap_or(usize::MAX)).0 as u32
}

/// The byte length of the character that starts at `position`.
///
/// # Safety
/// The string must satisfy the storage rule of `bytes`, and `position` must be a character start.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_char_width(data: *const u8, len: i64, position: i64) -> i64 {
    // SAFETY: guaranteed by the caller.
    let text = unsafe { super::bytes(data, len) };
    character_at(text, usize::try_from(position).unwrap_or(usize::MAX)).1 as i64
}

/// Compares two compiler-produced strings lexicographically by UTF-8 bytes.
///
/// # Safety
/// Each pointer must address its corresponding number of live, initialized
/// bytes, or have length zero. Both allocations must remain immutable here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_string_compare(
    a: *const u8,
    a_len: i64,
    b: *const u8,
    b_len: i64,
) -> i32 {
    // SAFETY: guaranteed by the generated call's string ABI.
    let (a, b) = unsafe { (super::bytes(a, a_len), super::bytes(b, b_len)) };
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_null_strings_and_embedded_nuls_compare_by_bytes() {
        let _serial = crate::string::serial();
        // SAFETY: empty strings need no allocation; byte literals stay live.
        unsafe {
            assert_eq!(
                zore_string_compare(std::ptr::null(), 0, std::ptr::null(), 0),
                0
            );
            assert_eq!(
                zore_string_compare(std::ptr::null(), 0, b"a".as_ptr(), 1),
                -1
            );
            assert_eq!(
                zore_string_compare(b"a\0b".as_ptr(), 3, b"a\0c".as_ptr(), 3),
                -1
            );
            assert_eq!(zore_string_compare(b"ab".as_ptr(), 2, b"a".as_ptr(), 1), 1);
        }
    }

    unsafe fn joined(a: &str, b: &str) -> String {
        let mut out = StringOut::empty();
        // SAFETY: both strings are live and `out` is writable.
        unsafe {
            zore_string_concat(
                &mut out,
                a.as_ptr(),
                a.len() as i64,
                b.as_ptr(),
                b.len() as i64,
            );
            String::from_utf8(super::super::bytes(out.data, out.len).to_vec()).unwrap()
        }
    }

    #[test]
    fn concatenation_builds_new_text_and_shares_for_empty_operands() {
        let _serial = crate::string::serial();
        // SAFETY: the operands are live string literals.
        unsafe {
            assert_eq!(joined("ab", "cd"), "abcd");
            assert_eq!(joined("", "x"), "x");
            assert_eq!(joined("x", ""), "x");
            assert_eq!(joined("", ""), "");
        }
        release_all();
    }

    #[test]
    fn runes_encode_and_decode_across_widths() {
        let _serial = crate::string::serial();
        for (text, rune, width) in [
            ("a", 'a', 1),
            ("é", 'é', 2),
            ("用", '用', 3),
            ("🦀", '🦀', 4),
        ] {
            let mut out = StringOut::empty();
            // SAFETY: `out` is writable and the text is a live literal.
            unsafe {
                zore_string_from_rune(&mut out, rune as u32);
                assert_eq!(super::super::bytes(out.data, out.len), text.as_bytes());
                assert_eq!(zore_string_rune_at(text.as_ptr(), width, 0), rune as u32);
                assert_eq!(zore_string_char_width(text.as_ptr(), width, 0), width);
            }
        }
        release_all();
    }

    #[test]
    fn boundaries_are_character_starts_and_the_end() {
        let _serial = crate::string::serial();
        let text = "aé";
        // SAFETY: the text is a live literal.
        unsafe {
            let check = |position| zore_string_is_boundary(text.as_ptr(), 3, position);
            assert!(check(0) && check(1) && !check(2) && check(3));
            assert!(!check(4) && !check(-1));
        }
    }

    /// Freed text has no buffer, or lies in a slot that waits for reuse.
    fn freed(data: *const u8, len: usize) -> bool {
        buffer_of(data, len).is_none_or(|buffer| buffer.owners.load(Ordering::Relaxed) == 0)
    }

    fn buffer_count() -> usize {
        live_buffers()
    }

    fn append(left: &StringOut, right: &str) -> StringOut {
        let mut out = StringOut::empty();
        // SAFETY: `left` is live, `right` is a literal, and `out` is writable.
        unsafe {
            zore_string_concat(
                &mut out,
                left.data,
                left.len,
                right.as_ptr(),
                right.len() as i64,
            )
        };
        out
    }

    fn read(text: &StringOut) -> String {
        // SAFETY: the text is live until the buffers are released.
        unsafe { String::from_utf8(super::super::bytes(text.data, text.len).to_vec()).unwrap() }
    }

    #[test]
    fn appending_at_the_end_reuses_the_buffer_and_leaves_older_text_alone() {
        let _serial = crate::string::serial();
        release_all();
        let ab = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let abc = append(&ab, "c");
        let abcd = append(&abc, "d");
        assert_eq!(abcd.data, abc.data, "the tail grows in place");
        let abx = append(&ab, "x");
        let abcy = append(&abc, "y");
        assert_ne!(abx.data, ab.data);
        assert_ne!(
            abcy.data, abc.data,
            "a text that is not at the end is copied"
        );
        assert_eq!(
            [read(&ab), read(&abc), read(&abcd), read(&abx), read(&abcy)],
            ["ab", "abc", "abcd", "abx", "abcy"]
        );
        release_all();
    }

    #[test]
    fn a_long_chain_of_appends_uses_few_buffers() {
        let _serial = crate::string::serial();
        release_all();
        let mut text = StringOut::shared(b"x".as_ptr(), 1);
        let appends = if cfg!(miri) { 300 } else { 10_000 };
        for _ in 0..appends {
            text = append(&text, "y");
        }
        assert_eq!(text.len, appends + 1);
        assert!(buffer_count() <= 16, "{} buffers", buffer_count());
        release_all();
    }

    #[test]
    fn appending_a_text_to_itself_and_to_a_suffix_is_correct() {
        let _serial = crate::string::serial();
        release_all();
        let ab = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let abab = append(&ab, "ab");
        let doubled = {
            let mut out = StringOut::empty();
            // SAFETY: both operands are live and `out` is writable.
            unsafe { zore_string_concat(&mut out, abab.data, abab.len, abab.data, abab.len) };
            out
        };
        assert_eq!(read(&doubled), "abababab");
        let tail = StringOut::shared(unsafe { doubled.data.add(4) }, 4);
        assert_eq!(read(&append(&tail, "!")), "abab!");
        assert_eq!(read(&doubled), "abababab");
        release_all();
    }

    #[test]
    fn the_last_owner_frees_a_buffer_and_static_or_empty_text_has_none() {
        let _serial = crate::string::serial();
        let built = StringOut::built(b"hello");
        assert_eq!(live_buffers(), 1);
        let piece = StringOut::shared(unsafe { built.data.add(1) }, 3);
        retain(built.data, 5);
        release(built.data, 5);
        release(built.data, 5);
        assert_eq!(live_buffers(), 1, "the shared piece still owns the buffer");
        assert_eq!(read(&piece), "ell");
        release(piece.data, 3);
        assert_eq!(live_buffers(), 0);
        retain(b"static".as_ptr(), 6);
        release(b"static".as_ptr(), 6);
        retain(std::ptr::null(), 0);
        release(std::ptr::null(), 0);
        assert_eq!(live_buffers(), 0);
    }

    #[test]
    fn every_concatenation_result_has_its_own_owner() {
        let _serial = crate::string::serial();
        let ab = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let abc = append(&ab, "c");
        let same = append(&abc, "");
        assert_eq!(same.data, abc.data);
        release(ab.data, 2);
        release(abc.data, 3);
        assert_eq!(live_buffers(), 1, "the unchanged text still owns it");
        release(same.data, 3);
        assert_eq!(live_buffers(), 0);
    }

    #[test]
    fn a_text_that_ends_a_buffer_is_still_readable_after_the_older_owners_leave() {
        let _serial = crate::string::serial();
        let ab = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let abcd = append(&append(&ab, "c"), "d");
        release(ab.data, 2);
        assert_eq!(read(&abcd), "abcd");
        assert!(live_buffers() >= 1);
        release_all();
    }

    fn piece_at(text: &StringOut, start: usize, len: usize) -> StringOut {
        StringOut::shared(text.data.wrapping_add(start), len)
    }

    #[test]
    fn a_buffer_shared_by_threads_is_freed_once_by_its_last_owner() {
        let _serial = crate::string::serial();
        release_all();
        let text = StringOut::built(b"shared between threads");
        let (address, len) = (text.data.addr(), text.len as usize);
        std::thread::scope(|scope| {
            for _ in 0..if cfg!(miri) { 2 } else { 4 } {
                scope.spawn(move || {
                    let data = std::ptr::with_exposed_provenance::<u8>(address);
                    for _ in 0..if cfg!(miri) { 100 } else { 10_000 } {
                        retain(data, len);
                        retain(data.wrapping_add(7), 7);
                        release(data, len);
                        release(data.wrapping_add(7), 7);
                    }
                });
            }
        });
        assert_eq!(live_buffers(), 1);
        assert_eq!(read(&text), "shared between threads");
        release(text.data, len);
        assert_eq!(live_buffers(), 0);
    }

    #[test]
    fn racing_appends_to_one_text_each_build_their_own_result() {
        let _serial = crate::string::serial();
        release_all();
        let base = append(&StringOut::shared(b"a".as_ptr(), 1), "b");
        let (address, len) = (base.data.addr(), base.len);
        let results: Vec<(usize, i64)> = std::thread::scope(|scope| {
            let handles: Vec<_> = ["c", "d", "e", "f", "g", "h"]
                .into_iter()
                .map(|tail| {
                    scope.spawn(move || {
                        let left = StringOut {
                            data: std::ptr::with_exposed_provenance(address),
                            len,
                        };
                        let joined = append(&left, tail);
                        (joined.data.expose_provenance(), joined.len)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        let texts: Vec<String> = results
            .iter()
            .map(|&(data, len)| {
                read(&StringOut {
                    data: std::ptr::with_exposed_provenance(data),
                    len,
                })
            })
            .collect();
        assert_eq!(texts, ["abc", "abd", "abe", "abf", "abg", "abh"]);
        let in_place = results.iter().filter(|&&(data, _)| data == address).count();
        assert_eq!(
            in_place, 1,
            "exactly one append claims the room after the text"
        );
        assert_eq!(read(&base), "ab");
        for (data, len) in results {
            release(std::ptr::with_exposed_provenance(data), len as usize);
        }
        release(base.data, 2);
        assert_eq!(live_buffers(), 0);
    }

    #[test]
    fn a_piece_far_inside_a_large_buffer_keeps_it_alive() {
        let _serial = crate::string::serial();
        release_all();
        let large = vec![b'x'; if cfg!(miri) { 3 << 16 } else { 3 << 20 }];
        let text = StringOut::built(&large);
        let piece = piece_at(&text, large.len() * 5 / 6 + 3, 4);
        let start = piece_at(&text, 0, 2);
        release(text.data, large.len());
        assert_eq!(live_buffers(), 1);
        assert_eq!(read(&piece), "xxxx");
        release(start.data, 2);
        assert_eq!(live_buffers(), 1);
        release(piece.data, 4);
        assert_eq!(live_buffers(), 0);
        assert!(buffer_of(piece.data, 4).is_none());
    }

    #[test]
    fn unreleased_text_counts_as_live_unless_pinned() {
        let _serial = crate::string::serial();
        release_all();
        let kept = StringOut::built(b"kept");
        let pinned = with_cause(b"wrapped", Some((kept.data, 4)));
        zore_string_pin(pinned.data, pinned.len);
        assert_eq!(live_buffers(), 0, "pinning also pins the cause");
        let leaked = StringOut::built(b"leaked");
        assert_eq!(live_buffers(), 1);
        release_all();
        assert_eq!(live_buffers(), 0);
        assert!(freed(leaked.data, 6));
    }

    #[test]
    fn a_wrapped_error_lets_go_of_its_cause_when_freed() {
        let _serial = crate::string::serial();
        release_all();
        let cause = StringOut::built(b"cause");
        let wrapped = with_cause(b"wrapped: cause", Some((cause.data, 5)));
        release(cause.data, 5);
        assert_eq!(live_buffers(), 2);
        let unwrapped = cause_of(wrapped.data, 14).expect("the exact text knows its cause");
        assert_eq!(unwrapped, (cause.data, 5));
        assert!(cause_of(wrapped.data, 7).is_none());
        release(unwrapped.0, unwrapped.1);
        release(wrapped.data, 14);
        assert_eq!(live_buffers(), 0);
    }

    #[test]
    fn every_small_size_gets_the_smallest_class_and_slot_division_is_exact() {
        let mut previous = 0;
        for (class, &size) in CLASSES.iter().enumerate() {
            assert_eq!(size % 16, 0);
            for wanted in previous + 1..=size {
                assert_eq!(class_for(wanted), Some(class), "{wanted} bytes");
            }
            previous = size;
            let step = if cfg!(miri) { 4093 } else { 1 };
            for offset in (0..SPAN).step_by(step) {
                assert_eq!(
                    ((offset as u64 * RECIPROCALS[class]) >> RECIPROCAL_SHIFT) as usize,
                    offset / size
                );
            }
        }
        assert_eq!(class_for(LARGEST_SLOT + 1), None);
    }

    #[test]
    fn every_byte_of_a_slot_finds_its_own_header() {
        let _serial = crate::string::serial();
        release_all();
        let texts: Vec<StringOut> = (0..40u8).map(|i| StringOut::built(&[i; 70])).collect();
        for text in &texts {
            let buffer = buffer_of(text.data, 70).expect("built text has a buffer");
            assert_eq!(buffer.text().cast_const(), text.data);
            assert!(page_entry(buffer.base().addr()) & SLAB != 0);
            for offset in 0..70 {
                let inside = buffer_of(text.data.wrapping_add(offset), 1).expect("inside the slot");
                assert_eq!(inside.base(), buffer.base());
            }
        }
        let pieces: Vec<StringOut> = texts.iter().map(|text| piece_at(text, 69, 1)).collect();
        for text in &texts {
            release(text.data, 70);
        }
        assert_eq!(live_buffers(), 40);
        for (i, piece) in pieces.iter().enumerate() {
            assert_eq!(read(piece).as_bytes(), [i as u8]);
            release(piece.data, 1);
        }
        assert_eq!(live_buffers(), 0);
        assert!(texts.iter().all(|text| freed(text.data, 70)));
    }

    #[test]
    fn a_large_buffer_covers_whole_pages_and_clears_them_when_freed() {
        let _serial = crate::string::serial();
        release_all();
        let large = vec![b'y'; 10 * PAGE];
        let text = StringOut::built(&large);
        let buffer = buffer_of(text.data, large.len()).expect("built text has a buffer");
        assert_eq!(buffer.base().addr() % PAGE, 0);
        assert_eq!((HEADER + buffer.capacity) % PAGE, 0);
        for page in 0..=10 {
            let inside = text.data.wrapping_add((page * PAGE).min(large.len() - 1));
            assert_eq!(buffer_of(inside, 1).expect("inside").base(), buffer.base());
        }
        release(text.data, large.len());
        assert_eq!(live_buffers(), 0);
        for page in 0..=10 {
            assert_eq!(page_entry(buffer.base().addr() + page * PAGE), 0);
        }
    }

    #[test]
    fn slots_freed_on_another_thread_are_reused_and_counted() {
        let _serial = crate::string::serial();
        release_all();
        let count = if cfg!(miri) { 3 * BATCH } else { 8 * BATCH };
        let bytes_before = crate::alloc::live_bytes();
        let addresses: Vec<usize> = std::thread::spawn(move || {
            (0..count)
                .map(|i| {
                    StringOut::built(format!("slot {i:04}").as_bytes())
                        .data
                        .expose_provenance()
                })
                .collect()
        })
        .join()
        .unwrap();
        assert_eq!(live_buffers(), count);
        assert_eq!(
            crate::alloc::live_bytes() - bytes_before,
            count * CLASSES[0]
        );
        let sent = addresses.clone();
        std::thread::spawn(move || {
            for data in sent {
                release(std::ptr::with_exposed_provenance(data), 9);
            }
        })
        .join()
        .unwrap();
        assert_eq!(live_buffers(), 0);
        assert_eq!(crate::alloc::live_bytes(), bytes_before);
        let freed: std::collections::HashSet<usize> = addresses.into_iter().collect();
        let again: Vec<usize> = std::thread::spawn(|| {
            (0..BATCH)
                .map(|i| {
                    let text = StringOut::built(format!("again {i:02}").as_bytes());
                    assert_eq!(read(&text), format!("again {i:02}"));
                    text.data.expose_provenance()
                })
                .collect()
        })
        .join()
        .unwrap();
        let reused = again.iter().filter(|data| freed.contains(data)).count();
        assert_eq!(
            reused, BATCH,
            "a new thread takes the slots the last thread gave back"
        );
        for data in again {
            release(std::ptr::with_exposed_provenance(data), 8);
        }
        assert_eq!(live_buffers(), 0);
        assert_eq!(crate::alloc::live_bytes(), bytes_before);
    }

    #[test]
    fn text_built_and_freed_after_the_thread_cache_is_gone_takes_the_lock_once() {
        static RAN_AFTER_CACHE: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        struct AfterCache;
        impl Drop for AfterCache {
            fn drop(&mut self) {
                RAN_AFTER_CACHE.store(CACHE.try_with(|_| ()).is_err(), Ordering::Relaxed);
                let large = "x".repeat(CLASSES[CLASSES.len() - 1] - HEADER);
                let texts: Vec<StringOut> = (0..3 * SPAN / CLASSES[CLASSES.len() - 1])
                    .map(|_| StringOut::built(large.as_bytes()))
                    .chain(
                        (0..3 * BATCH).map(|i| StringOut::built(format!("late {i:03}").as_bytes())),
                    )
                    .collect();
                for text in texts {
                    release(text.data, text.len as usize);
                }
            }
        }
        thread_local! {
            static AFTER_CACHE: AfterCache = const { AfterCache };
        }
        let _serial = crate::string::serial();
        release_all();
        let bytes_before = crate::alloc::live_bytes();
        std::thread::spawn(|| {
            AFTER_CACHE.with(|_| ());
            release(StringOut::built(b"cached").data, 6);
        })
        .join()
        .unwrap();
        // Thread-local destructor order is only promised to be last-in, first-out on Linux.
        if cfg!(target_os = "linux") {
            assert!(RAN_AFTER_CACHE.load(Ordering::Relaxed));
        }
        assert_eq!(live_buffers(), 0);
        assert_eq!(crate::alloc::live_bytes(), bytes_before);
    }

    #[test]
    fn appending_fills_the_slot_and_then_moves_to_a_larger_class() {
        let _serial = crate::string::serial();
        release_all();
        let mut text = StringOut::built(b"ab");
        let first = buffer_of(text.data, 2).unwrap();
        assert_eq!(HEADER + first.capacity, CLASSES[0]);
        for _ in 0..7 {
            let next = append(&text, "cd");
            release(text.data, text.len as usize);
            text = next;
        }
        assert_eq!(text.len, 16);
        assert_eq!(buffer_of(text.data, 16).unwrap().base(), first.base());
        let grown = append(&text, "e");
        assert_ne!(buffer_of(grown.data, 17).unwrap().base(), first.base());
        assert_eq!(read(&grown), "abcdcdcdcdcdcdcde");
        release(text.data, 16);
        release(grown.data, 17);
        assert_eq!(live_buffers(), 0);
    }
}
