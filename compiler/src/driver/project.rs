//! Finds the packages a program uses: folders of `.ore` files and the bundled standard packages.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::stdlib;
use crate::ast;
use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::{Parsed, parse, parse_standard};
use crate::resolve::{FileUnit, ImportBinding, PackageUnit};
use crate::source::{SourceError, SourceFile, SourceMap, Span};

/// The project's root folder and the name its `zore.toml` gives it.
pub struct Manifest {
    pub root: PathBuf,
    pub name: Option<String>,
}

pub trait FileSystem {
    fn read(&self, path: &Path) -> Result<String, SourceError>;

    /// The `.ore` files directly inside `dir`, sorted by file name.
    fn ore_files(&self, dir: &Path) -> Result<Vec<PathBuf>, SourceError>;

    /// The nearest folder at or above `dir` that holds `zore.toml`.
    fn manifest(&self, dir: &Path) -> Result<Option<Manifest>, SourceError>;

    /// A path that two spellings of one folder share.
    fn canonical(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }
}

pub struct Disk;

fn io_error(path: &Path, source: std::io::Error) -> SourceError {
    SourceError::Io {
        path: path.to_path_buf(),
        source,
    }
}

impl FileSystem for Disk {
    fn read(&self, path: &Path) -> Result<String, SourceError> {
        let bytes = std::fs::read(path).map_err(|source| io_error(path, source))?;
        String::from_utf8(bytes).map_err(|error| SourceError::InvalidUtf8 {
            path: path.to_path_buf(),
            valid_up_to: error.utf8_error().valid_up_to(),
        })
    }

    fn ore_files(&self, dir: &Path) -> Result<Vec<PathBuf>, SourceError> {
        let listing = std::fs::read_dir(dir).map_err(|source| io_error(dir, source))?;
        let mut files: Vec<PathBuf> = listing
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "ore") && path.is_file())
            .collect();
        files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        Ok(files)
    }

    fn manifest(&self, dir: &Path) -> Result<Option<Manifest>, SourceError> {
        let Ok(start) = std::fs::canonicalize(dir) else {
            return Ok(None);
        };
        for folder in start.ancestors() {
            let path = folder.join("zore.toml");
            if path.is_file() {
                return Ok(Some(Manifest {
                    root: folder.to_path_buf(),
                    name: project_name(&self.read(&path)?),
                }));
            }
        }
        Ok(None)
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }
}

/// Files held in memory, for tests.
#[derive(Default)]
pub struct Memory {
    files: BTreeMap<PathBuf, String>,
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        self.files.insert(path.into(), text.into());
        self
    }
}

impl FileSystem for Memory {
    fn read(&self, path: &Path) -> Result<String, SourceError> {
        self.files.get(path).cloned().ok_or_else(|| {
            io_error(
                path,
                std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
            )
        })
    }

    fn ore_files(&self, dir: &Path) -> Result<Vec<PathBuf>, SourceError> {
        Ok(self
            .files
            .keys()
            .filter(|path| {
                path.parent() == Some(dir) && path.extension().is_some_and(|e| e == "ore")
            })
            .cloned()
            .collect())
    }

    fn manifest(&self, dir: &Path) -> Result<Option<Manifest>, SourceError> {
        for folder in dir.ancestors() {
            if let Some(text) = self.files.get(&folder.join("zore.toml")) {
                return Ok(Some(Manifest {
                    root: folder.to_path_buf(),
                    name: project_name(text),
                }));
            }
        }
        Ok(None)
    }
}

/// The `name` of a project file, when it is a valid project name.
fn project_name(text: &str) -> Option<String> {
    let value = text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "name").then(|| value.trim().trim_matches('"').to_string())
    })?;
    let valid = !value.is_empty()
        && value != "zore"
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    valid.then_some(value)
}

pub struct LoadedFile {
    pub file: ast::File,
    pub imports: Vec<ImportBinding>,
}

pub struct LoadedPackage {
    pub path: String,
    pub name: String,
    pub files: Vec<LoadedFile>,
}

/// Every package a program uses, dependencies first and the entry package last.
pub struct Project {
    pub(crate) packages: Vec<LoadedPackage>,
    /// Holds the text of the bundled standard packages.
    pub(crate) std_sources: SourceMap,
}

