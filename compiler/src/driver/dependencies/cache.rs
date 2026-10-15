use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Mode, Options, digest, error};
use crate::driver::project::manifest::portable_path;
use crate::source::SourceError;

static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);

pub(super) struct Staging(pub PathBuf);

impl Staging {
    pub(super) fn new(parent: &Path) -> Result<Self, SourceError> {
        for _ in 0..100 {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = parent.join(format!(
                ".zore-stage-{}-{nonce}-{}",
                std::process::id(),
                NEXT_STAGING.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(error(parent, e)),
            }
        }
        Err(error(
            parent,
            "could not allocate a unique staging directory",
        ))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        #[cfg(windows)]
        make_writable(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
#[allow(
    clippy::permissions_set_readonly_false,
    reason = "Only Windows read-only file attributes are cleared"
)]
fn make_writable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                make_writable(&entry.path());
            }
        }
    } else if metadata.permissions().readonly() {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
}

fn cache_root() -> Result<PathBuf, SourceError> {
    if let Some(path) = std::env::var_os("ZORE_CACHE_DIR").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
    };
    base.map(|p| p.join("zore/dependencies")).ok_or_else(|| {
        error(
            Path::new("ZORE_CACHE_DIR"),
            "set ZORE_CACHE_DIR to a writable dependency cache directory",
        )
    })
}

pub(super) fn select(
    url: &str,
    revision: &str,
    options: Options,
) -> Result<(PathBuf, String), SourceError> {
    let root = cache_root()?;
    let entry = root.join(digest(format!("{url}\0{revision}").as_bytes()));
    if entry.exists() {
        return validate(&entry, url, revision).map_err(|e| {
            error(
                &entry,
                format!(
                    "corrupt Git cache: {e}; remove this cache entry and run `zore fetch` again"
                ),
            )
        });
    }
    if options.offline || options.mode == Mode::Build {
        return Err(error(
            &entry,
            "Git source is missing from the cache; run `zore fetch` without --offline first",
        ));
    }
    fs::create_dir_all(&root).map_err(|e| error(&root, e))?;
    let staging = Staging::new(&root)?;
    let repository = staging.0.join("repository.git");
    let status = git(None)
        .args(["init", "--bare", "--object-format=sha1"])
        .arg(&repository)
        .output()
        .map_err(|_| {
            error(
                &entry,
                "Git is required; install git and retry `zore fetch`",
            )
        })?;
    if !status.status.success() {
        return Err(error(&entry, "could not initialize Git cache staging"));
    }
    let status = git(Some(&repository))
        .args([
            "fetch",
            "--no-tags",
            "--no-recurse-submodules",
            "--depth=1",
            "--",
            url,
            revision,
        ])
        .output()
        .map_err(|_| error(&entry, "could not start Git fetch"))?;
    if !status.status.success() {
        return Err(error(
            &entry,
            "Git fetch failed; check repository access, your normal Git credentials, and whether the pinned commit is available",
        ));
    }
    fs::write(staging.0.join("source"), format!("{url}\n{revision}\n"))
        .map_err(|e| error(&entry, e))?;
    let (tree, entries) = inventory(&repository, revision)?;
    let checkout = staging.0.join("sources");
    fs::create_dir(&checkout).map_err(|e| error(&entry, e))?;
    blobs(&repository, &checkout, &entries, true)?;
    validate(&staging.0, url, revision)?;
    match fs::rename(&staging.0, &entry) {
        Ok(()) => Ok((
            fs::canonicalize(entry.join("sources")).map_err(|e| error(&entry, e))?,
            tree,
        )),
        Err(_) if entry.exists() => validate(&entry, url, revision),
        Err(e) => Err(error(&entry, e)),
    }
}

fn git(repository: Option<&Path>) -> Command {
    let mut command = Command::new("git");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
    ] {
        command.env_remove(key);
    }
    command
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    command.args([
        "-c",
        "gc.auto=0",
        "-c",
        "maintenance.auto=false",
        "-c",
        "protocol.ext.allow=never",
    ]);
    if let Some(repository) = repository {
        command.arg("--git-dir").arg(repository);
    }
    command.stdin(Stdio::null()).stderr(Stdio::null());
    command
}

