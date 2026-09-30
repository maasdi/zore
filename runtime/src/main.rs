//! Native process entry, compiled with a generated Zore object by rustc.

#[path = "lib.rs"]
mod runtime;

unsafe extern "C" {
    fn zore_entry();
}

fn main() {
    // Rust startup ignores SIGPIPE on supported Unix hosts. Failed output
    // therefore reaches the runtime's write-error handling.
    // SAFETY: the compiler supplies this no-argument, no-result entry point.
    unsafe { zore_entry() };
    runtime::finish();
}
