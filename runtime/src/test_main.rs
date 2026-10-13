unsafe extern "C" {
    fn zore_entry() -> i64;
}

fn main() {
    // SAFETY: the compiler supplies the test dispatcher's integer result.
    let status = unsafe { zore_entry() };
    zore_runtime::finish();
    std::process::exit(status as i32);
}
