use std::sync::atomic::{AtomicUsize, Ordering};

static LIVE_RESOURCES: AtomicUsize = AtomicUsize::new(0);

#[cfg(zore_native_adapters)]
pub(super) fn resource_created() {
    LIVE_RESOURCES.fetch_add(1, Ordering::Relaxed);
}

#[cfg(zore_native_adapters)]
pub(super) fn resource_released() {
    LIVE_RESOURCES.fetch_sub(1, Ordering::Relaxed);
}

pub(super) fn live_resources() -> usize {
    LIVE_RESOURCES.load(Ordering::Relaxed)
}
