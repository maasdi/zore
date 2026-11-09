use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use super::build::{BuildError, TempDir, build_test_project};
use super::check::check_project_with_entry;
use super::command::TestOptions;
use super::dependencies::{DependencyDisk, Options};
use super::project::{Disk, FileSystem, LoadError, Project, is_test_file, load_test_project};
use super::session::report;
use crate::ast::{File, Item, Param, ParamMode, Type};
use crate::diagnostic::{Diagnostic, Severity};
use crate::source::SourceMap;

const ENTRY_NAME: &str = "$test";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Plain,
    Context,
    Benchmark,
}

struct Test {
    name: String,
    kind: Kind,
    returns_error: bool,
}

struct Suite {
    directory: PathBuf,
    executable: PathBuf,
    tests: Vec<Test>,
}

pub(super) fn run(target: &Path, dependencies: Options, options: &TestOptions) -> ExitCode {
    match test(target, dependencies, options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("zore: {message}");
            }
            ExitCode::FAILURE
        }
    }
}

fn test(target: &Path, dependencies: Options, options: &TestOptions) -> Result<bool, String> {
    let bench = options.bench;
    let metadata = fs::symlink_metadata(target).map_err(|error| path_error(target, error))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "{}: symbolic links are not test targets",
            target.display()
        ));
    }
    let mut directories = Vec::new();
    let selected_file = if metadata.is_file() && target.extension().is_some_and(|ext| ext == "ore")
    {
        directories.push(
            target
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
        );
        is_test_file(target).then(|| target.file_name().expect("a source file").to_owned())
    } else if metadata.is_dir() {
        collect(target, &mut directories)?;
        None
    } else {
        return Err(format!(
            "{}: expected a .ore file or directory",
            target.display()
        ));
    };
    directories.sort();
    let scratch = TempDir::new().map_err(|error| error.to_string())?;
    let mut suites = Vec::new();
    for (index, directory) in directories.into_iter().enumerate() {
        let paths = Disk
            .ore_files(&directory)
            .map_err(|error| error.to_string())?;
        let Some(entry) = paths.first() else { continue };
        let mut sources = SourceMap::new();
        let mut project = load_test_project(&mut sources, &DependencyDisk(dependencies), entry)
            .map_err(|error| load_error(&sources, error))?;
        let tests = discover(&project, &sources, selected_file.as_deref(), options)?;
        if tests.is_empty() {
            let checked = check_project_with_entry(&project, &sources, Some(ENTRY_NAME));
            if !checked.diagnostics.is_empty() {
                report(&sources, &checked.diagnostics, "test");
                return Err(String::new());
            }
            continue;
        }
        let name = &project.packages.last().expect("entry package").name;
        let runner = dispatcher(name, &tests, options.goal_nanoseconds);
        project
            .add_test_runner(&mut sources, runner, ENTRY_NAME)
            .map_err(|error| load_error(&sources, error))?;
        let executable = scratch.executable(&format!("tests-{index}"));
        build_test_project(&project, &sources, &executable, ENTRY_NAME).map_err(
            |error| match error {
                BuildError::Diagnostics(diagnostics) => {
                    report(&sources, &diagnostics, "test");
                    String::new()
                }
                error => error.to_string(),
            },
        )?;
        suites.push(Suite {
            directory,
            executable: fs::canonicalize(&executable)
                .map_err(|error| path_error(&executable, error))?,
            tests,
        });
    }
    if bench {
        let runs = if options.count == 1 {
            String::new()
        } else {
            format!(", {} runs each", options.count)
        };
        println!(
            "Benchmarks: zore {}, {} {}, clang -O2, at least {} per benchmark in its own process{runs}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            goal_text(options.goal_nanoseconds),
        );
    }
    let mut passed = 0;
    let mut failed = 0;
    let repeats = if bench { options.count } else { 1 };
    for suite in suites {
        for test in suite
            .tests
            .iter()
            .flat_map(|test| std::iter::repeat_n(test, repeats as usize))
        {
            let label = format!("{}::{}", suite.directory.display(), test.name);
            println!("RUN {label}");
            io::stdout().flush().map_err(|error| error.to_string())?;
            match Command::new(&suite.executable)
                .arg(&test.name)
                .current_dir(&suite.directory)
                .status()
            {
                Ok(status) if status.success() => {
                    passed += 1;
                    println!("PASS {label}");
                }
                Ok(status) => {
                    failed += 1;
                    match status.code() {
                        Some(code) => println!("FAIL {label} (exit status: {code})"),
                        None => println!("FAIL {label} ({status})"),
                    }
                }
                Err(error) => {
                    failed += 1;
                    println!("FAIL {label} (could not run: {error})");
                }
            }
        }
    }
    let noun = if bench { "Benchmarks" } else { "Tests" };
    println!("{noun}: {passed} passed, {failed} failed");
    Ok(failed == 0)
}

