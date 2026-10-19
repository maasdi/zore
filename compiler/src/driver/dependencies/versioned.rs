use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use super::lock::{Binding, Bindings, ReleaseKey};
use super::{
    GitProject, Mode, Options, atomic_write, cache, digest, error, lock, lockfile, portable,
};
use crate::driver::project::manifest::resolve_roots;
use crate::driver::project::{Dependency, Disk, FileSystem, Manifest};
use crate::driver::release::{self, Version};
use crate::source::SourceError;

#[derive(Clone)]
struct Request {
    manifest: Manifest,
    source: Option<GitProject>,
    version: Option<Version>,
    trace: String,
}

#[derive(Clone, Eq, PartialEq)]
enum Identity {
    Local(PathBuf),
    Git(String, String),
}

impl Request {
    fn identity(&self) -> Result<Identity, SourceError> {
        Ok(match &self.source {
            Some(source) => Identity::Git(
                source.url.clone(),
                portable(
                    self.manifest
                        .root
                        .strip_prefix(&source.checkout)
                        .map_err(|_| {
                            error(&self.manifest.root, "project escapes its Git checkout")
                        })?,
                )?,
            ),
            None => Identity::Local(self.manifest.root.clone()),
        })
    }
}

struct Requirements {
    identity: Identity,
    first: Request,
    minimum: Option<Request>,
    pin: Option<Request>,
}

impl Requirements {
    fn add(&mut self, request: Request) -> Result<(), SourceError> {
        let conflict = |kind: &str, previous: &Request| {
            error(
                &request.manifest.root.join("zore.toml"),
                format!(
                    "conflicting project `{}` ({kind}): {} conflicts with {}",
                    request.manifest.name, previous.trace, request.trace
                ),
            )
        };
        if request.identity()? != self.identity {
            let kind = if matches!(&self.identity, Identity::Local(_))
                != matches!(request.identity()?, Identity::Local(_))
            {
                "conflicting local/Git source selections"
            } else {
                "conflicting source identities"
            };
            return Err(conflict(kind, &self.first));
        }
        if let Some(version) = &request.version {
            if let Some(previous) = &self.minimum {
                let prior = previous.version.as_ref().expect("versioned minimum");
                if !version.compatible_with(prior) {
                    return Err(conflict("incompatible version lines", previous));
                }
                if version <= prior {
                    return Ok(());
                }
            }
            self.minimum = Some(request);
        } else {
            if let Some(previous) = &self.pin
                && previous.source.as_ref().map(|source| &source.revision)
                    != request.source.as_ref().map(|source| &source.revision)
            {
                return Err(conflict("conflicting hard pins", previous));
            }
            self.pin = Some(request);
        }
        Ok(())
    }

    fn selected(&self) -> Result<Request, SourceError> {
        if let (Some(minimum), Some(pin)) = (&self.minimum, &self.pin)
            && minimum.source.as_ref().map(|source| &source.revision)
                != pin.source.as_ref().map(|source| &source.revision)
        {
            return Err(error(
                &minimum.manifest.root.join("zore.toml"),
                format!(
                    "hard pin disagrees with selected minimum for `{}`: {} conflicts with {}; align the pin and version requirements",
                    minimum.manifest.name, pin.trace, minimum.trace
                ),
            ));
        }
        Ok(self
            .minimum
            .as_ref()
            .or(self.pin.as_ref())
            .expect("collected requirement")
            .clone())
    }
}

struct Resolver {
    options: Options,
    has_lock: bool,
    known: Bindings,
    releases: Bindings,
    fetched: HashMap<(String, String), GitProject>,
}

impl Resolver {
    fn pinned(&mut self, url: &str, revision: &str) -> Result<GitProject, SourceError> {
        let key = (url.to_string(), revision.to_string());
        if let Some(source) = self.fetched.get(&key) {
            return Ok(source.clone());
        }
        let (checkout, tree) = cache::select(url, revision, self.options)?;
        let source = GitProject {
            url: url.into(),
            revision: revision.into(),
            tree,
            checkout,
        };
        self.fetched.insert(key, source.clone());
        Ok(source)
    }

