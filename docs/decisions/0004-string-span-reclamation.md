# 0004 — Reclaim physical pages of empty string spans

Issue: #241. Status: implemented; validation evidence is recorded in the PR and
the logging benchmark notes. This is an internal allocator change; no public
API, ownership, UTF-8, error or suspension contract changes.

Small strings still use size-class slots in reserved 256 KiB spans. Retain and
release still locate headers through the page table without a lock. Allocation
and final release normally use thread-local free-slot caches, with batches of
64 moving through a shared allocator per class.

Free-slot addresses now live in vectors outside the spans, so discarding pages
cannot erase allocator links. A per-class map counts reserved slots per span:
live slots, cached free slots and a thread's uncarved remainder all count as
reserved. Only transfers under the shared lock update these counts. A span with
zero reservations has no live string and no thread cache that can access it.
At most one such committed span per class stays warm; subsequent empty spans
are discarded. A partially used span and slots retained in a sleeping thread's
bounded cache cannot be reclaimed by this policy.

Linux/macOS allocate spans as anonymous mappings. Linux discards unused pages
with `MADV_DONTNEED`; macOS marks them `MADV_FREE_REUSABLE` and notifies the
kernel with `MADV_FREE_REUSE` before reuse. Windows reserves and commits with `VirtualAlloc`, decommits
with `VirtualFree(MEM_DECOMMIT)`, and recommits before reuse. Address ranges and
page-table entries stay reserved for process lifetime. Resident-memory effects
of the Unix advice depend on the host. Unsupported hosts retain their previous
keep-for-reuse allocation policy. Miri uses allocated storage with 32 KiB spans
and smaller fixture counts to bound interpreter bookkeeping; it zeroes free
spans to model discard without invoking unsupported OS memory APIs. Native
tests retain the full 256 KiB spans and workload sizes.

The shared lock serializes discard and recommit with taking slots or remainders.
Page-table entries mark discarded spans so teardown skips inaccessible headers,
including on Windows where decommitted pages cannot be read. Recommit precedes
clearing the marker and handing a slot to a cache. Ordinary live strings keep a
reservation until their final release; no concurrent valid retain/lookup can
observe its own span being discarded. Retaining a pointer after its last owner
is released remains invalid.

Large-buffer allocation/free, string causes, pinned values, live-buffer counts
and `allocatedBytes` accounting retain their existing behavior. Reserved virtual
space, free-slot metadata and page-table nodes remain allocated; this change
returns physical string pages, not all allocator metadata or virtual addresses.

Tests cover discard/recommit over repeated cycles, live lookup/retain/release
racing with discard of other spans, thread exit and `release_all` after discard.
The manual million-short-string resident-memory measurement runs with
`cargo test --locked -p zore-runtime million_short_strings_return_resident_pages
-- --ignored --nocapture`; it asserts a substantial RSS decrease on Linux and
Windows and records macOS RSS without requiring immediate advisory reclamation.
Native supported-host tests exercise actual OS primitives; Miri checks the
allocator logic and pointer provenance using its simulated discard path.
