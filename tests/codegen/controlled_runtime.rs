use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use zore::driver::build::{TempDir, compiler, emit_llvm};
use zore::source::SourceMap;

struct Runtime {
    _dir: TempDir,
    library: PathBuf,
}

fn rustc() -> Command {
    let mut command = Command::new(std::env::var("ZORE_RUSTC").unwrap_or_else(|_| "rustc".into()));
    if std::env::var_os("ZORE_HARDENING_ASAN").is_some() {
        command.arg("-Zsanitizer=address");
    }
    command
}

fn checked(command: &mut Command) {
    let output = super::bounded_process::run(command, std::time::Duration::from_secs(120))
        .expect("run bounded native test tool");
    assert!(
        output.status.success(),
        "{command:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        let dir = TempDir::new().unwrap();
        let library = dir.path().join("libzore_runtime.rlib");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../runtime/src/lib.rs");
        checked(
            rustc()
                .args([
                    "--edition=2024",
                    "--crate-name=zore_runtime",
                    "--crate-type=rlib",
                    "--cfg=zore_test_hooks",
                    "-Copt-level=2",
                    "-Cpanic=abort",
                ])
                .arg(source)
                .arg("-o")
                .arg(&library),
        );
        Runtime { _dir: dir, library }
    })
}

pub(super) fn build(source: &str, output: &Path) {
    build_with_workers(source, output, 2);
}

pub(super) fn build_with_workers(source: &str, output: &Path, workers: usize) {
    build_with_optimization(source, output, workers, "-O2");
}

pub(super) fn build_with_optimization(
    source: &str,
    output: &Path,
    workers: usize,
    optimization: &str,
) {
    assert!(
        source.contains("waitPending(")
            || source.contains("waitBlocked(")
            || source.contains("waitIdle(")
    );
    let timer_hook = source.contains("expireTimers(");
    let mut source = format!(
        "{source}\nfunc waitPending(n int) {{}}\nfunc waitBlocked(n int) {{}}\nfunc waitIdle(n int) {{}}\n"
    );
    if timer_hook {
        source.push_str("func expireTimers(n int) {}\n");
    }
    let check_timers = source.contains("checkTimers(");
    if check_timers {
        source.push_str("func checkTimers(n int) {}\n");
    }
    let allocations = source.contains("allocatedBytes(");
    if allocations {
        source.push_str("func allocatedBytes() int { return 0 }\n");
    }
    let mut sources = SourceMap::new();
    let file = sources.add("test.ore", source).unwrap();
    let mut llvm = emit_llvm(sources.file(file).unwrap()).expect("check controlled test program");
    let mut hooks = vec![
        ("waitPending", "zore_test_wait_pending"),
        ("waitBlocked", "zore_test_wait_blocked"),
        ("waitIdle", "zore_test_wait_idle"),
    ];
    if timer_hook {
        hooks.push(("expireTimers", "zore_test_expire_timers"));
    }
    if check_timers {
        hooks.push(("checkTimers", "zore_test_check_timers"));
    }
    for (name, hook) in hooks {
        let symbol = format!("define internal void @\"main.{name}\"(");
        let start = llvm.find(&symbol).expect("checkpoint function emitted");
        let end = start + llvm[start..].find("\n}").expect("function end") + 2;
        llvm.replace_range(
            start..end,
            &format!("define internal void @\"main.{name}\"(i64 %n) {{\nentry:\n  call void @{hook}(i64 %n)\n  ret void\n}}"),
        );
        llvm.push_str(&format!("\ndeclare void @{hook}(i64)\n"));
    }
    if allocations {
        let symbol = "define internal i64 @\"main.allocatedBytes\"(";
        let start = llvm.find(symbol).expect("allocation checkpoint emitted");
        let end = start + llvm[start..].find("\n}").expect("function end") + 2;
        llvm.replace_range(
            start..end,
            "define internal i64 @\"main.allocatedBytes\"() {\nentry:\n  %bytes = call i64 @zore_test_allocated_bytes()\n  ret i64 %bytes\n}",
        );
        llvm.push_str("\ndeclare i64 @zore_test_allocated_bytes()\n");
    }
    let dir = TempDir::new().unwrap();
    let ir = dir.path().join("program.ll");
    let object = dir.path().join("program.o");
    fs::write(&ir, llvm).unwrap();
    let mut compile = Command::new(compiler());
    if std::env::var_os("ZORE_HARDENING_ASAN").is_some() {
        compile.arg("-fsanitize=address");
    }
    if !cfg!(windows) {
        compile.arg("-fPIC");
    }
    checked(
        compile
            .args(["-Wno-override-module", optimization, "-c"])
            .arg(&ir)
            .arg("-o")
            .arg(&object),
    );
    let entry = dir.path().join("main.rs");
    let entry_source = r#"
unsafe extern "C" { fn zore_entry(); }
#[unsafe(no_mangle)]
extern "C" fn zore_test_wait_pending(count: i64) {
    zore_runtime::scheduler::test_support::wait_pending(usize::try_from(count).unwrap());
}
#[unsafe(no_mangle)]
extern "C" fn zore_test_wait_blocked(count: i64) {
    zore_runtime::scheduler::test_support::wait_blocked(usize::try_from(count).unwrap());
}
#[unsafe(no_mangle)]
extern "C" fn zore_test_wait_idle(count: i64) {
    zore_runtime::scheduler::test_support::wait_idle(usize::try_from(count).unwrap());
}
#[unsafe(no_mangle)]
extern "C" fn zore_test_allocated_bytes() -> i64 {
    i64::try_from(zore_runtime::scheduler::test_support::allocated_bytes()).unwrap()
}
#[unsafe(no_mangle)]
extern "C" fn zore_test_expire_timers(count: i64) {
    zore_runtime::scheduler::test_support::expire_timers(usize::try_from(count).unwrap());
}
#[unsafe(no_mangle)]
extern "C" fn zore_test_check_timers(count: i64) {
    zore_runtime::scheduler::test_support::check_timers(usize::try_from(count).unwrap());
}
fn main() {
    zore_runtime::scheduler::test_support::install_pool(2);
    unsafe { zore_entry() };
    zore_runtime::finish();
}
"#;
    fs::write(
        &entry,
        entry_source.replace("install_pool(2)", &format!("install_pool({workers})")),
    )
    .unwrap();
    checked(
        rustc()
            .args([
                "--edition=2024",
                "-Copt-level=2",
                "-Cpanic=abort",
                "--extern",
            ])
            .arg(format!("zore_runtime={}", runtime().library.display()))
            .arg(format!("-Clink-arg={}", object.display()))
            .arg(&entry)
            .arg("-o")
            .arg(output),
    );
}