    fn release(&mut self, key: &ReleaseKey, trace: &str) -> Result<Request, SourceError> {
        if let Some(binding) = self.releases.get(key).cloned() {
            let source = self.pinned(&key.url, &binding.revision)?;
            return request(
                &key.name,
                &source.checkout.join(&key.subdir),
                Some(source.clone()),
                Some(key.version.clone()),
                trace.into(),
            );
        }
        if self.options.mode == Mode::Build && !self.has_lock {
            return Err(error(
                Path::new("zore.lock"),
                "versioned dependencies require a matching lockfile; run `zore install` first",
            ));
        }
        let cached = cached_binding(key)?;
        let locked = self.known.get(key).cloned();
        if self.has_lock && self.options.mode != Mode::Lock && locked.is_none() {
            return Err(error(
                Path::new("zore.lock"),
                "missing or mismatched release provenance; run `zore lock` after reviewing manifest changes",
            ));
        }
        if let (Some(locked), Some(cached)) = (&locked, &cached)
            && locked != cached
        {
            return Err(error(
                &binding_path(key)?,
                "corrupt release binding conflicts with zore.lock; remove this binding cache entry and retry",
            ));
        }
        let known = locked.or(cached);
        let source = if self.has_lock && self.options.mode != Mode::Lock {
            self.pinned(&key.url, &known.as_ref().expect("locked release").revision)?
        } else if self.options.offline || self.options.mode == Mode::Build {
            let known = known.as_ref().ok_or_else(|| {
                error(
                    Path::new("zore.lock"),
                    format!(
                        "release binding `{}` is missing; run `zore install` online first",
                        key.tag()
                    ),
                )
            })?;
            self.pinned(&key.url, &known.revision)?
        } else {
            let (revision, checkout, tree) = cache::tagged(
                &key.url,
                &key.tag(),
                known.as_ref().map(|binding| binding.revision.as_str()),
            )?;
            let source = GitProject {
                url: key.url.clone(),
                revision,
                tree,
                checkout,
            };
            self.fetched
                .insert((key.url.clone(), source.revision.clone()), source.clone());
            source
        };
        let root = release_root(&source.checkout, &key.subdir)?;
        let node = request(
            &key.name,
            &root,
            Some(source.clone()),
            Some(key.version.clone()),
            trace.into(),
        )?;
        release::validate(&Disk, &node.manifest, Some(&key.tag()))?;
        let native = node.manifest.native.as_ref().map(|id| {
            let adapter = crate::driver::native::find(id).expect("validated adapter");
            (id.clone(), adapter.fingerprint.to_string())
        });
        let actual = Binding {
            key: key.clone(),
            revision: source.revision,
            tree: source.tree,
            manifest: digest(node.manifest.text.as_bytes()),
            native,
        };
        if known.as_ref().is_some_and(|known| known != &actual) {
            return Err(error(
                &node.manifest.root,
                format!(
                    "release identity conflict for `{}`: cached source, manifest or native recipe differs from its known binding",
                    key.tag()
                ),
            ));
        }
        self.releases.insert(key.clone(), actual);
        Ok(node)
    }

    fn dependency(
        &mut self,
        node: &Request,
        name: &str,
        dependency: &Dependency,
    ) -> Result<Request, SourceError> {
        let trace = format!("{} -> {name}", node.trace);
        match dependency {
            Dependency::Release {
                url,
                version,
                subdir,
            } => {
                let key = ReleaseKey {
                    name: name.into(),
                    url: url.clone(),
                    subdir: subdir.clone(),
                    version: version.clone(),
                };
                self.release(&key, &format!("{trace}@{version}"))
            }
            Dependency::Git {
                url,
                revision,
                subdir,
            } => {
                let source = self.pinned(url, revision)?;
                request(
                    name,
                    &source.checkout.join(subdir),
                    Some(source.clone()),
                    None,
                    format!("{trace}@{revision}"),
                )
            }
            Dependency::Path(path) => {
                let target = Disk.canonical(&node.manifest.root.join(path));
                if let Some(source) = &node.source
                    && (path.is_absolute()
                        || path.has_root()
                        || !target.starts_with(&source.checkout))
                {
                    return Err(error(
                        &node.manifest.root.join("zore.toml"),
                        "a fetched project's path dependency must stay inside its Git checkout",
                    ));
                }
                request(name, &target, node.source.clone(), None, trace)
            }
        }
    }
}

