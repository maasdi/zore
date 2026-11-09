#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::collections::HashMap;
use std::ffi::{CString, c_char, c_int, c_void};
use std::fs::{File, Permissions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path};

#[cfg(target_os = "linux")]
const DIRECTORY: c_int = 0o200000;
#[cfg(target_os = "linux")]
const NOFOLLOW: c_int = 0o400000;
#[cfg(target_os = "linux")]
const CLOEXEC: c_int = 0o2000000;
#[cfg(target_os = "linux")]
const CREATE_EXCLUSIVE: c_int = 0o100 | 0o200;
#[cfg(target_os = "linux")]
const REMOVE_DIR: c_int = 0x200;
#[cfg(target_os = "linux")]
const STAT_NOFOLLOW: c_int = 0x100;
#[cfg(target_os = "macos")]
const DIRECTORY: c_int = 0x100000;
#[cfg(target_os = "macos")]
const NOFOLLOW: c_int = 0x100;
#[cfg(target_os = "macos")]
const CLOEXEC: c_int = 0x1000000;
#[cfg(target_os = "macos")]
const CREATE_EXCLUSIVE: c_int = 0x200 | 0x800;
#[cfg(target_os = "macos")]
const REMOVE_DIR: c_int = 0x80;
#[cfg(target_os = "macos")]
const STAT_NOFOLLOW: c_int = 0x20;

