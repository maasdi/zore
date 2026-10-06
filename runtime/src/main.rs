#[path = "lib.rs"]
mod runtime;

unsafe extern "C" {
    fn zore_entry();
}

fn main() {
    // Rust startup ignores SIGPIPE, so failed output reaches write-error handling.
    // SAFETY: the compiler supplies this no-argument, no-result entry point.
    unsafe { zore_entry() };
    runtime::finish();
}
