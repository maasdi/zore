use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use sha2::{Digest, Sha256};

use super::native::Providers;
use crate::codegen;
use crate::diagnostic::Diagnostic;
use crate::driver::check::check_project_with_entry;
use crate::driver::project::{Project, load_file};
use crate::dropck;
use crate::mir::lower::lower;
use crate::source::{Layered, SourceFile, Sources};

const RUNTIME_MAIN: &str = include_str!("../../../runtime/src/main.rs");

const RUNTIME_SOURCES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("../../../runtime/src/lib.rs")),
    ("native.rs", include_str!("../../../runtime/src/native.rs")),
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
    (
        "deadline.rs",
        include_str!("../../../runtime/src/deadline.rs"),
    ),
    ("slot.rs", include_str!("../../../runtime/src/slot.rs")),
    ("signal.rs", include_str!("../../../runtime/src/signal.rs")),
    (
        "signal_windows.rs",
        include_str!("../../../runtime/src/signal_windows.rs"),
    ),
    ("errors.rs", include_str!("../../../runtime/src/errors.rs")),
    ("exec.rs", include_str!("../../../runtime/src/exec.rs")),
    (
        "filepath.rs",
        include_str!("../../../runtime/src/filepath.rs"),
    ),
    ("io.rs", include_str!("../../../runtime/src/io.rs")),
    ("float.rs", include_str!("../../../runtime/src/float.rs")),
    ("map.rs", include_str!("../../../runtime/src/map.rs")),
    ("math.rs", include_str!("../../../runtime/src/math.rs")),
    ("mutex.rs", include_str!("../../../runtime/src/mutex.rs")),
    ("net.rs", include_str!("../../../runtime/src/net.rs")),
    (
        "net_poll.rs",
        include_str!("../../../runtime/src/net_poll.rs"),
    ),
    ("os.rs", include_str!("../../../runtime/src/os.rs")),
    ("panic.rs", include_str!("../../../runtime/src/panic.rs")),
    (
        "reactor.rs",
        include_str!("../../../runtime/src/reactor.rs"),
    ),
    (
        "reactor_windows.rs",
        include_str!("../../../runtime/src/reactor_windows.rs"),
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
        "task_scope.rs",
        include_str!("../../../runtime/src/task_scope.rs"),
    ),
    (
        "unicode.rs",
        include_str!("../../../runtime/src/unicode.rs"),
    ),
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

pub enum Stage<'a> {
    Checked,
    Lowered,
    Emitted(&'a str),
}

pub fn emit_project_llvm(project: &Project, sources: &dyn Sources) -> Result<String, BuildError> {
    emit_stages(project, sources, None, &mut |_| {})
}

