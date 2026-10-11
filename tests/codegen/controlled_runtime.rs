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

fn rustc() -> String {
    std::env::var("ZORE_RUSTC").unwrap_or_else(|_| "rustc".into())
}

fn checked(command: &mut Command) {
    let output = command.output().expect("run native test tool");
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
            Command::new(rustc())
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
    assert!(source.contains("waitPending(") || source.contains("waitBlocked("));
    let source = format!("{source}\nfunc waitPending(n int) {{}}\nfunc waitBlocked(n int) {{}}\n");
    let mut sources = SourceMap::new();
    let file = sources.add("test.ore", source).unwrap();
    let mut llvm = emit_llvm(sources.file(file).unwrap()).expect("check controlled test program");
    for (name, hook) in [
        ("waitPending", "zore_test_wait_pending"),
        ("waitBlocked", "zore_test_wait_blocked"),
    ] {
        let symbol = format!("define void @\"main.{name}\"(");
        let start = llvm.find(&symbol).expect("checkpoint function emitted");
        let end = start + llvm[start..].find("\n}").expect("function end") + 2;
        llvm.replace_range(
            start..end,
            &format!("define void @\"main.{name}\"(i64 %n) {{\nentry:\n  call void @{hook}(i64 %n)\n  ret void\n}}"),
        );
        llvm.push_str(&format!("\ndeclare void @{hook}(i64)\n"));
    }
    let dir = TempDir::new().unwrap();
    let ir = dir.path().join("program.ll");
    let object = dir.path().join("program.o");
    fs::write(&ir, llvm).unwrap();
    checked(
        Command::new(compiler())
            .args(["-Wno-override-module", "-O2", "-fPIC", "-c"])
            .arg(&ir)
            .arg("-o")
            .arg(&object),
    );
    let entry = dir.path().join("main.rs");
    fs::write(
        &entry,
        r#"
unsafe extern "C" { fn zore_entry(); }
#[unsafe(no_mangle)]
extern "C" fn zore_test_wait_pending(count: i64) {
    zore_runtime::scheduler::test_support::wait_pending(usize::try_from(count).unwrap());
}
#[unsafe(no_mangle)]
extern "C" fn zore_test_wait_blocked(count: i64) {
    zore_runtime::scheduler::test_support::wait_blocked(usize::try_from(count).unwrap());
}
fn main() {
    zore_runtime::scheduler::test_support::install_pool(2);
    unsafe { zore_entry() };
    zore_runtime::finish();
}
"#,
    )
    .unwrap();
    checked(
        Command::new(rustc())
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
