use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use super::build::{BuildError, TempDir, build_test_project};
use super::check::check_project_with_entry;
use super::project::{Disk, FileSystem, LoadError, Project, is_test_file, load_test_project};
use super::session::report;
use crate::ast::{Item, Type};
use crate::diagnostic::{Diagnostic, Severity};
use crate::source::SourceMap;

const ENTRY_NAME: &str = "$test";

struct Test {
    name: String,
    returns_error: bool,
}

struct Suite {
    directory: PathBuf,
    executable: PathBuf,
    tests: Vec<Test>,
}

pub(super) fn run(target: &Path) -> ExitCode {
    match test(target) {
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

fn test(target: &Path) -> Result<bool, String> {
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
        let mut project = load_test_project(&mut sources, &Disk, entry)
            .map_err(|error| load_error(&sources, error))?;
        let tests = discover(&project, &sources, selected_file.as_deref())?;
        if tests.is_empty() {
            let checked = check_project_with_entry(&project, &sources, Some(ENTRY_NAME));
            if !checked.diagnostics.is_empty() {
                report(&sources, &checked.diagnostics, "test");
                return Err(String::new());
            }
            continue;
        }
        let name = &project.packages.last().expect("entry package").name;
        let runner = dispatcher(name, &tests);
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
    let mut passed = 0;
    let mut failed = 0;
    for suite in suites {
        for test in suite.tests {
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
    println!("Tests: {passed} passed, {failed} failed");
    Ok(failed == 0)
}

fn discover(
    project: &Project,
    sources: &SourceMap,
    selected: Option<&std::ffi::OsStr>,
) -> Result<Vec<Test>, String> {
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
            if !name
                .strip_prefix("Test")
                .is_some_and(|suffix| suffix.starts_with(|c: char| c.is_ascii_uppercase()))
            {
                continue;
            }
            let returns_error =
                matches!(function.results.as_slice(), [Type::Named(name)] if name.text == "error");
            if function.is_async
                || !function.type_params.is_empty()
                || !function.params.is_empty()
                || (!function.results.is_empty() && !returns_error)
            {
                diagnostics.push(Diagnostic::new(Severity::Error,
                    "a test must be synchronous, non-generic, take no parameters, and return nothing or error",
                    function.name.span));
            } else if selected.is_none_or(|selected| path.file_name() == Some(selected)) {
                tests.push(Test {
                    name: name.clone(),
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

fn dispatcher(package: &str, tests: &[Test]) -> String {
    let mut text = format!("package {package}\nimport \"std/os\"\n");
    if tests.iter().any(|test| test.returns_error) {
        text.push_str("import \"std/errors\"\n");
    }
    text.push_str("func runner() int {\nlet args = os.Args()\n");
    for test in tests {
        text.push_str(&format!("if args[1] == \"{}\" {{\n", test.name));
        if test.returns_error {
            text.push_str(&format!(
                "let err = {}()\nif err != nil {{\nprintln(errors.Message(err))\nreturn 1\n}}\n",
                test.name
            ));
        } else {
            text.push_str(&format!("{}()\n", test.name));
        }
        text.push_str("return 0\n}\n");
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
