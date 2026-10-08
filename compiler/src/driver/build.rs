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

const RUNTIME_MAIN: &str = include_str!("../../../runtime/src/main.rs");

const RUNTIME_SOURCES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("../../../runtime/src/lib.rs")),
    ("alloc.rs", include_str!("../../../runtime/src/alloc.rs")),
    (
        "blocking.rs",
        include_str!("../../../runtime/src/blocking.rs"),
    ),
    (
        "channel.rs",
        include_str!("../../../runtime/src/channel.rs"),
    ),
    (
        "deadlock.rs",
        include_str!("../../../runtime/src/deadlock.rs"),
    ),
    ("slot.rs", include_str!("../../../runtime/src/slot.rs")),
    ("io.rs", include_str!("../../../runtime/src/io.rs")),
    ("map.rs", include_str!("../../../runtime/src/map.rs")),
    ("mutex.rs", include_str!("../../../runtime/src/mutex.rs")),
    ("net.rs", include_str!("../../../runtime/src/net.rs")),
    (
        "net_poll.rs",
        include_str!("../../../runtime/src/net_poll.rs"),
    ),
    ("panic.rs", include_str!("../../../runtime/src/panic.rs")),
    (
        "reactor.rs",
        include_str!("../../../runtime/src/reactor.rs"),
    ),
    ("string.rs", include_str!("../../../runtime/src/string.rs")),
    (
        "strconv.rs",
        include_str!("../../../runtime/src/strconv.rs"),
    ),
    (
        "strings.rs",
        include_str!("../../../runtime/src/strings.rs"),
    ),
    ("sys.rs", include_str!("../../../runtime/src/sys.rs")),
    ("task.rs", include_str!("../../../runtime/src/task.rs")),
    (
        "scheduler.rs",
        include_str!("../../../runtime/src/scheduler.rs"),
    ),
    ("waiter.rs", include_str!("../../../runtime/src/waiter.rs")),
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
    let machines = crate::async_lowering::lower(&package, &program);
    let layered = Layered(sources, project.std_sources());
    codegen::emit(&package, &program, &machines, &layered).map_err(BuildError::Diagnostics)
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
    let runtime = runtime_library(&rustc, dir.path())?;
    let main_path = dir.path().join("main.rs");
    fs::write(&main_path, RUNTIME_MAIN)
        .map_err(|e| BuildError::Io(format!("{}: {e}", main_path.display())))?;
    let mut link_arg = std::ffi::OsString::from("link-arg=");
    link_arg.push(&object_path);
    let mut extern_arg = std::ffi::OsString::from("zore_runtime=");
    extern_arg.push(&runtime);
    let result = Command::new(&rustc)
        .arg("--edition=2024")
        .arg("--crate-name=zore_program")
        .arg("-Copt-level=2")
        .arg("-Cpanic=abort")
        .arg("--extern")
        .arg(extern_arg)
        .arg("-C")
        .arg(format!("linker={cc}"))
        .arg("-C")
        .arg(link_arg)
        .arg(&main_path)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|error| toolchain_error(&rustc, &error))?;
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

fn toolchain_error(rustc: &std::ffi::OsStr, error: &std::io::Error) -> BuildError {
    BuildError::Toolchain(format!(
        "could not run `{}` ({error}); `zore build` requires rustc 1.98 or newer \
         to compile the Rust runtime (set ZORE_RUSTC to choose a compiler)",
        rustc.to_string_lossy()
    ))
}

/// Where compiled runtimes are kept between builds: `ZORE_CACHE_DIR`, otherwise the user's cache
/// folder. A shared temporary folder is never used, since another user could plant a library there.
fn cache_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ZORE_CACHE_DIR") {
        return Some(PathBuf::from(dir).join("zore"));
    }
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join("zore"));
    }
    let home = PathBuf::from(std::env::var_os("HOME").filter(|home| !home.is_empty())?);
    if cfg!(target_os = "macos") {
        Some(home.join("Library").join("Caches").join("zore"))
    } else {
        Some(home.join(".cache").join("zore"))
    }
}

/// The runtime compiled as a library, reused across builds with the same sources and `rustc`.
/// Without a usable cache folder it is compiled into `scratch` and used once.
fn runtime_library(rustc: &std::ffi::OsStr, scratch: &Path) -> Result<PathBuf, BuildError> {
    let version = Command::new(rustc)
        .arg("-vV")
        .output()
        .map_err(|error| toolchain_error(rustc, &error))?;
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(&version.stdout, &mut hasher);
    for &(name, contents) in RUNTIME_SOURCES {
        std::hash::Hash::hash(&(name, contents), &mut hasher);
    }
    let key = std::hash::Hasher::finish(&hasher);
    let cached = cache_root().map(|root| {
        root.join(format!("runtime-{key:016x}"))
            .join("libzore_runtime.rlib")
    });
    if let Some(path) = cached.as_ref().filter(|path| path.is_file()) {
        return Ok(path.clone());
    }
    let sources = scratch.join("runtime");
    fs::create_dir_all(&sources)
        .map_err(|e| BuildError::Io(format!("{}: {e}", sources.display())))?;
    for &(name, contents) in RUNTIME_SOURCES {
        let path = sources.join(name);
        fs::write(&path, contents)
            .map_err(|e| BuildError::Io(format!("{}: {e}", path.display())))?;
    }
    let built = scratch.join("libzore_runtime.rlib");
    let result = Command::new(rustc)
        .arg("--edition=2024")
        .arg("--crate-name=zore_runtime")
        .arg("--crate-type=rlib")
        .arg("-Copt-level=2")
        .arg("-Cpanic=abort")
        .arg(sources.join("lib.rs"))
        .arg("-o")
        .arg(&built)
        .output()
        .map_err(|error| toolchain_error(rustc, &error))?;
    if !result.status.success() {
        return Err(BuildError::Toolchain(format!(
            "`{}` failed to compile the Rust runtime; ensure rustc 1.98 or newer \
             and clang target the same host:\n{}",
            rustc.to_string_lossy(),
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(cached
        .and_then(|path| keep_in_cache(&built, &path))
        .unwrap_or(built))
}

/// Copies `built` to `path` so that readers see either nothing or the whole file.
fn keep_in_cache(built: &Path, path: &Path) -> Option<PathBuf> {
    let folder = path.parent()?;
    fs::create_dir_all(folder).ok()?;
    let staged = folder.join(format!(
        "staged-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    let moved = fs::copy(built, &staged)
        .and_then(|_| fs::rename(&staged, path))
        .is_ok();
    if !moved {
        let _ = fs::remove_file(&staged);
        return None;
    }
    Some(path.to_path_buf())
}