fn release_root(checkout: &Path, subdir: &str) -> Result<PathBuf, SourceError> {
    let mut root = checkout.to_path_buf();
    for part in subdir
        .split('/')
        .filter(|part| !part.is_empty())
        .chain(std::iter::once("zore.toml"))
    {
        let mut found = false;
        for entry in fs::read_dir(&root).map_err(|source| error(&root, source))? {
            if entry.map_err(|source| error(&root, source))?.file_name() == part {
                found = true;
                break;
            }
        }
        if !found {
            return Err(error(
                &root,
                "release subdir must name a project root containing zore.toml with exact Git path spelling",
            ));
        }
        root.push(part);
    }
    root.pop();
    Ok(root)
}

fn request(
    name: &str,
    root: &Path,
    source: Option<GitProject>,
    version: Option<Version>,
    trace: String,
) -> Result<Request, SourceError> {
    let root = Disk.canonical(root);
    if source
        .as_ref()
        .is_some_and(|source| !root.starts_with(&source.checkout))
    {
        return Err(error(&root, "Git subdir escapes its checkout"));
    }
    let manifest = Disk
        .manifest(&root)?
        .filter(|manifest| manifest.root == root)
        .ok_or_else(|| {
            error(
                &root,
                format!("dependency `{name}` path must name a project root containing zore.toml"),
            )
        })?;
    if manifest.name != name {
        return Err(error(
            &root.join("zore.toml"),
            format!(
                "dependency key `{name}` does not match project name `{}`",
                manifest.name
            ),
        ));
    }
    Ok(Request {
        manifest,
        source,
        version,
        trace,
    })
}

