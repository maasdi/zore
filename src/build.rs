//! Native builds: check, lower to MIR, emit LLVM IR, and compile it with clang
//! together with the C runtime (decision record 0001).

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::check::check_file;
use crate::codegen;
use crate::diagnostic::Diagnostic;
use crate::lower::lower;
use crate::source::SourceFile;

const RUNTIME_SOURCE: &str = include_str!("../runtime/zore_runtime.c");

#[derive(Debug)]
pub enum BuildError {
    /// The program was rejected; render these against the source map.
    Diagnostics(Vec<Diagnostic>),
    /// The package has no §3.19 entry point to build.
    NotExecutable(String),
    /// The external toolchain was missing or failed.
    Toolchain(String),
    Io(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostics(d) => write!(f, "{} error(s) in the program", d.len()),
            Self::NotExecutable(message) | Self::Toolchain(message) | Self::Io(message) => {
                f.write_str(message)
            }
        }
    }
}

/// Check and lower a file, returning its LLVM IR.
pub fn emit_llvm(file: &SourceFile) -> Result<String, BuildError> {
    let checked = check_file(file);
    if !checked.diagnostics.is_empty() {
        return Err(BuildError::Diagnostics(checked.diagnostics));
    }
    let package = checked
        .package
        .expect("no diagnostics means a checked package");
    if package.entry.is_none() {
        return Err(BuildError::NotExecutable(format!(
            "package `{}` is not executable; only a `main` package with `func main()` can be built",
            package.name
        )));
    }
    let program = lower(&package);
    codegen::emit(&package, &program, file).map_err(BuildError::Diagnostics)
}

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// A private temporary directory removed when dropped.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Result<Self, BuildError> {
        let path = std::env::temp_dir().join(format!(
            "zore-build-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path)
            .map_err(|e| BuildError::Io(format!("{}: {e}", path.display())))?;
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The C compiler: `ZORE_CC` if set, otherwise `clang` on `PATH`.
pub fn compiler() -> String {
    std::env::var("ZORE_CC").unwrap_or_else(|_| "clang".into())
}

/// Build an executable at `output`.
pub fn build(file: &SourceFile, output: &Path) -> Result<(), BuildError> {
    let ir = emit_llvm(file)?;
    let dir = TempDir::new()?;
    let ir_path = dir.path().join("program.ll");
    let runtime_path = dir.path().join("zore_runtime.c");
    for (path, contents) in [(&ir_path, ir.as_str()), (&runtime_path, RUNTIME_SOURCE)] {
        fs::write(path, contents)
            .map_err(|e| BuildError::Io(format!("{}: {e}", path.display())))?;
    }
    let cc = compiler();
    let result = Command::new(&cc)
        .arg("-O2")
        .arg("-Wno-override-module")
        .arg(&ir_path)
        .arg(&runtime_path)
        .arg("-o")
        .arg(output)
        .output();
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            return Err(BuildError::Toolchain(format!(
                "could not run `{cc}` ({error}); `zore build` requires clang with LLVM 15 or \
                 newer (set ZORE_CC to choose a compiler)"
            )));
        }
    };
    if !result.status.success() {
        return Err(BuildError::Toolchain(format!(
            "`{cc}` failed to compile the generated program; this is a compiler bug:\n{}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}