fn discover(
    project: &Project,
    sources: &SourceMap,
    selected: Option<&std::ffi::OsStr>,
    options: &TestOptions,
) -> Result<Vec<Test>, String> {
    let bench = options.bench;
    let mut tests = Vec::new();
    let mut diagnostics = Vec::new();
    for loaded in &project.packages.last().expect("entry package").files {
        for item in &loaded.file.items {
            let Item::Func(function) = item else { continue };
            let path = sources
                .file(function.span.file())
                .expect("loaded declaration")
                .path();
            if !is_test_file(path) || function.receiver.is_some() {
                continue;
            }
            let name = &function.name.text;
            let benchmark = if prefixed(name, "Test") {
                false
            } else if prefixed(name, "Benchmark") {
                true
            } else {
                continue;
            };
            let returns_error =
                matches!(function.results.as_slice(), [Type::Named(name)] if name.text == "error");
            let context = if benchmark { "B" } else { "T" };
            let kind = match function.params.as_slice() {
                [] if !benchmark => Some(Kind::Plain),
                [param] if is_context(param, &loaded.file, context) => Some(if benchmark {
                    Kind::Benchmark
                } else {
                    Kind::Context
                }),
                _ => None,
            };
            let Some(kind) = kind.filter(|_| {
                !function.is_async
                    && function.type_params.is_empty()
                    && (function.results.is_empty() || returns_error)
            }) else {
                let message = if benchmark {
                    "a benchmark must be synchronous, non-generic, take one `b mut testing.B`, and return nothing or error"
                } else {
                    "a test must be synchronous, non-generic, take no parameters or one `t mut testing.T`, and return nothing or error"
                };
                diagnostics.push(Diagnostic::new(
                    Severity::Error,
                    message,
                    function.name.span,
                ));
                continue;
            };
            if benchmark == bench
                && selected.is_none_or(|selected| path.file_name() == Some(selected))
                && options
                    .filter
                    .as_deref()
                    .is_none_or(|filter| name.contains(filter))
            {
                tests.push(Test {
                    name: name.clone(),
                    kind,
                    returns_error,
                });
            }
        }
    }
    if !diagnostics.is_empty() {
        report(sources, &diagnostics, "test");
        return Err(String::new());
    }
    tests.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(tests)
}

fn prefixed(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|suffix| suffix.starts_with(|c: char| c.is_ascii_uppercase()))
}

/// `t mut testing.T` or `b mut testing.B`, where `testing` is the file's name for `std/testing`.
fn is_context(param: &Param, file: &File, name: &str) -> bool {
    let Some(import) = file
        .imports
        .iter()
        .find(|import| import.path == "std/testing")
    else {
        return false;
    };
    let bound = import
        .alias
        .as_ref()
        .map_or("testing", |alias| alias.text.as_str());
    param.mode == ParamMode::Mut
        && matches!(&param.ty, Type::Qualified { package, name: ty, .. }
            if package.text == bound && ty.text == name)
}

fn goal_text(nanoseconds: u64) -> String {
    if nanoseconds.is_multiple_of(1_000_000_000) {
        format!("{}s", nanoseconds / 1_000_000_000)
    } else {
        format!("{}ms", nanoseconds / 1_000_000)
    }
}

fn dispatcher(package: &str, tests: &[Test], goal_nanoseconds: u64) -> String {
    let mut text = format!("package {package}\nimport \"std/os\"\n");
    if tests
        .iter()
        .any(|test| test.returns_error && test.kind == Kind::Plain)
    {
        text.push_str("import \"std/errors\"\n");
    }
    if tests.iter().any(|test| test.kind != Kind::Plain) {
        text.push_str("import \"std/testing\"\n");
    }
    text.push_str("func runner() int {\nlet args = os.Args()\n");
    for test in tests {
        let name = &test.name;
        text.push_str(&format!("if args[1] == \"{name}\" {{\n"));
        match test.kind {
            Kind::Plain if test.returns_error => text.push_str(&format!(
                "let err = {name}()\nif err != nil {{\nprintln(errors.Message(err))\nreturn 1\n}}\nreturn 0\n"
            )),
            Kind::Plain => text.push_str(&format!("{name}()\nreturn 0\n")),
            Kind::Context | Kind::Benchmark => {
                let (binding, value) = if test.kind == Kind::Context {
                    ("t", format!("testing.NewT(\"{name}\")"))
                } else {
                    (
                        "b",
                        format!("testing.NewB(\"{name}\", {goal_nanoseconds})"),
                    )
                };
                text.push_str(&format!("var {binding} = {value}\n"));
                if test.returns_error {
                    text.push_str(&format!(
                        "let err = {name}({binding})\nreturn {binding}.Finish(err)\n"
                    ));
                } else {
                    text.push_str(&format!("{name}({binding})\nreturn {binding}.Finish(nil)\n"));
                }
            }
        }
        text.push_str("}\n");
    }
    text.push_str("return 1\n}\n");
    text
}

fn collect(directory: &Path, directories: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| path_error(directory, error))?;
    let mut has_sources = false;
    for entry in entries {
        let entry = entry.map_err(|error| path_error(directory, error))?;
        let kind = entry
            .file_type()
            .map_err(|error| path_error(&entry.path(), error))?;
        let name = entry.file_name();
        if kind.is_dir()
            && !name.as_encoded_bytes().starts_with(b".")
            && name != "target"
            && name != "node_modules"
        {
            collect(&entry.path(), directories)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "ore") {
            has_sources = true;
        }
    }
    if has_sources {
        directories.push(directory.to_path_buf());
    }
    Ok(())
}

fn load_error(sources: &SourceMap, error: LoadError) -> String {
    match error {
        LoadError::Source(error) => error.to_string(),
        LoadError::Diagnostics(diagnostics) => {
            report(sources, &diagnostics, "test");
            String::new()
        }
    }
}

fn path_error(path: &Path, error: io::Error) -> String {
    format!("{}: {error}", path.display())
}
