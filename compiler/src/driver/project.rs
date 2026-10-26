//! Finds the packages a program uses: folders of `.ore` files and the bundled standard packages.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::ast;
use crate::async_lowering::NativeWait;
use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::{Parsed, parse, parse_standard};
use crate::resolve::{FileUnit, ImportBinding, PackageUnit};
use crate::source::{SourceError, SourceFile, SourceMap, Span};
use crate::stdlib;

pub(crate) mod manifest;
mod string_methods;
pub use manifest::{Dependency, Manifest};

pub trait FileSystem {
    fn read(&self, path: &Path) -> Result<String, SourceError>;

    /// Reads a regular UTF-8 file, rejecting directories and special files.
    fn read_regular(&self, path: &Path) -> Result<String, SourceError> {
        self.read(path)
    }

    /// The `.ore` files directly inside `dir`, sorted by file name.
    fn ore_files(&self, dir: &Path) -> Result<Vec<PathBuf>, SourceError>;

    /// The nearest folder at or above `dir` that holds `zore.toml`.
    fn manifest(&self, dir: &Path) -> Result<Option<Manifest>, SourceError>;

    fn require_lockfile(&self) -> bool {
        false
    }

    fn projects(&self, root: Manifest) -> Result<HashMap<String, Manifest>, SourceError>;

    /// A path that two spellings of one folder share.
    fn canonical(&self, path: &Path) -> PathBuf {
        normalized(path)
    }
}

pub struct Disk;

pub(crate) fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.as_encoded_bytes().ends_with(b"_test.ore"))
}

fn io_error(path: &Path, source: std::io::Error) -> SourceError {
    SourceError::Io {
        path: path.to_path_buf(),
        source,
    }
}

impl FileSystem for Disk {
    fn read_regular(&self, path: &Path) -> Result<String, SourceError> {
        let metadata = std::fs::metadata(path).map_err(|source| io_error(path, source))?;
        if !metadata.is_file() {
            return Err(io_error(
                path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "metadata path must name a regular file",
                ),
            ));
        }
        self.read(path)
    }

    fn projects(&self, root: Manifest) -> Result<HashMap<String, Manifest>, SourceError> {
        super::dependencies::resolve(root, Default::default())
    }

    fn read(&self, path: &Path) -> Result<String, SourceError> {
        let bytes = std::fs::read(path).map_err(|source| io_error(path, source))?;
        String::from_utf8(bytes).map_err(|error| SourceError::InvalidUtf8 {
            path: path.to_path_buf(),
            valid_up_to: error.utf8_error().valid_up_to(),
        })
    }

    fn ore_files(&self, dir: &Path) -> Result<Vec<PathBuf>, SourceError> {
        let listing = std::fs::read_dir(dir).map_err(|source| io_error(dir, source))?;
        let mut files = Vec::new();
        for entry in listing {
            let entry = entry.map_err(|source| io_error(dir, source))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|source| io_error(&path, source))?;
            if kind.is_file() && path.extension().is_some_and(|ext| ext == "ore") {
                files.push(path);
            }
        }
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
                return Manifest::parse(folder.to_path_buf(), &self.read(&path)?).map(Some);
            }
        }
        Ok(None)
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| normalized(path))
    }
}

/// Files held in memory, for tests.
#[derive(Default)]
pub struct Memory {
    files: BTreeMap<PathBuf, (PathBuf, String)>,
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        let path = path.into();
        self.files.insert(normalized(&path), (path, text.into()));
        self
    }
}

impl FileSystem for Memory {
    fn projects(&self, root: Manifest) -> Result<HashMap<String, Manifest>, SourceError> {
        manifest::resolve(self, root)
    }

    fn read(&self, path: &Path) -> Result<String, SourceError> {
        self.files
            .get(&normalized(path))
            .map(|(_, text)| text.clone())
            .ok_or_else(|| {
                io_error(
                    path,
                    std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
                )
            })
    }

    fn ore_files(&self, dir: &Path) -> Result<Vec<PathBuf>, SourceError> {
        let dir = normalized(dir);
        Ok(self
            .files
            .iter()
            .filter(|(path, _)| {
                path.parent() == Some(dir.as_path()) && path.extension().is_some_and(|e| e == "ore")
            })
            .map(|(_, (path, _))| path.clone())
            .collect())
    }

