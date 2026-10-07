use super::command as cli;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::diagnostic::Diagnostic;
use crate::driver::build::{BuildError, TempDir, build_project};
use crate::driver::check::check_project;
use crate::driver::project::{Disk, LoadError, Project, load_project};
use crate::source::SourceMap;

pub fn run() -> ExitCode {
    match cli::parse(std::env::args_os().skip(1).collect()) {
        Ok(cli::Command::Help(action)) => {
            print!("{}", cli::help(action));
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Version) => {
            println!("zore {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Run { action, target }) => match action {
            cli::Action::Check => check(&target),
            cli::Action::Build => build_command(&target),
            cli::Action::Run => run_command(&target),
            cli::Action::Format | cli::Action::Test => {
                eprintln!(
                    "zore: {} is not implemented yet; {:?} was not processed",
                    action.name(),
                    target
                );
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            eprintln!("zore: {message}\nTry 'zore --help' for usage.");
            ExitCode::from(2)
        }
    }
}

fn load(target: &Path, verb: &str) -> Result<(SourceMap, Project), ExitCode> {
    let mut sources = SourceMap::new();
    match load_project(&mut sources, &Disk, target) {
        Ok(project) => Ok((sources, project)),
        Err(LoadError::Source(error)) => {
            eprintln!("zore: {error}");
            Err(ExitCode::FAILURE)
        }
        Err(LoadError::Diagnostics(diagnostics)) => Err(report(&sources, &diagnostics, verb)),
    }
}

fn report(sources: &SourceMap, diagnostics: &[Diagnostic], verb: &str) -> ExitCode {
    for diagnostic in diagnostics {
        match diagnostic.render(sources) {
            Ok(rendered) => eprintln!("{rendered}"),
            Err(_) => eprintln!("error: {}", diagnostic.message()),
        }
    }
    let count = diagnostics.len();
    eprintln!(
        "zore: {verb} failed with {count} error{}",
        if count == 1 { "" } else { "s" }
    );
    ExitCode::FAILURE
}

fn check(target: &Path) -> ExitCode {
    let (sources, project) = match load(target, "check") {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let checked = check_project(&project, &sources);
    if checked.diagnostics.is_empty() {
        return ExitCode::SUCCESS;
    }
    report(&sources, &checked.diagnostics, "check")
}

fn build_to(target: &Path, output: &Path, verb: &str) -> Result<(), ExitCode> {
    let (sources, project) = load(target, verb)?;
    match build_project(&project, &sources, output) {
        Ok(()) => Ok(()),
        Err(BuildError::Diagnostics(diagnostics)) => Err(report(&sources, &diagnostics, verb)),
        Err(error) => {
            eprintln!("zore: {error}");
            Err(ExitCode::FAILURE)
        }
    }
}

fn build_command(target: &Path) -> ExitCode {
    let Some(stem) = target.file_stem() else {
        eprintln!("zore: {target:?} has no file name");
        return ExitCode::FAILURE;
    };
    let executable_in_current_dir = PathBuf::from(stem);
    match build_to(target, &executable_in_current_dir, "build") {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

fn run_command(target: &Path) -> ExitCode {
    let dir = match TempDir::new() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("zore: {error}");
            return ExitCode::FAILURE;
        }
    };
    let executable = dir.path().join("program");
    if let Err(code) = build_to(target, &executable, "build") {
        return code;
    }
    match std::process::Command::new(&executable).status() {
        Ok(status) => match status.code() {
            Some(code) => ExitCode::from(code.clamp(0, 255) as u8),
            None => {
                eprintln!("zore: the program was terminated by a signal");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("zore: could not run the built program: {error}");
            ExitCode::FAILURE
        }
    }
}