pub(super) fn resolve(
    root: Manifest,
    options: Options,
    existing: Option<&str>,
) -> Result<HashMap<String, Manifest>, SourceError> {
    let entry = Disk.canonical(&root.root);
    let mut resolver = Resolver {
        options,
        has_lock: existing.is_some(),
        known: lock::read(existing)?,
        releases: BTreeMap::new(),
        fetched: HashMap::new(),
    };
    let trace = root.name.clone();
    let root = Request {
        manifest: root,
        source: None,
        version: None,
        trace,
    };
    let mut queue = VecDeque::from([root]);
    let mut visited = HashSet::new();
    let mut requirements: BTreeMap<String, Requirements> = BTreeMap::new();
    while let Some(node) = queue.pop_front() {
        let identity = node.identity()?;
        let requirements = requirements
            .entry(node.manifest.name.clone())
            .or_insert_with(|| Requirements {
                identity,
                first: node.clone(),
                minimum: None,
                pin: None,
            });
        requirements.add(node.clone())?;
        if !visited.insert(node.manifest.root.clone()) {
            continue;
        }
        release::validate_metadata(&Disk, &node.manifest)?;
        if node.manifest.native.is_some() {
            crate::driver::native::load(&Disk, &node.manifest)?;
        }
        for (name, dependency) in &node.manifest.dependencies {
            let child = resolver
                .dependency(&node, name, dependency)
                .map_err(|source| {
                    error(
                        &node.manifest.root.join("zore.toml"),
                        format!("dependency path {} -> {name}: {source}", node.trace),
                    )
                })?;
            queue.push_back(child);
        }
    }
    let selected = requirements
        .into_iter()
        .map(|(name, requirements)| requirements.selected().map(|node| (name, node)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let roots = selected
        .values()
        .map(|node| node.manifest.clone())
        .collect();
    let projects = resolve_roots(&Disk, roots, &mut |_, name, _| {
        selected
            .get(name)
            .map(|node| node.manifest.root.clone())
            .ok_or_else(|| error(&entry, "selected dependency is missing"))
    })?;
    let sources = selected
        .values()
        .filter_map(|node| {
            node.source
                .clone()
                .map(|source| (node.manifest.root.clone(), source))
        })
        .collect();
    let versions = selected
        .values()
        .filter_map(|node| {
            node.version
                .clone()
                .map(|version| (node.manifest.root.clone(), version))
        })
        .collect();
    let needed = !resolver.releases.is_empty();
    let current = resolver.releases.clone();
    resolver.known.extend(resolver.releases);
    let needs_lock = options.mode != Mode::Build
        || options.locked
        || existing.is_some()
        || selected
            .values()
            .any(|node| node.source.is_some() || node.manifest.native.is_some());
    let mut replacement = None;
    if needs_lock {
        let expected = lockfile(&entry, &projects, &sources, &versions, &resolver.known)?;
        if options.mode == Mode::Lock
            || (options.mode == Mode::Install && !options.locked && existing.is_none())
        {
            replacement = Some(expected);
        } else if existing != Some(expected.as_str()) {
            return Err(error(
                &entry.join("zore.lock"),
                "missing or mismatched lockfile; run `zore install` if missing, or `zore lock` after reviewing manifest changes",
            ));
        }
    }
    if needed && options.mode != Mode::Build {
        for binding in current.values() {
            publish_binding(binding)?;
        }
    }
    if let Some(expected) = replacement {
        atomic_write(&entry.join("zore.lock"), expected.as_bytes())?;
    }
    Ok(projects)
}

fn binding_path(key: &ReleaseKey) -> Result<PathBuf, SourceError> {
    let identity = format!("{}\0{}\0{}\0{}", key.name, key.url, key.subdir, key.version);
    Ok(cache::cache_root()?
        .join("releases")
        .join(digest(identity.as_bytes())))
}

fn cached_binding(key: &ReleaseKey) -> Result<Option<Binding>, SourceError> {
    let directory = binding_path(key)?;
    match fs::symlink_metadata(&directory) {
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        _ => {
            return Err(error(
                &directory,
                "corrupt release binding cache directory; remove this binding cache entry and retry",
            ));
        }
    }
    let regular = fs::symlink_metadata(directory.join("binding"))
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
    if !regular
        || fs::read_dir(&directory)
            .map_err(|source| error(&directory, source))?
            .count()
            != 1
    {
        return Err(error(
            &directory,
            "corrupt release binding cache files; remove this binding cache entry and retry",
        ));
    }
    match fs::read_to_string(directory.join("binding")) {
        Ok(text) => {
            let binding =
                lock::parse_binding(text.strip_suffix('\n').unwrap_or(&text)).map_err(|_| {
                    error(
                        &directory,
                        "corrupt release binding cache; remove this binding cache entry and retry",
                    )
                })?;
            if binding.key != *key || text != format!("{}\n", lock::render_binding(&binding)) {
                return Err(error(
                    &directory,
                    "corrupt release binding identity; remove this binding cache entry and retry",
                ));
            }
            Ok(Some(binding))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !directory.exists() => {
            Ok(None)
        }
        Err(source) => Err(error(
            &directory,
            format!(
                "corrupt release binding cache: {source}; remove this binding cache entry and retry"
            ),
        )),
    }
}

fn publish_binding(binding: &Binding) -> Result<(), SourceError> {
    if let Some(existing) = cached_binding(&binding.key)? {
        if existing != *binding {
            return Err(error(
                &binding_path(&binding.key)?,
                "release identity conflict in shared cache; publish a new version",
            ));
        }
        return Ok(());
    }
    let path = binding_path(&binding.key)?;
    let parent = path.parent().expect("binding cache root");
    fs::create_dir_all(parent).map_err(|source| error(parent, source))?;
    let staging = cache::Staging::new(parent)?;
    fs::write(
        staging.0.join("binding"),
        format!("{}\n", lock::render_binding(binding)),
    )
    .map_err(|source| error(&path, source))?;
    match fs::rename(&staging.0, &path) {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => {
            if cached_binding(&binding.key)?.as_ref() != Some(binding) {
                return Err(error(&path, "concurrent release identity conflict"));
            }
            Ok(())
        }
        Err(source) => Err(error(&path, source)),
    }
}