fn output(repository: &Path, args: &[&str]) -> Result<Vec<u8>, SourceError> {
    let output = git(Some(repository))
        .args(args)
        .output()
        .map_err(|_| error(repository, "Git is required to validate cached sources"))?;
    if !output.status.success() {
        return Err(error(repository, "Git object validation failed"));
    }
    Ok(output.stdout)
}

#[derive(Clone)]
struct Entry {
    object: String,
    executable: bool,
}

fn inventory(
    repository: &Path,
    revision: &str,
) -> Result<(String, BTreeMap<String, Entry>), SourceError> {
    let kind = output(repository, &["cat-file", "-t", revision])?;
    if kind != b"commit\n" {
        return Err(error(repository, "pinned revision must identify a commit"));
    }
    output(repository, &["fsck", "--strict", "--no-reflogs", revision])?;
    let tree = output(repository, &["rev-parse", &format!("{revision}^{{tree}}")])?;
    let tree = String::from_utf8(tree)
        .map_err(|_| error(repository, "invalid Git tree ID"))?
        .trim()
        .to_string();
    let listing = output(repository, &["ls-tree", "-rz", "--full-tree", revision])?;
    let mut entries = BTreeMap::new();
    for record in listing.split(|c| *c == 0).filter(|s| !s.is_empty()) {
        let text = std::str::from_utf8(record)
            .map_err(|_| error(repository, "Git source paths must be UTF-8"))?;
        let (metadata, path) = text
            .split_once('\t')
            .ok_or_else(|| error(repository, "invalid Git tree entry"))?;
        let parts: Vec<_> = metadata.split(' ').collect();
        if parts.len() != 3 || !matches!(parts[0], "100644" | "100755") || parts[1] != "blob" {
            return Err(error(
                repository,
                "Git dependencies do not support symlinks or submodules",
            ));
        }
        if !portable_path(path) {
            return Err(error(
                repository,
                "Git source has a path that is not portable to Windows",
            ));
        }
        entries.insert(
            path.to_string(),
            Entry {
                object: parts[2].into(),
                executable: parts[0] == "100755",
            },
        );
    }
    let mut names = BTreeMap::new();
    for path in entries.keys() {
        let mut prefix = String::new();
        for part in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if let Some(previous) = names.insert(prefix.to_lowercase(), prefix.clone())
                && previous != prefix
            {
                return Err(error(repository, "Git source has case-colliding paths"));
            }
        }
    }
    Ok((tree, entries))
}

fn validate(entry: &Path, url: &str, revision: &str) -> Result<(PathBuf, String), SourceError> {
    if fs::read_to_string(entry.join("source")).map_err(|e| error(entry, e))?
        != format!("{url}\n{revision}\n")
    {
        return Err(error(
            entry,
            "cached source identity does not match the manifest",
        ));
    }
    let repository = entry.join("repository.git");
    let (tree, entries) = inventory(&repository, revision)?;
    let checkout = entry.join("sources");
    let mut expected = BTreeSet::new();
    for path in entries.keys() {
        let mut current = Path::new(path);
        loop {
            expected.insert(current.to_path_buf());
            let Some(parent) = current.parent().filter(|p| !p.as_os_str().is_empty()) else {
                break;
            };
            current = parent;
        }
    }
    validate_paths(&checkout, &checkout, &expected)?;
    blobs(&repository, &checkout, &entries, false)?;
    Ok((
        fs::canonicalize(&checkout).map_err(|e| error(entry, e))?,
        tree,
    ))
}