impl Project {
    pub fn units(&self) -> Vec<PackageUnit<'_>> {
        self.packages
            .iter()
            .map(|package| PackageUnit {
                path: package.path.clone(),
                name: package.name.clone(),
                files: package
                    .files
                    .iter()
                    .map(|loaded| FileUnit {
                        file: &loaded.file,
                        imports: loaded
                            .imports
                            .iter()
                            .map(|binding| ImportBinding {
                                package: binding.package,
                                name: binding.name.clone(),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect()
    }

    pub fn std_sources(&self) -> &SourceMap {
        &self.std_sources
    }
}

pub enum LoadError {
    Source(SourceError),
    Diagnostics(Vec<Diagnostic>),
}

/// Loads one in-memory file as the whole entry package; only standard packages can be imported.
pub fn load_file(file: &SourceFile) -> Result<Project, Vec<Diagnostic>> {
    let memory = Memory::new();
    let mut loader = Loader::new(&memory, None);
    loader.load_package("main".into(), Kind::Entry, vec![parse(file)]);
    loader.finish().map_err(|error| match error {
        LoadError::Diagnostics(diagnostics) => diagnostics,
        LoadError::Source(_) => unreachable!("an in-memory file reads nothing"),
    })
}

/// Loads the folder that holds `entry` as the entry package, and everything it imports.
pub fn load_project(
    sources: &mut SourceMap,
    files: &dyn FileSystem,
    entry: &Path,
) -> Result<Project, LoadError> {
    let dir = match entry.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let manifest = files.manifest(&dir).map_err(LoadError::Source)?;
    let mut paths = files.ore_files(&dir).map_err(LoadError::Source)?;
    if !paths
        .iter()
        .any(|path| path.file_name() == entry.file_name())
    {
        paths.push(entry.to_path_buf());
    }
    let mut loader = Loader::new(files, Some(sources));
    loader.manifest = manifest;
    loader.entry_dir = Some(files.canonical(&dir));
    let parsed = loader.parse_folder(&paths)?;
    loader.load_package("main".into(), Kind::Entry, parsed);
    loader.finish()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Entry,
    Imported,
}

struct Loader<'a> {
    files: &'a dyn FileSystem,
    sources: Option<&'a mut SourceMap>,
    std_sources: SourceMap,
    manifest: Option<Manifest>,
    entry_dir: Option<PathBuf>,
    packages: Vec<LoadedPackage>,
    /// Import path to package index.
    done: HashMap<String, usize>,
    /// Import paths of the packages being loaded, outermost first.
    stack: Vec<String>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Loader<'a> {
    fn new(files: &'a dyn FileSystem, sources: Option<&'a mut SourceMap>) -> Self {
        Self {
            files,
            sources,
            std_sources: SourceMap::new(),
            manifest: None,
            entry_dir: None,
            packages: Vec::new(),
            done: HashMap::new(),
            stack: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn finish(self) -> Result<Project, LoadError> {
        if !self.diagnostics.is_empty() {
            return Err(LoadError::Diagnostics(self.diagnostics));
        }
        Ok(Project {
            packages: self.packages,
            std_sources: self.std_sources,
        })
    }

    fn parse_folder(&mut self, paths: &[PathBuf]) -> Result<Vec<Parsed>, LoadError> {
        let mut parsed = Vec::new();
        for path in paths {
            let text = self.files.read(path).map_err(LoadError::Source)?;
            let sources = self
                .sources
                .as_deref_mut()
                .expect("folders are loaded into a source map");
            let id = sources.add(path, text).map_err(LoadError::Source)?;
            parsed.push(parse(sources.file(id).expect("just added")));
        }
        Ok(parsed)
    }

    fn load_package(&mut self, path: String, kind: Kind, parsed: Vec<Parsed>) -> usize {
        self.stack.push(path.clone());
        let mut files = Vec::new();
        let mut name: Option<(String, Span)> = None;
        for parsed in parsed {
            let had_errors = !parsed.diagnostics.is_empty();
            self.diagnostics.extend(parsed.diagnostics);
            if let Some(clause) = &parsed.file.package {
                match &name {
                    Some((first, first_span)) if *first != clause.text => {
                        self.diagnostics.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!(
                                    "this file declares package `{}`, but another file in the folder declares `{first}`",
                                    clause.text
                                ),
                                clause.span,
                            )
                            .related(*first_span, "declared here"),
                        );
                    }
                    Some(_) => {}
                    None => name = Some((clause.text.clone(), clause.span)),
                }
            }
            let imports = if had_errors {
                Vec::new()
            } else {
                self.bind_imports(&parsed.file)
            };
            files.push(LoadedFile {
                file: parsed.file,
                imports,
            });
        }
        let package_name = name.map_or_else(String::new, |(name, span)| {
            self.check_package_name(&path, kind, &name, span);
            name
        });
        self.stack.pop();
        let index = self.packages.len();
        self.packages.push(LoadedPackage {
            path: path.clone(),
            name: package_name,
            files,
        });
        self.done.insert(path, index);
        index
    }

    fn check_package_name(&mut self, path: &str, kind: Kind, name: &str, span: Span) {
        if kind == Kind::Entry {
            return;
        }
        let last = path.rsplit('/').next().unwrap_or(path);
        if name != last {
            self.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!(
                        "package `{name}` is imported as `{path}`, so it must be named `{last}`"
                    ),
                    span,
                )
                .note("a package's name is the last segment of its import path"),
            );
        }
    }

    /// Resolves each import of a file to a loaded package.
    fn bind_imports(&mut self, file: &ast::File) -> Vec<ImportBinding> {
        let mut bindings = Vec::new();
        for import in &file.imports {
            match self.load_import(&import.path, import.path_span) {
                Some(package) => bindings.push(ImportBinding {
                    package,
                    name: import.path.rsplit('/').next().unwrap_or("").to_string(),
                }),
                None => bindings.push(ImportBinding {
                    package: usize::MAX,
                    name: String::new(),
                }),
            }
        }
        bindings
    }

    fn import_error(&mut self, message: String, span: Span) -> Option<usize> {
        self.diagnostics
            .push(Diagnostic::new(Severity::Error, message, span));
        None
    }

    fn load_import(&mut self, path: &str, span: Span) -> Option<usize> {
        let valid = !path.is_empty()
            && path.split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            });
        if !valid {
            return self.import_error(format!("`{path}` is not a valid import path"), span);
        }
        if let Some(position) = self.stack.iter().position(|visiting| visiting == path) {
            let mut chain = self.stack[position..].to_vec();
            chain.push(path.to_string());
            return self.import_error(format!("import cycle: {}", chain.join(" -> ")), span);
        }
        if let Some(&index) = self.done.get(path) {
            return Some(index);
        }
        let (first, rest) = path.split_once('/').unwrap_or((path, ""));
        if first == "zore" {
            return self.load_standard(path, rest, span);
        }
        let Some(manifest) = &self.manifest else {
            return self.import_error(
                format!("cannot import `{path}`: no `zore.toml` project file was found"),
                span,
            );
        };
        let Some(project) = manifest.name.clone() else {
            return self.import_error(
                format!("cannot import `{path}`: `zore.toml` has no valid `name`"),
                span,
            );
        };
        if first != project {
            return self.import_error(
                format!(
                    "package `{path}` was not found: import paths start with `{project}` or `zore`"
                ),
                span,
            );
        }
        if rest.is_empty() {
            return self.import_error(
                format!("`{path}` names the project, not a package inside it"),
                span,
            );
        }
        let dir = manifest.root.join(rest);
        if self.entry_dir.as_deref() == Some(self.files.canonical(&dir).as_path()) {
            return self.import_error("the `main` package cannot be imported".into(), span);
        }
        let paths = match self.files.ore_files(&dir) {
            Ok(paths) if !paths.is_empty() => paths,
            _ => {
                return self.import_error(
                    format!(
                        "package `{path}` was not found: the folder `{rest}` has no `.ore` files"
                    ),
                    span,
                );
            }
        };
        let parsed = match self.parse_folder(&paths) {
            Ok(parsed) => parsed,
            Err(LoadError::Source(error)) => {
                return self.import_error(error.to_string(), span);
            }
            Err(LoadError::Diagnostics(_)) => return None,
        };
        Some(self.load_package(path.to_string(), Kind::Imported, parsed))
    }

    fn load_standard(&mut self, path: &str, name: &str, span: Span) -> Option<usize> {
        let Some(files) = stdlib::package(name) else {
            return self.import_error(format!("no standard package `{path}`"), span);
        };
        let mut parsed = Vec::new();
        for (file_name, text) in files {
            let id = self
                .std_sources
                .add(format!("{path}/{file_name}"), (*text).to_string())
                .expect("bundled sources are small");
            parsed.push(parse_standard(
                self.std_sources.file(id).expect("just added"),
            ));
        }
        Some(self.load_package(path.to_string(), Kind::Imported, parsed))
    }
}
