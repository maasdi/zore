use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::codegen;
use crate::diagnostic::Diagnostic;
use crate::driver::check::check_project;
use crate::driver::project::{Project, load_file};
use crate::dropck;
use crate::mir::lower::lower;
use crate::source::{Layered, SourceFile, Sources};

const RUNTIME_SOURCES: &[(&str, &str)] = &[
    ("main.rs", include_str!("../../../runtime/src/main.rs")),
    ("lib.rs", include_str!("../../../runtime/src/lib.rs")),
    ("alloc.rs", include_str!("../../../runtime/src/alloc.rs")),
    (
        "channel.rs",
        include_str!("../../../runtime/src/channel.rs"),
    ),
    ("fiber.rs", include_str!("../../../runtime/src/fiber.rs")),
    ("io.rs", include_str!("../../../runtime/src/io.rs")),
    ("map.rs", include_str!("../../../runtime/src/map.rs")),
    ("panic.rs", include_str!("../../../runtime/src/panic.rs")),
    ("string.rs", include_str!("../../../runtime/src/string.rs")),
    (
        "strconv.rs",
        include_str!("../../../runtime/src/strconv.rs"),
    ),
    (
        "strings.rs",
        include_str!("../../../runtime/src/strings.rs"),
    ),
    ("task.rs", include_str!("../../../runtime/src/task.rs")),
];

#[derive(Debug)]
pub enum BuildError {
    Diagnostics(Vec<Diagnostic>),
    NotExecutable(String),
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

pub fn emit_llvm(file: &SourceFile) -> Result<String, BuildError> {
    let project = load_file(file).map_err(BuildError::Diagnostics)?;
    emit_project_llvm(&project, file)
}

pub fn emit_project_llvm(project: &Project, sources: &dyn Sources) -> Result<String, BuildError> {
    let checked = check_project(project, sources);
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
    let mut program = lower(&package);
    dropck::insert(&package, &mut program);
    let layered = Layered(sources, project.std_sources());
    codegen::emit(&package, &program, &layered).map_err(BuildError::Diagnostics)
}

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// Removed on drop.
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

/// `ZORE_CC`, otherwise `clang` on `PATH`.
pub fn compiler() -> String {
    std::env::var("ZORE_CC").unwrap_or_else(|_| "clang".into())
}

pub fn build(file: &SourceFile, output: &Path) -> Result<(), BuildError> {
    link(&emit_llvm(file)?, output)
}

pub fn build_project(
    project: &Project,
    sources: &dyn Sources,
    output: &Path,
) -> Result<(), BuildError> {
    link(&emit_project_llvm(project, sources)?, output)
}

fn link(ir: &str, output: &Path) -> Result<(), BuildError> {
    let dir = TempDir::new()?;
    let ir_path = dir.path().join("program.ll");
    let object_path = dir.path().join("program.o");
    fs::write(&ir_path, ir).map_err(|e| BuildError::Io(format!("{}: {e}", ir_path.display())))?;
    for &(name, contents) in RUNTIME_SOURCES {
        let path = dir.path().join(name);
        fs::write(&path, contents)
            .map_err(|e| BuildError::Io(format!("{}: {e}", path.display())))?;
    }
    let cc = compiler();
    let result = Command::new(&cc)
        .arg("-O2")
        .arg("-Wno-override-module")
        .arg("-fPIC")
        .arg("-c")
        .arg(&ir_path)
        .arg("-o")
        .arg(&object_path)
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
    let rustc = std::env::var_os("ZORE_RUSTC").unwrap_or_else(|| "rustc".into());
    let mut link_arg = std::ffi::OsString::from("link-arg=");
    link_arg.push(&object_path);
    let result = Command::new(&rustc)
        .arg("--edition=2024")
        .arg("--crate-name=zore_program")
        .arg("-Copt-level=2")
        .arg("-Cpanic=abort")
        .arg("-C")
        .arg(format!("linker={cc}"))
        .arg("-C")
        .arg(link_arg)
        .arg(dir.path().join("main.rs"))
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|error| {
            BuildError::Toolchain(format!(
                "could not run `{}` ({error}); `zore build` requires rustc 1.98 or newer \
                 to compile the Rust runtime (set ZORE_RUSTC to choose a compiler)",
                rustc.to_string_lossy()
            ))
        })?;
    if !result.status.success() {
        return Err(BuildError::Toolchain(format!(
            "`{}` failed to compile or link the Rust runtime; ensure rustc 1.98 or newer \
             and clang target the same host:\n{}",
            rustc.to_string_lossy(),
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}