fn validate_paths(
    root: &Path,
    path: &Path,
    expected: &BTreeSet<PathBuf>,
) -> Result<(), SourceError> {
    let kind = fs::symlink_metadata(path)
        .map_err(|e| error(path, e))?
        .file_type();
    if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
        return Err(error(
            path,
            "cache sources must be regular files and directories",
        ));
    }
    if path != root && !expected.contains(path.strip_prefix(root).expect("walked child")) {
        return Err(error(path, "unexpected file or directory in Git cache"));
    }
    if kind.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| error(path, e))? {
            validate_paths(root, &entry.map_err(|e| error(path, e))?.path(), expected)?;
        }
    }
    Ok(())
}

fn blobs(
    repository: &Path,
    checkout: &Path,
    entries: &BTreeMap<String, Entry>,
    create: bool,
) -> Result<(), SourceError> {
    let mut child = git(Some(repository))
        .arg("cat-file")
        .arg("--batch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| error(repository, "could not validate Git blobs"))?;
    let mut input = child.stdin.take().expect("piped stdin");
    let mut output = BufReader::new(child.stdout.take().expect("piped stdout"));
    let result = (|| {
        for (name, entry) in entries {
            writeln!(input, "{}", entry.object)
                .and_then(|_| input.flush())
                .map_err(|e| error(repository, e))?;
            let mut header = String::new();
            output
                .read_line(&mut header)
                .map_err(|e| error(repository, e))?;
            let fields: Vec<_> = header.split_whitespace().collect();
            if fields.len() != 3 || fields[0] != entry.object || fields[1] != "blob" {
                return Err(error(repository, "invalid cached Git blob"));
            }
            let size: u64 = fields[2]
                .parse()
                .map_err(|_| error(repository, "invalid cached Git blob size"))?;
            let path = checkout.join(name);
            #[cfg(not(unix))]
            let _ = entry.executable;
            if create {
                fs::create_dir_all(path.parent().expect("blob parent"))
                    .map_err(|e| error(&path, e))?;
                let mut file = File::create(&path).map_err(|e| error(&path, e))?;
                let copied = std::io::copy(&mut (&mut output).take(size), &mut file)
                    .map_err(|e| error(&path, e))?;
                if copied != size {
                    return Err(error(&path, "truncated Git blob"));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(
                        &path,
                        fs::Permissions::from_mode(if entry.executable { 0o755 } else { 0o644 }),
                    )
                    .map_err(|e| error(&path, e))?;
                }
            } else {
                if !fs::symlink_metadata(&path)
                    .map_err(|e| error(&path, e))?
                    .is_file()
                {
                    return Err(error(&path, "cached blob must be a regular file"));
                }
                let mut file = File::open(&path).map_err(|e| error(&path, e))?;
                if file.metadata().map_err(|e| error(&path, e))?.len() != size {
                    return Err(error(
                        &path,
                        "cached file size differs from pinned Git blob",
                    ));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if (file
                        .metadata()
                        .map_err(|e| error(&path, e))?
                        .permissions()
                        .mode()
                        & 0o111
                        != 0)
                        != entry.executable
                    {
                        return Err(error(
                            &path,
                            "cached executable mode differs from pinned Git tree",
                        ));
                    }
                }
                let mut remaining = size;
                let mut expected = [0; 16384];
                let mut actual = [0; 16384];
                while remaining > 0 {
                    let count = remaining.min(expected.len() as u64) as usize;
                    output
                        .read_exact(&mut expected[..count])
                        .and_then(|_| file.read_exact(&mut actual[..count]))
                        .map_err(|e| error(&path, e))?;
                    if expected[..count] != actual[..count] {
                        return Err(error(
                            &path,
                            "cached file bytes differ from pinned Git blob",
                        ));
                    }
                    remaining -= count as u64;
                }
            }
            let mut newline = [0];
            output
                .read_exact(&mut newline)
                .map_err(|e| error(repository, e))?;
            if newline != *b"\n" {
                return Err(error(repository, "invalid Git blob terminator"));
            }
        }
        Ok(())
    })();
    drop(input);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| error(repository, e))?;
    result?;
    if !status.success() {
        return Err(error(repository, "Git blob validation failed"));
    }
    Ok(())
}
