use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use sha2::{Digest, Sha256};

use super::project::{Disk, FileSystem, Manifest};
use crate::source::SourceError;

mod cache;
mod lock;
mod versioned;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum Mode {
    #[default]
    Build,
    Install,
    Lock,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Options {
    pub locked: bool,
    pub offline: bool,
    pub mode: Mode,
}

pub(super) struct DependencyDisk(pub Options);

impl FileSystem for DependencyDisk {
    fn read_regular(&self, path: &Path) -> Result<String, SourceError> {
        Disk.read_regular(path)
    }

    fn require_lockfile(&self) -> bool {
        self.0.locked
    }
    fn read(&self, path: &Path) -> Result<String, SourceError> {
        Disk.read(path)
    }

    fn ore_files(&self, dir: &Path) -> Result<Vec<PathBuf>, SourceError> {
        Disk.ore_files(dir)
    }

    fn manifest(&self, dir: &Path) -> Result<Option<Manifest>, SourceError> {
        Disk.manifest(dir)
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        Disk.canonical(path)
    }

    fn projects(&self, root: Manifest) -> Result<HashMap<String, Manifest>, SourceError> {
        resolve(root, self.0)
    }
}

pub(super) fn run(target: &Path, options: Options) -> ExitCode {
    let result = (|| {
        fs::metadata(target).map_err(|e| error(target, e))?;
        let directory = if target.is_dir() {
            target
        } else {
            target
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
        };
        let root = Disk
            .manifest(directory)?
            .ok_or_else(|| error(target, "no zore.toml project was found"))?;
        resolve(root, options).map(|_| ())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zore: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Clone)]
struct GitProject {
    url: String,
    revision: String,
    tree: String,
    checkout: PathBuf,
}

pub(super) fn resolve(
    root: Manifest,
    options: Options,
) -> Result<HashMap<String, Manifest>, SourceError> {
    let entry = Disk.canonical(&root.root);
    let lock_path = entry.join("zore.lock");
    let _guard = if options.mode != Mode::Build {
        Some(ProjectGuard::acquire(&entry)?)
    } else {
        None
    };
    let existing = match fs::read_to_string(&lock_path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(error(&lock_path, e)),
    };
    if options.locked && existing.is_none() {
        return Err(error(
            &lock_path,
            "--locked requires zore.lock; run `zore install` first",
        ));
    }
    versioned::resolve(root, options, existing.as_deref())
}

fn lockfile(
    entry: &Path,
    projects: &HashMap<String, Manifest>,
    git_projects: &HashMap<PathBuf, GitProject>,
    versions: &BTreeMap<PathBuf, crate::driver::release::Version>,
    bindings: &lock::Bindings,
) -> Result<String, SourceError> {
    let mut text = format!(
        "version = {}\n\n[projects]\n",
        if bindings.is_empty() { 1 } else { 2 }
    );
    let sorted: BTreeMap<_, _> = projects.iter().collect();
    for (name, manifest) in sorted {
        let bytes =
            fs::read(manifest.root.join("zore.toml")).map_err(|e| error(&manifest.root, e))?;
        if bytes != manifest.text.as_bytes() {
            return Err(error(
                &manifest.root,
                "manifest changed during dependency resolution; retry",
            ));
        }
        text.push_str(&format!(
            "{name} = {{ manifest = {}",
            quote(&digest(manifest.text.as_bytes()))
        ));
        if let Some(source) = git_projects.get(&manifest.root) {
            let subdir = manifest
                .root
                .strip_prefix(&source.checkout)
                .expect("confined Git project");
            text.push_str(&format!(
                ", git = {}, rev = {}, subdir = {}, tree = {}",
                quote(&source.url),
                quote(&source.revision),
                quote(&portable(subdir)?),
                quote(&source.tree)
            ));
        } else {
            text.push_str(&format!(
                ", path = {}",
                quote(&portable(&relative(entry, &manifest.root))?)
            ));
        }
        if let Some(version) = versions.get(&manifest.root) {
            text.push_str(&format!(
                ", version = {}, tag = {}",
                quote(&version.to_string()),
                quote(&format!("{name}/v{version}"))
            ));
        }
        if let Some(id) = &manifest.native {
            let adapter = super::native::find(id).expect("validated native adapter");
            text.push_str(&format!(
                ", adapter = {}, recipe = {}",
                quote(id),
                quote(adapter.fingerprint)
            ));
        }
        text.push_str(" }\n");
    }
    if !bindings.is_empty() {
        text.push_str(&lock::render_bindings(bindings));
    }
    Ok(text)
}

fn portable(path: &Path) -> Result<String, SourceError> {
    path.to_str()
        .map(|s| {
            if cfg!(windows) {
                s.replace('\\', "/")
            } else {
                s.into()
            }
        })
        .ok_or_else(|| error(path, "lockfile paths must be UTF-8"))
}

fn relative(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    if from.first() != to.first() {
        return to.iter().collect();
    }
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut path = PathBuf::new();
    for part in &from[shared..] {
        if matches!(part, Component::Normal(_)) {
            path.push("..");
        }
    }
    for part in &to[shared..] {
        path.push(part.as_os_str());
    }
    if path.as_os_str().is_empty() {
        path.push(".");
    }
    path
}

fn quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", c as u32)),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn error(path: &Path, message: impl std::fmt::Display) -> SourceError {
    SourceError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::other(message.to_string()),
    }
}

struct ProjectGuard(PathBuf);

impl ProjectGuard {
    fn acquire(root: &Path) -> Result<Self, SourceError> {
        let path = root.join(".zore-lock.guard");
        OpenOptions::new().write(true).create_new(true).open(&path)
            .map_err(|_| error(&path, "dependency resolution is active or was interrupted; retry, or remove the guard after confirming no resolver is running"))?;
        Ok(Self(path))
    }
}

impl Drop for ProjectGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SourceError> {
    let staging = cache::Staging::new(path.parent().expect("lock root"))?;
    let temporary = staging.0.join("zore.lock");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| error(path, e))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| error(path, e))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|e| error(path, e))
}