pub fn native_ir(
    project: &Project,
    sources: &dyn Sources,
    observe: &mut dyn FnMut(Stage<'_>),
) -> Result<String, BuildError> {
    native_ir_with_entry(project, sources, None, observe)
}

fn native_ir_with_entry(
    project: &Project,
    sources: &dyn Sources,
    entry_name: Option<&str>,
    observe: &mut dyn FnMut(Stage<'_>),
) -> Result<String, BuildError> {
    let ir = emit_stages(project, sources, entry_name, observe)?;
    observe(Stage::Emitted(&ir));
    let pruned = match codegen::retain_reachable_functions(&ir) {
        Cow::Owned(pruned) => Some(pruned),
        Cow::Borrowed(_) => None,
    };
    Ok(pruned.unwrap_or(ir))
}

fn emit_stages(
    project: &Project,
    sources: &dyn Sources,
    entry_name: Option<&str>,
    observe: &mut dyn FnMut(Stage<'_>),
) -> Result<String, BuildError> {
    let checked = check_project_with_entry(project, sources, entry_name);
    if !checked.diagnostics.is_empty() {
        return Err(BuildError::Diagnostics(checked.diagnostics));
    }
    let package = checked
        .package
        .expect("no diagnostics means a checked package");
    if entry_name.is_none() && package.entry.is_none() {
        return Err(BuildError::NotExecutable(format!(
            "package `{}` is not executable; only a `main` package with `func main()` can be built",
            package.name
        )));
    }
    observe(Stage::Checked);
    let mut program = lower(&package);
    dropck::insert(&package, &mut program);
    let machines = crate::async_lowering::lower(&package, &program);
    observe(Stage::Lowered);
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

    pub fn executable(&self, name: &str) -> PathBuf {
        self.0
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
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
    let project = load_file(file).map_err(BuildError::Diagnostics)?;
    link(
        &native_ir(&project, file, &mut |_| {})?,
        output,
        RUNTIME_MAIN,
        &Providers::new(),
    )
}

pub fn build_project(
    project: &Project,
    sources: &dyn Sources,
    output: &Path,
) -> Result<(), BuildError> {
    link(
        &native_ir(project, sources, &mut |_| {})?,
        output,
        RUNTIME_MAIN,
        &project.native_adapters,
    )
}

pub(crate) fn build_test_project(
    project: &Project,
    sources: &dyn Sources,
    output: &Path,
    entry_name: &str,
) -> Result<(), BuildError> {
    link(
        &native_ir_with_entry(project, sources, Some(entry_name), &mut |_| {})?,
        output,
        include_str!("../../../runtime/src/test_main.rs"),
        &project.native_adapters,
    )
}

fn link(
    ir: &str,
    output: &Path,
    runtime_main: &str,
    native_adapters: &Providers,
) -> Result<(), BuildError> {
    if cfg!(not(target_pointer_width = "64")) {
        return Err(BuildError::Toolchain("native builds require a 64-bit host toolchain; check and fmt remain available without native linking".into()));
    }
    if cfg!(all(
        windows,
        not(all(target_env = "msvc", target_arch = "x86_64"))
    )) {
        return Err(BuildError::Toolchain(
            "Windows native builds require x86_64-pc-windows-msvc; install the x64 MSVC Rust toolchain, Visual Studio C++ Build Tools and clang".into(),
        ));
    }
    if !native_adapters.is_empty() && !native_host_supported() {
        return Err(BuildError::Toolchain("curated native adapters support Linux GNU/macOS x86_64 or aarch64 and Windows x86_64 MSVC; use a supported native host (check remains available)".into()));
    }
    let dir = TempDir::new()?;
    let ir_path = dir.path().join("program.ll");
    let object_path = dir.path().join(if cfg!(windows) {
        "program.obj"
    } else {
        "program.o"
    });
    fs::write(&ir_path, ir.as_bytes())
        .map_err(|e| BuildError::Io(format!("{}: {e}", ir_path.display())))?;
    let cc = compiler();
    let mut compile = Command::new(&cc);
    compile.args(["-O2", "-Wno-override-module"]);
    if cfg!(windows) {
        compile.arg("--target=x86_64-pc-windows-msvc");
    } else {
        compile.arg("-fPIC");
    }
    let compilation = compile
        .arg("-c")
        .arg(&ir_path)
        .arg("-o")
        .arg(&object_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let compilation = match compilation {
        Ok(compilation) => compilation,
        Err(error) => {
            return Err(BuildError::Toolchain(format!(
                "could not run `{cc}` ({error}); `zore build` requires clang with LLVM 15 or \
                 newer (set ZORE_CC to choose a compiler)"
            )));
        }
    };
    let rustc = std::env::var_os("ZORE_RUSTC").unwrap_or_else(|| "rustc".into());
    let (result, runtime) = std::thread::scope(|scope| {
        let runtime = scope.spawn(|| runtime_library(&rustc, dir.path(), native_adapters));
        let result = compilation.wait_with_output();
        (
            result,
            runtime.join().expect("runtime build worker panicked"),
        )
    });
    let result = result.map_err(|error| {
        BuildError::Toolchain(format!("could not collect `{cc}` output: {error}"))
    })?;
    if !result.status.success() {
        return Err(BuildError::Toolchain(format!(
            "`{cc}` failed to compile the generated program; this is a compiler bug:\n{}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    let runtime = runtime?;
    let native_objects = native_objects(&cc, dir.path(), native_adapters)?;
    let main_path = dir.path().join("main.rs");
    fs::write(&main_path, runtime_main)
        .map_err(|e| BuildError::Io(format!("{}: {e}", main_path.display())))?;
    let mut link_arg = std::ffi::OsString::from("link-arg=");
    link_arg.push(&object_path);
    let mut extern_arg = std::ffi::OsString::from("zore_runtime=");
    extern_arg.push(&runtime);
    let mut link = Command::new(&rustc);
    if !cfg!(windows) {
        link.arg("-C").arg(format!("linker={cc}"));
    }
    if cfg!(windows)
        && native_adapters
            .values()
            .any(|provider| provider.adapter.mbedtls)
    {
        link.arg("-l").arg("bcrypt");
        link.arg("-l").arg("crypt32");
    }
    for object in native_objects {
        let mut argument = std::ffi::OsString::from("link-arg=");
        argument.push(object);
        link.arg("-C").arg(argument);
    }
    let result = link
        .arg("--edition=2024")
        .arg("--crate-name=zore_program")
        .arg("-Copt-level=2")
        .arg("-Cpanic=abort")
        .arg("--extern")
        .arg(extern_arg)
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
             and clang target the same host; Windows also requires Visual Studio C++ Build Tools and the Windows SDK:\n{}",
            rustc.to_string_lossy(),
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}

fn native_host_supported() -> bool {
    cfg!(all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        any(
            target_os = "macos",
            all(target_os = "linux", target_env = "gnu")
        )
    )) || cfg!(all(windows, target_arch = "x86_64", target_env = "msvc"))
}

fn native_objects(
    cc: &str,
    directory: &Path,
    selections: &Providers,
) -> Result<Vec<PathBuf>, BuildError> {
    let mut objects = Vec::new();
    let mut vendor = None;
    if selections.values().any(|provider| provider.adapter.mbedtls) {
        let closure = mbedtls_closure(cc, directory)?;
        objects.extend(mbedtls_sources().map(|name| closure.join(mbedtls_object(name))));
        vendor = Some(closure);
    }
    for (id, provider) in selections {
        let adapter = provider.adapter;
        let source = directory.join(format!("{}.c", adapter.project));
        let header = directory.join(format!("{}.h", adapter.project));
        let object = directory.join(format!(
            "{}.{}",
            adapter.project,
            if cfg!(windows) { "obj" } else { "o" }
        ));
        fs::write(&source, &provider.source)
            .and_then(|_| fs::write(&header, &provider.header))
            .map_err(|e| BuildError::Io(format!("{}: {e}", source.display())))?;
        let mut compile = Command::new(cc);
        compile.args(super::native::C_FLAGS);
        compile.args(adapter.c_flags);
        if adapter.mbedtls {
            mbedtls_flags(&mut compile, vendor.as_deref().expect("Mbed TLS closure"));
        }
        let result = compile
            .arg(host_c_flag())
            .arg("-c")
            .arg(&source)
            .arg("-o")
            .arg(&object)
            .output()
            .map_err(|e| {
                BuildError::Toolchain(format!(
                    "could not compile native adapter {id} with ZORE_CC={cc}: {e}"
                ))
            })?;
        if !result.status.success() {
            return Err(BuildError::Toolchain(format!(
                "native adapter {id} failed to compile; use the documented host C toolchain:\n{}",
                String::from_utf8_lossy(&result.stderr)
            )));
        }
        objects.push(object);
    }
    Ok(objects)
}

fn host_c_flag() -> &'static str {
    if cfg!(windows) {
        "--target=x86_64-pc-windows-msvc"
    } else {
        "-fPIC"
    }
}

const MBEDTLS_DEFINES: &[&str] = &[
    "-DMBEDTLS_CONFIG_FILE=\"zore_config.h\"",
    "-D_DEFAULT_SOURCE",
];

fn mbedtls_flags(command: &mut Command, vendor: &Path) {
    command
        .arg("-I")
        .arg(vendor.join("include"))
        .arg("-I")
        .arg(vendor.join("library"))
        .arg("-I")
        .arg(vendor)
        .args(MBEDTLS_DEFINES);
}

fn mbedtls_sources() -> impl Iterator<Item = &'static str> {
    super::mbedtls::SOURCES
        .iter()
        .map(|&(name, _)| name)
        .filter(|name| name.ends_with(".c"))
}

fn mbedtls_object(source: &str) -> PathBuf {
    Path::new(source).with_extension("o")
}

/// Cached per closure, compiler and flags; compiled into `scratch` when no cache folder is usable.
fn mbedtls_closure(cc: &str, scratch: &Path) -> Result<PathBuf, BuildError> {
    let version = Command::new(cc)
        .arg("--version")
        .output()
        .map_err(|error| BuildError::Toolchain(format!("could not run `{cc}`: {error}")))?;
    let mut hash = Sha256::new();
    let parts = [
        super::mbedtls::FINGERPRINT.as_bytes(),
        super::mbedtls::RECIPE.as_bytes(),
        cc.as_bytes(),
        &version.stdout,
        host_c_flag().as_bytes(),
    ];
    let flags = super::native::VENDORED_C_FLAGS
        .iter()
        .chain(MBEDTLS_DEFINES);
    for part in parts.into_iter().chain(flags.map(|flag| flag.as_bytes())) {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    let Some(cached) = cache_root().map(|root| root.join(format!("mbedtls-{:x}", hash.finalize())))
    else {
        return compile_mbedtls(cc, scratch.join("mbedtls"));
    };
    if cached.is_dir() {
        return Ok(cached);
    }
    let staged = cached.with_file_name(format!(
        "staged-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    if fs::create_dir_all(&staged).is_err() {
        return compile_mbedtls(cc, scratch.join("mbedtls"));
    }
    match compile_mbedtls(cc, staged.clone()) {
        Ok(_) if fs::rename(&staged, &cached).is_ok() => Ok(cached),
        Ok(_) if cached.is_dir() => {
            let _ = fs::remove_dir_all(&staged);
            Ok(cached)
        }
        Ok(staged) => Ok(staged),
        Err(error) => {
            let _ = fs::remove_dir_all(&staged);
            Err(error)
        }
    }
}

fn compile_mbedtls(cc: &str, vendor: PathBuf) -> Result<PathBuf, BuildError> {
    for &(name, contents) in super::mbedtls::SOURCES {
        let path = vendor.join(name);
        fs::create_dir_all(path.parent().expect("vendor path"))
            .and_then(|()| fs::write(&path, contents))
            .map_err(|error| BuildError::Io(format!("{}: {error}", path.display())))?;
    }
    let sources: Vec<&str> = mbedtls_sources().collect();
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(sources.len());
    let failure = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut failures = Vec::new();
                    while let Some(&name) = sources.get(next.fetch_add(1, Ordering::Relaxed)) {
                        if let Err(error) = compile_mbedtls_source(cc, &vendor, name) {
                            failures.push((name, error));
                        }
                    }
                    failures
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("Mbed TLS compile worker"))
            .min_by_key(|&(name, _)| name)
    });
    match failure {
        Some((_, error)) => Err(error),
        None => Ok(vendor),
    }
}

fn compile_mbedtls_source(cc: &str, vendor: &Path, name: &str) -> Result<(), BuildError> {
    let mut compile = Command::new(cc);
    compile
        .current_dir(vendor)
        .args(super::native::VENDORED_C_FLAGS);
    mbedtls_flags(&mut compile, Path::new("."));
    let result = compile
        .arg(host_c_flag())
        .arg("-c")
        .arg(name)
        .arg("-o")
        .arg(mbedtls_object(name))
        .output()
        .map_err(|error| BuildError::Toolchain(format!("could not compile Mbed TLS: {error}")))?;
    if !result.status.success() {
        return Err(BuildError::Toolchain(format!(
            "Mbed TLS {name} compilation failed:\n{}",
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
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .filter(|directory| !directory.is_empty())
            .map(|directory| PathBuf::from(directory).join("zore").join("cache"));
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
fn runtime_library(
    rustc: &std::ffi::OsStr,
    scratch: &Path,
    native_adapters: &Providers,
) -> Result<PathBuf, BuildError> {
    let version = Command::new(rustc)
        .arg("-vV")
        .output()
        .map_err(|error| toolchain_error(rustc, &error))?;
    if !version.status.success() {
        return Err(BuildError::Toolchain(format!(
            "`{}` could not report its host toolchain:\n{}",
            rustc.to_string_lossy(),
            String::from_utf8_lossy(&version.stderr)
        )));
    }
    if cfg!(windows)
        && !String::from_utf8_lossy(&version.stdout)
            .lines()
            .any(|line| line == "host: x86_64-pc-windows-msvc")
    {
        return Err(BuildError::Toolchain("Windows native builds require ZORE_RUSTC to select an x86_64-pc-windows-msvc compiler; GNU and ARM Windows toolchains are not supported".into()));
    }
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(&version.stdout, &mut hasher);
    for &(name, contents) in RUNTIME_SOURCES {
        std::hash::Hash::hash(&(name, contents), &mut hasher);
    }
    for provider in native_adapters.values() {
        let adapter = provider.adapter;
        std::hash::Hash::hash(&(adapter.id, adapter.fingerprint), &mut hasher);
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
    let mut compile = Command::new(rustc);
    if !native_adapters.is_empty() {
        let mut registration = String::new();
        let adapters = sources.join("adapters");
        fs::create_dir_all(&adapters)
            .map_err(|e| BuildError::Io(format!("{}: {e}", adapters.display())))?;
        for (index, provider) in native_adapters.values().enumerate() {
            let bridge = adapters.join(format!("bridge_{index}.rs"));
            fs::write(&bridge, &provider.bridge)
                .map_err(|e| BuildError::Io(format!("{}: {e}", bridge.display())))?;
            registration.push_str(&format!(
                "#[path = \"adapters/bridge_{index}.rs\"]\nmod native_adapter_{index};\n"
            ));
        }
        let path = sources.join("adapters.rs");
        fs::write(&path, registration)
            .map_err(|e| BuildError::Io(format!("{}: {e}", path.display())))?;
        compile.arg("--cfg").arg("zore_native_adapters");
        compile.env("ZORE_NATIVE_ADAPTERS", path);
    }
    let result = compile
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

/// Publishes a complete runtime directory without replacing one another build may be linking.
fn keep_in_cache(built: &Path, path: &Path) -> Option<PathBuf> {
    let root = path.parent()?.parent()?;
    fs::create_dir_all(root).ok()?;
    let staged = root.join(format!(
        "staged-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    publish_in_cache(built, path, &staged)
}

fn publish_in_cache(built: &Path, path: &Path, staged: &Path) -> Option<PathBuf> {
    let target = path.parent()?;
    fs::create_dir(staged).ok()?;
    let published = stage_runtime(built, &staged.join(path.file_name()?)).is_ok()
        && (fs::rename(staged, target).is_ok()
            || (fs::remove_dir(target).is_ok() && fs::rename(staged, target).is_ok()));
    if !published {
        let _ = fs::remove_dir_all(staged);
    }
    published.then(|| path.to_path_buf())
}

fn stage_runtime(built: &Path, staged: &Path) -> std::io::Result<()> {
    let mut source = fs::File::open(built)?;
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staged)?;
    std::io::copy(&mut source, &mut destination)?;
    destination.sync_all()
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::sync::Barrier;
    use std::thread;

    use super::*;

    fn cache_root(cached: &Path) -> &Path {
        cached.parent().unwrap().parent().unwrap()
    }

    #[test]
    fn runtime_cache_publishes_complete_files_and_preserves_private_builds() {
        let directory = TempDir::new().unwrap();
        let built = directory.path().join("built.rlib");
        let cached = directory
            .path()
            .join("cache/runtime-0/libzore_runtime.rlib");
        let contents = vec![42; 65536];
        fs::write(&built, &contents).unwrap();

        assert_eq!(keep_in_cache(&built, &cached), Some(cached.clone()));
        assert_eq!(fs::read(&cached).unwrap(), contents);
        assert_eq!(fs::read(&built).unwrap(), contents);
        assert_eq!(fs::read_dir(cache_root(&cached)).unwrap().count(), 1);
    }

    #[test]
    fn runtime_cache_preserves_an_existing_published_file() {
        let directory = TempDir::new().unwrap();
        let built = directory.path().join("built.rlib");
        let cached = directory
            .path()
            .join("cache/runtime-0/libzore_runtime.rlib");
        fs::write(&built, b"first runtime").unwrap();
        assert_eq!(keep_in_cache(&built, &cached), Some(cached.clone()));
        let mut published = fs::File::open(&cached).unwrap();

        fs::write(&built, b"later runtime").unwrap();
        let published_again = keep_in_cache(&built, &cached);
        assert_eq!(fs::read(&cached).unwrap(), b"first runtime");
        assert_eq!(published_again, None);
        let mut retained = Vec::new();
        published.read_to_end(&mut retained).unwrap();
        assert_eq!(retained, b"first runtime");
        assert_eq!(fs::read(&built).unwrap(), b"later runtime");
        assert_eq!(fs::read_dir(cache_root(&cached)).unwrap().count(), 1);
    }

    #[test]
    fn runtime_cache_publication_failure_preserves_the_private_build() {
        let directory = TempDir::new().unwrap();
        let built = directory.path().join("built.rlib");
        let blocked = directory.path().join("blocked");
        fs::write(&built, b"private runtime").unwrap();
        fs::write(&blocked, b"regular file").unwrap();

        assert_eq!(
            keep_in_cache(&built, &blocked.join("runtime-0/libzore_runtime.rlib")),
            None
        );
        assert_eq!(fs::read(&built).unwrap(), b"private runtime");
        assert_eq!(fs::read(&blocked).unwrap(), b"regular file");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn runtime_cache_publishes_over_an_empty_leftover_directory() {
        let directory = TempDir::new().unwrap();
        let built = directory.path().join("built.rlib");
        let cached = directory
            .path()
            .join("cache/runtime-0/libzore_runtime.rlib");
        fs::write(&built, b"runtime").unwrap();
        fs::create_dir_all(cached.parent().unwrap()).unwrap();

        assert_eq!(keep_in_cache(&built, &cached), Some(cached.clone()));
        assert_eq!(fs::read(&cached).unwrap(), b"runtime");
        assert_eq!(fs::read_dir(cache_root(&cached)).unwrap().count(), 1);
    }

    #[test]
    fn runtime_cache_staging_collision_cannot_mutate_a_published_file() {
        let directory = TempDir::new().unwrap();
        let built = directory.path().join("built.rlib");
        let cached = directory
            .path()
            .join("cache/runtime-0/libzore_runtime.rlib");
        fs::write(&built, b"first runtime").unwrap();
        assert_eq!(keep_in_cache(&built, &cached), Some(cached.clone()));
        let stale = cache_root(&cached).join("stale-staging-dir");
        fs::create_dir(&stale).unwrap();
        let stale = stale.join("libzore_runtime.rlib");
        fs::hard_link(&cached, &stale).unwrap();
        fs::write(&built, b"later runtime").unwrap();

        assert_eq!(
            publish_in_cache(&built, &cached, stale.parent().unwrap()),
            None
        );
        assert_eq!(fs::read(&cached).unwrap(), b"first runtime");
        assert_eq!(fs::read(&stale).unwrap(), b"first runtime");
        assert_eq!(fs::read(&built).unwrap(), b"later runtime");
    }

    #[test]
    fn runtime_cache_readers_keep_the_first_file_during_concurrent_publication() {
        let directory = TempDir::new().unwrap();
        let built = directory.path().join("first.rlib");
        let cached = directory
            .path()
            .join("cache/runtime-0/libzore_runtime.rlib");
        let contents = vec![42; 65536];
        fs::write(&built, &contents).unwrap();
        assert_eq!(keep_in_cache(&built, &cached), Some(cached.clone()));
        #[cfg(unix)]
        let published = fs::metadata(&cached).unwrap();
        let start = Barrier::new(6);
        let later: Vec<_> = (0..4)
            .map(|index| {
                let built = directory.path().join(format!("later-{index}.rlib"));
                fs::write(&built, vec![index; contents.len()]).unwrap();
                built
            })
            .collect();
        let readers: Vec<_> = (0..2).map(|_| fs::File::open(&cached).unwrap()).collect();

        thread::scope(|scope| {
            for built in later {
                let cached = &cached;
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    for _ in 0..32 {
                        assert_eq!(keep_in_cache(&built, cached), None);
                    }
                });
            }
            for mut retained in readers {
                let cached = &cached;
                let contents = &contents;
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    for _ in 0..128 {
                        assert_eq!(&fs::read(cached).unwrap(), contents);
                    }
                    let mut bytes = Vec::new();
                    retained.read_to_end(&mut bytes).unwrap();
                    assert_eq!(&bytes, contents);
                });
            }
        });

        assert_eq!(fs::read(&cached).unwrap(), contents);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = fs::metadata(&cached).unwrap();
            assert_eq!(
                (current.dev(), current.ino()),
                (published.dev(), published.ino())
            );
        }
        assert_eq!(fs::read_dir(cache_root(&cached)).unwrap().count(), 1);
    }
}