unsafe extern "C" {
    fn openat(fd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    fn mkdirat(fd: c_int, path: *const c_char, mode: u32) -> c_int;
    fn unlinkat(fd: c_int, path: *const c_char, flags: c_int) -> c_int;
    fn fstatat(fd: c_int, path: *const c_char, stat: *mut c_void, flags: c_int) -> c_int;
    #[cfg(target_os = "linux")]
    fn renameat2(
        old_fd: c_int,
        old: *const c_char,
        new_fd: c_int,
        new: *const c_char,
        flags: u32,
    ) -> c_int;
    #[cfg(target_os = "macos")]
    fn renameatx_np(
        old_fd: c_int,
        old: *const c_char,
        new_fd: c_int,
        new: *const c_char,
        flags: u32,
    ) -> c_int;
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
fn name(value: &str) -> io::Result<CString> {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\']) {
        return Err(invalid("invalid path component"));
    }
    CString::new(value).map_err(|_| invalid("NUL in path"))
}
fn open(parent: &File, child: &CString, flags: c_int) -> io::Result<File> {
    // SAFETY: the directory handle and NUL-terminated child remain valid for the call.
    let fd = unsafe {
        openat(
            parent.as_raw_fd(),
            child.as_ptr(),
            flags | NOFOLLOW | CLOEXEC,
            0o600u32,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a fresh owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn mkdir(parent: &File, child: &CString) -> io::Result<()> {
    // SAFETY: the handle and NUL-terminated child are valid.
    if unsafe { mkdirat(parent.as_raw_fd(), child.as_ptr(), 0o700) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn unlink(parent: &File, child: &CString, directory: bool) -> io::Result<()> {
    // SAFETY: unlinkat removes one child and does not follow its final component.
    if unsafe {
        unlinkat(
            parent.as_raw_fd(),
            child.as_ptr(),
            if directory { REMOVE_DIR } else { 0 },
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn absent(parent: &File, child: &CString) -> io::Result<()> {
    #[repr(C, align(16))]
    struct Stat([u8; 512]);
    let mut stat = Stat([0; 512]);
    // SAFETY: supported 64-bit hosts have smaller stat layouts and alignment at most 16.
    let result = unsafe {
        fstatat(
            parent.as_raw_fd(),
            child.as_ptr(),
            stat.0.as_mut_ptr().cast(),
            STAT_NOFOLLOW,
        )
    };
    if result == 0 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination exists",
        ));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(error)
    }
}
fn identity(file: &File) -> io::Result<(u64, u64)> {
    let meta = file.metadata()?;
    Ok((meta.dev(), meta.ino()))
}
fn parent_path(destination: &str) -> io::Result<(File, CString)> {
    let path = Path::new(destination);
    let basename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| invalid("invalid destination"))?;
    let basename = name(basename)?;
    if destination.ends_with('/') {
        return Err(invalid("destination has trailing slash"));
    }
    let mut parent = File::open(if path.is_absolute() { "/" } else { "." })?;
    for component in path.parent().unwrap_or(Path::new(".")).components() {
        match component {
            Component::RootDir | Component::CurDir => (),
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| invalid("non-UTF-8 parent"))?;
                parent = open(&parent, &name(value)?, DIRECTORY)?;
            }
            _ => return Err(invalid("parent traversal is forbidden")),
        }
    }
    absent(&parent, &basename)?;
    Ok((parent, basename))
}

struct Node {
    path: String,
    directory: bool,
}
pub(super) struct Tree {
    parent: File,
    destination: CString,
    envelope: File,
    envelope_name: CString,
    root: File,
    directories: HashMap<String, (u64, u64)>,
    identities: HashMap<(u64, u64), String>,
    nodes: Vec<Node>,
    limit: usize,
    active: Option<File>,
    published: bool,
    cleaned: bool,
}
impl Tree {
    pub(super) fn create(destination: &str, limit: usize) -> io::Result<Self> {
        let (parent, destination) = parent_path(destination)?;
        let mut selected = None;
        for _ in 0..100 {
            let bytes = super::crypto::random_bytes(16).map_err(|e| invalid(&e))?;
            let text = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let candidate = name(&format!(".zore-stage-{text}"))?;
            match mkdir(&parent, &candidate) {
                Ok(()) => {
                    selected = Some(candidate);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
        let envelope_name = selected.ok_or_else(|| invalid("no staging name available"))?;
        let opened = (|| {
            let envelope = open(&parent, &envelope_name, DIRECTORY)?;
            envelope.set_permissions(Permissions::from_mode(0o700))?;
            mkdir(&envelope, &name("tree")?)?;
            let root = open(&envelope, &name("tree")?, DIRECTORY)?;
            root.set_permissions(Permissions::from_mode(0o700))?;
            Ok::<_, io::Error>((envelope, root))
        })();
        let (envelope, root) = match opened {
            Ok(value) => value,
            Err(original) => {
                if let Ok(envelope) = open(&parent, &envelope_name, DIRECTORY) {
                    let _ = unlink(&envelope, &name("tree")?, true);
                }
                return match unlink(&parent, &envelope_name, true) {
                    Ok(()) => Err(original),
                    Err(cleanup) => Err(invalid(&format!("{original}; cleanup failed: {cleanup}"))),
                };
            }
        };
        Ok(Self {
            parent,
            destination,
            envelope,
            envelope_name,
            root,
            directories: HashMap::new(),
            identities: HashMap::new(),
            nodes: Vec::new(),
            limit,
            active: None,
            published: false,
            cleaned: false,
        })
    }
    fn parts(path: &str) -> io::Result<Vec<&str>> {
        let parts: Vec<_> = path.split('/').collect();
        if path.len() > 1024 || parts.len() > 64 {
            return Err(invalid("local path limit exceeded"));
        }
        for part in &parts {
            name(part)?;
        }
        Ok(parts)
    }
    fn walk(&mut self, parts: &[&str], create: bool) -> io::Result<File> {
        let mut current = self.root.try_clone()?;
        let mut path = String::new();
        for part in parts {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            let child = name(part)?;
            if let Some(expected) = self.directories.get(&path) {
                let next = open(&current, &child, DIRECTORY)?;
                if identity(&next)? != *expected {
                    return Err(invalid("staging directory identity changed"));
                }
                current = next;
                continue;
            }
            if !create {
                return Err(invalid("untracked staging directory"));
            }
            if self.nodes.len() == self.limit {
                return Err(invalid("node limit exceeded"));
            }
            mkdir(&current, &child)?;
            self.nodes.push(Node {
                path: path.clone(),
                directory: true,
            });
            let next = open(&current, &child, DIRECTORY)?;
            next.set_permissions(Permissions::from_mode(0o700))?;
            let id = identity(&next)?;
            if self.identities.insert(id, path.clone()).is_some() {
                return Err(invalid("filesystem name alias"));
            }
            self.directories.insert(path.clone(), id);
            current = next;
        }
        Ok(current)
    }
    pub(super) fn mkdir(&mut self, path: &str) -> io::Result<()> {
        if self.active.is_some() {
            return Err(invalid("an entry file is still open"));
        }
        let parts = Self::parts(path)?;
        self.walk(&parts, true).map(drop)
    }
    pub(super) fn create_file(&mut self, path: &str) -> io::Result<()> {
        if self.active.is_some() {
            return Err(invalid("an entry file is still open"));
        }
        let parts = Self::parts(path)?;
        let parent = self.walk(&parts[..parts.len() - 1], true)?;
        if self.nodes.len() == self.limit {
            return Err(invalid("node limit exceeded"));
        }
        let file = open(
            &parent,
            &name(parts[parts.len() - 1])?,
            1 | CREATE_EXCLUSIVE,
        )?;
        self.nodes.push(Node {
            path: path.into(),
            directory: false,
        });
        file.set_permissions(Permissions::from_mode(0o600))?;
        self.active = Some(file);
        Ok(())
    }
    pub(super) fn file(&mut self) -> io::Result<&mut File> {
        self.active
            .as_mut()
            .ok_or_else(|| invalid("no active file"))
    }
    pub(super) fn finish_file(&mut self) -> io::Result<()> {
        let file = self
            .active
            .take()
            .ok_or_else(|| invalid("no active file"))?;
        let synced = file.sync_all();
        let closed = super::os::close_reporting(file);
        synced.and(closed)
    }
    pub(super) fn commit(&mut self) -> io::Result<()> {
        if self.active.is_some() {
            return Err(invalid("an entry file is still open"));
        }
        #[cfg(target_os = "linux")]
        // SAFETY: both names are single components in live anchored directory handles.
        let result = unsafe {
            renameat2(
                self.envelope.as_raw_fd(),
                c"tree".as_ptr(),
                self.parent.as_raw_fd(),
                self.destination.as_ptr(),
                1,
            )
        };
        #[cfg(target_os = "macos")]
        // SAFETY: RENAME_EXCL atomically refuses an existing destination.
        let result = unsafe {
            renameatx_np(
                self.envelope.as_raw_fd(),
                c"tree".as_ptr(),
                self.parent.as_raw_fd(),
                self.destination.as_ptr(),
                4,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        self.published = true;
        self.nodes.clear();
        self.directories.clear();
        self.identities.clear();
        self.abort().map_err(|error| {
            invalid(&format!(
                "tree published; staging envelope cleanup failed: {error}"
            ))
        })
    }
    pub(super) fn abort(&mut self) -> io::Result<()> {
        if self.cleaned {
            return Ok(());
        }
        let close_error = self
            .active
            .take()
            .and_then(|file| super::os::close_reporting(file).err());
        while let Some(node) = self.nodes.last() {
            let path = node.path.clone();
            let directory = node.directory;
            let parts = Self::parts(&path)?;
            let parent = self.walk(&parts[..parts.len() - 1], false)?;
            unlink(&parent, &name(parts[parts.len() - 1])?, directory)?;
            if let Some(id) = self.directories.remove(&path) {
                self.identities.remove(&id);
            }
            self.nodes.pop();
        }
        if !self.published {
            unlink(&self.envelope, &name("tree")?, true)?;
        }
        unlink(&self.parent, &self.envelope_name, true)?;
        self.cleaned = true;
        match close_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = self.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::symlink;

    struct Sandbox(std::path::PathBuf);
    impl Sandbox {
        fn new() -> Self {
            let nonce = super::super::crypto::random_bytes(16).unwrap();
            let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
            let root = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("zore-tree-{suffix}"));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn destination(&self) -> String {
            self.0.join("result").to_str().unwrap().into()
        }
        fn children(&self) -> usize {
            std::fs::read_dir(&self.0).unwrap().count()
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn publication_and_private_permissions() {
        let root = Sandbox::new();
        let mut tree = Tree::create(&root.destination(), 3).unwrap();
        tree.create_file("a/b.txt").unwrap();
        tree.file().unwrap().write_all(b"hello").unwrap();
        assert!(!Path::new(&root.destination()).exists());
        assert_eq!(
            tree.file().unwrap().metadata().unwrap().mode() & 0o777,
            0o600
        );
        tree.finish_file().unwrap();
        tree.mkdir("a").unwrap();
        tree.commit().unwrap();
        assert_eq!(
            std::fs::read(root.0.join("result/a/b.txt")).unwrap(),
            b"hello"
        );
        assert_eq!(
            std::fs::metadata(root.0.join("result")).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(root.children(), 1);
    }
    #[test]
    fn failed_commit_and_drop_remove_staging() {
        let root = Sandbox::new();
        {
            let mut tree = Tree::create(&root.destination(), 3).unwrap();
            tree.create_file("a/b.txt").unwrap();
            assert!(tree.commit().is_err());
        }
        assert_eq!(root.children(), 0);
        let mut tree = Tree::create(&root.destination(), 3).unwrap();
        tree.mkdir("a/b").unwrap();
        std::fs::write(root.0.join("result"), b"untouched").unwrap();
        assert!(tree.commit().is_err());
        tree.abort().unwrap();
        assert_eq!(std::fs::read(root.0.join("result")).unwrap(), b"untouched");
        assert_eq!(root.children(), 1);
    }
    #[test]
    fn rejects_links_traversal_and_existing_destinations() {
        let root = Sandbox::new();
        let outside = Sandbox::new();
        symlink(&outside.0, root.0.join("link")).unwrap();
        assert!(Tree::create(root.0.join("link/result").to_str().unwrap(), 3).is_err());
        assert!(Tree::create(root.0.join("../escape").to_str().unwrap(), 3).is_err());
        symlink(outside.0.join("missing"), root.0.join("result")).unwrap();
        assert!(Tree::create(&root.destination(), 3).is_err());
        std::fs::remove_file(root.0.join("result")).unwrap();
        let mut tree = Tree::create(&root.destination(), 3).unwrap();
        for name in ["../escape", "/absolute", "a//b", "a/./b", "a\\b", "a\0b"] {
            assert!(tree.create_file(name).is_err(), "{name:?}");
        }
        let staging = root
            .0
            .join(tree.envelope_name.to_str().unwrap())
            .join("tree");
        symlink(&outside.0, staging.join("alias")).unwrap();
        assert!(tree.create_file("alias/escape").is_err());
        std::fs::remove_file(staging.join("alias")).unwrap();
        tree.abort().unwrap();
        assert_eq!(outside.children(), 0);
    }
    #[test]
    fn parent_replacement_cannot_redirect_writes_or_publication() {
        let root = Sandbox::new();
        let outside = Sandbox::new();
        std::fs::create_dir(root.0.join("parent")).unwrap();
        let destination = root.0.join("parent/result");
        let mut tree = Tree::create(destination.to_str().unwrap(), 3).unwrap();
        std::fs::rename(root.0.join("parent"), root.0.join("moved")).unwrap();
        symlink(&outside.0, root.0.join("parent")).unwrap();
        tree.create_file("file").unwrap();
        tree.file().unwrap().write_all(b"anchored").unwrap();
        tree.finish_file().unwrap();
        tree.commit().unwrap();
        assert_eq!(
            std::fs::read(root.0.join("moved/result/file")).unwrap(),
            b"anchored"
        );
        assert_eq!(outside.children(), 0);
    }
    #[test]
    fn node_depth_and_collision_failures_clean_up() {
        let root = Sandbox::new();
        let mut tree = Tree::create(&root.destination(), 2).unwrap();
        assert!(tree.create_file("a/b/c").is_err());
        tree.abort().unwrap();
        assert_eq!(root.children(), 0);
        let mut tree = Tree::create(&root.destination(), 10).unwrap();
        assert!(tree.mkdir(&vec!["a"; 65].join("/")).is_err());
        tree.create_file("a").unwrap();
        tree.finish_file().unwrap();
        assert!(tree.mkdir("a/b").is_err());
        assert!(tree.create_file("a").is_err());
        tree.abort().unwrap();
        assert_eq!(root.children(), 0);
    }
}