    fn manifest(&self, dir: &Path) -> Result<Option<Manifest>, SourceError> {
        let dir = normalized(dir);
        for folder in dir.ancestors() {
            if let Some((_, text)) = self.files.get(&folder.join("zore.toml")) {
                return Manifest::parse(folder.to_path_buf(), text).map(Some);
            }
        }
        Ok(None)
    }
}

fn normalized(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(result.components().next_back(), Some(Component::Normal(_))) =>
            {
                result.pop();
            }
            Component::ParentDir if result.has_root() => {}
            _ => result.push(component.as_os_str()),
        }
    }
    result
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

pub(crate) type NativeBinding = (&'static str, Option<NativeWait>);

/// Every package a program uses, dependencies first and the entry package last.
pub struct Project {
    pub(crate) packages: Vec<LoadedPackage>,
    pub(super) native_adapters: super::native::Providers,
    pub(crate) native_bindings: HashMap<Span, NativeBinding>,
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

    pub(crate) fn add_test_runner(
        &mut self,
        sources: &mut SourceMap,
        text: String,
        entry_name: &str,
    ) -> Result<(), LoadError> {
        let id = sources
            .add("<zore test runner>", text)
            .map_err(LoadError::Source)?;
        let mut parsed = parse(sources.file(id).expect("added test runner"));
        if !parsed.diagnostics.is_empty() {
            return Err(LoadError::Diagnostics(parsed.diagnostics));
        }
        for item in &mut parsed.file.items {
            if let ast::Item::Func(function) = item {
                function.name.text = entry_name.to_string();
                hide_test_imports(&mut function.body);
            }
        }
        let mut loader = Loader::new(&Disk, Some(sources));
        loader.packages = std::mem::take(&mut self.packages);
        loader.std_sources = std::mem::take(&mut self.std_sources);
        loader.native_adapters = std::mem::take(&mut self.native_adapters);
        loader.native_bindings = std::mem::take(&mut self.native_bindings);
        let mut entry = loader.packages.pop().expect("a loaded entry package");
        loader.done = loader
            .packages
            .iter()
            .enumerate()
            .map(|(index, package)| (package.path.clone(), index))
            .collect();
        let mut imports = loader.bind_imports(&parsed.file, None);
        for import in &mut imports {
            import.name = format!("$test_{}", import.name);
        }
        entry.files.push(LoadedFile {
            file: parsed.file,
            imports,
        });
        loader.packages.push(entry);
        *self = loader.finish()?;
        Ok(())
    }
}

fn hide_test_imports(block: &mut ast::Block) {
    for statement in &mut block.stmts {
        match &mut statement.kind {
            ast::StmtKind::Binding(binding) => hide_test_import_expression(&mut binding.value),
            ast::StmtKind::Expr(expression) => hide_test_import_expression(expression),
            ast::StmtKind::Return(expressions) => {
                for expression in expressions {
                    hide_test_import_expression(expression);
                }
            }
            ast::StmtKind::If(branch) => {
                hide_test_import_expression(&mut branch.condition);
                hide_test_imports(&mut branch.then_block);
                assert!(
                    branch.else_branch.is_none(),
                    "the dispatcher has no else branches"
                );
            }
            _ => unreachable!("unexpected generated dispatcher statement"),
        }
    }
}

fn hide_test_import_expression(expression: &mut ast::Expr) {
    match &mut expression.kind {
        ast::ExprKind::Name(name) if name == "os" || name == "errors" => {
            *name = format!("$test_{name}");
        }
        ast::ExprKind::Call { callee, args } => {
            hide_test_import_expression(callee);
            for argument in args {
                hide_test_import_expression(argument);
            }
        }
        ast::ExprKind::Field { base, .. } => hide_test_import_expression(base),
        ast::ExprKind::Binary { lhs, rhs, .. } => {
            hide_test_import_expression(lhs);
            hide_test_import_expression(rhs);
        }
        ast::ExprKind::Index { base, index } => {
            hide_test_import_expression(base);
            hide_test_import_expression(index);
        }
        ast::ExprKind::Name(_)
        | ast::ExprKind::String(_)
        | ast::ExprKind::Int(_)
        | ast::ExprKind::Nil => {}
        _ => unreachable!("unexpected generated dispatcher expression"),
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
    loader.load_package("main".into(), Kind::Entry, vec![parse(file)], None);
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
    if is_test_file(entry) {
        return Err(LoadError::Source(io_error(
            entry,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "use `zore test` for _test.ore files",
            ),
        )));
    }
    load_package_project(sources, files, entry, false)
}

pub(crate) fn load_test_project(
    sources: &mut SourceMap,
    files: &dyn FileSystem,
    entry: &Path,
) -> Result<Project, LoadError> {
    load_package_project(sources, files, entry, true)
}

fn load_package_project(
    sources: &mut SourceMap,
    files: &dyn FileSystem,
    entry: &Path,
    include_tests: bool,
) -> Result<Project, LoadError> {
    let dir = match entry.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let manifest = files.manifest(&dir).map_err(LoadError::Source)?;
    if manifest.is_none() && files.require_lockfile() {
        return Err(LoadError::Source(io_error(
            &dir.join("zore.lock"),
            std::io::Error::other(
                "--locked requires a project with zore.toml and zore.lock; run `zore install` first",
            ),
        )));
    }
    let mut paths = files.ore_files(&dir).map_err(LoadError::Source)?;
    if !include_tests {
        paths.retain(|path| !is_test_file(path));
    }
    if !paths
        .iter()
        .any(|path| path.file_name() == entry.file_name())
    {
        paths.push(entry.to_path_buf());
    }
    let mut loader = Loader::new(files, Some(sources));
    let owner = manifest.as_ref().map(|manifest| manifest.name.clone());
    if let Some(manifest) = manifest {
        loader.projects = files.projects(manifest).map_err(LoadError::Source)?;
    }
    loader.entry_dir = Some(files.canonical(&dir));
    let parsed = loader.parse_folder(&paths, owner.as_deref())?;
    loader.load_package("main".into(), Kind::Entry, parsed, owner.as_deref());
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
    projects: HashMap<String, Manifest>,
    directories: HashMap<PathBuf, String>,
    entry_dir: Option<PathBuf>,
    packages: Vec<LoadedPackage>,
    native_adapters: super::native::Providers,
    native_bindings: HashMap<Span, NativeBinding>,
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
            projects: HashMap::new(),
            directories: HashMap::new(),
            entry_dir: None,
            packages: Vec::new(),
            native_adapters: BTreeMap::new(),
            native_bindings: HashMap::new(),
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
            native_adapters: self.native_adapters,
            native_bindings: self.native_bindings,
            std_sources: self.std_sources,
        })
    }

    fn parse_folder(
        &mut self,
        paths: &[PathBuf],
        owner: Option<&str>,
    ) -> Result<Vec<Parsed>, LoadError> {
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
        if let Some(project) = owner.and_then(|name| self.projects.get(name))
            && paths
                .first()
                .and_then(|path| path.parent())
                .is_some_and(|dir| self.files.canonical(dir) == project.root)
            && let Some(id) = &project.native
        {
            let provider = super::native::load(self.files, project).map_err(LoadError::Source)?;
            let adapter = provider.adapter;
            let sources = self.sources.as_deref_mut().expect("project source map");
            let file = sources
                .add(
                    format!("<native {}>", adapter.id),
                    provider.bindings.clone(),
                )
                .map_err(LoadError::Source)?;
            let bindings = parse_standard(sources.file(file).expect("native bindings"));
            for item in &bindings.file.items {
                if let ast::Item::Func(function) = item {
                    let (_, symbol, wait) = adapter
                        .symbols
                        .iter()
                        .find(|(name, _, _)| *name == function.name.text)
                        .expect("registered binding symbol");
                    self.native_bindings
                        .insert(function.name.span, (symbol, *wait));
                }
            }
            parsed.push(bindings);
            self.native_adapters.insert(id.clone(), provider);
        }
        Ok(parsed)
    }

    fn load_package(
        &mut self,
        path: String,
        kind: Kind,
        parsed: Vec<Parsed>,
        owner: Option<&str>,
    ) -> usize {
        self.stack.push(path.clone());
        if !self.done.contains_key("std/strings")
            && !self.stack.iter().any(|path| path == "std/strings")
            && let Some(span) = parsed
                .iter()
                .find_map(|parsed| string_methods::used(&parsed.file))
        {
            self.load_standard("std/strings", "strings", span);
        }
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
                self.bind_imports(&parsed.file, owner)
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
        if name == "main" {
            self.diagnostics.push(Diagnostic::new(
                Severity::Error,
                "the `main` package cannot be imported",
                span,
            ));
        } else if name != last {
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
    fn bind_imports(&mut self, file: &ast::File, owner: Option<&str>) -> Vec<ImportBinding> {
        let mut bindings = Vec::new();
        for import in &file.imports {
            match self.load_import(&import.path, import.path_span, owner) {
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

    fn load_import(&mut self, path: &str, span: Span, owner: Option<&str>) -> Option<usize> {
        let mut segments = path.split('/');
        let first = segments.next().unwrap_or("");
        let valid = !first.is_empty()
            && first
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && segments.all(|segment| {
                let mut chars = segment.chars();
                chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            });
        if !valid {
            return self.import_error(format!("`{path}` is not a valid import path"), span);
        }
        let (_, rest) = path.split_once('/').unwrap_or((path, ""));
        let target = if first == "std" {
            None
        } else {
            let Some(project) = owner.and_then(|owner| self.projects.get(owner)) else {
                return self.import_error(
                    format!("cannot import `{path}`: no `zore.toml` project file was found"),
                    span,
                );
            };
            if first != project.name && !project.dependencies.contains_key(first) {
                return self.import_error(format!("package `{path}` was not found: import paths start with `{}` or `std`, or a directly declared dependency", project.name), span);
            }
            Some(self.projects.get(first).expect("validated project").clone())
        };
        if let Some(position) = self.stack.iter().position(|visiting| visiting == path) {
            let mut chain = self.stack[position..].to_vec();
            chain.push(path.to_string());
            return self.import_error(format!("import cycle: {}", chain.join(" -> ")), span);
        }
        if let Some(&index) = self.done.get(path) {
            return Some(index);
        }
        let Some(target) = target else {
            return self.load_standard(path, rest, span);
        };
        let dir = self.files.canonical(&target.root.join(rest));
        if !dir.starts_with(&target.root) {
            return self.import_error(
                format!(
                    "package `{path}` escapes project root {}",
                    target.root.display()
                ),
                span,
            );
        }
        if self.entry_dir.as_deref() == Some(self.files.canonical(&dir).as_path()) {
            return self.import_error("the `main` package cannot be imported".into(), span);
        }
        let paths = match self.files.ore_files(&dir) {
            Ok(paths) => paths
                .into_iter()
                .filter(|path| !is_test_file(path))
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        let paths = match paths {
            paths if !paths.is_empty() => paths,
            _ => {
                return self.import_error(
                    format!(
                        "package `{path}` was not found: the folder `{rest}` has no `.ore` files ({})", dir.display()
                    ),
                    span,
                );
            }
        };
        match self.files.manifest(&dir) {
            Ok(Some(manifest)) if self.files.canonical(&manifest.root) == target.root => {}
            Ok(_) => {
                return self.import_error(
                    format!(
                        "package `{path}` crosses a project boundary at {}",
                        dir.display()
                    ),
                    span,
                );
            }
            Err(error) => return self.import_error(error.to_string(), span),
        }
        if let Some(previous) = self.directories.get(&dir)
            && previous != path
        {
            return self.import_error(
                format!(
                    "canonical package folder {} is already imported as `{previous}`, not `{path}`",
                    dir.display()
                ),
                span,
            );
        }
        self.directories.insert(dir.clone(), path.to_string());
        let parsed = match self.parse_folder(&paths, Some(first)) {
            Ok(parsed) => parsed,
            Err(LoadError::Source(error)) => {
                return self.import_error(error.to_string(), span);
            }
            Err(LoadError::Diagnostics(_)) => return None,
        };
        Some(self.load_package(path.to_string(), Kind::Imported, parsed, Some(first)))
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
        Some(self.load_package(path.to_string(), Kind::Imported, parsed, None))
    }
}
