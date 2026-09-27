//! Zore driver: `check`, `build`, and `run` invoke the compiler; `fmt` and
//! `test` are not implemented yet.

mod cli;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use zore::build::{BuildError, TempDir, build};
use zore::check::check_file;
use zore::diagnostic::Diagnostic;
use zore::source::{FileId, SourceMap};

fn main() -> ExitCode {
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

fn load(target: &Path) -> Result<(SourceMap, FileId), ExitCode> {
    let mut sources = SourceMap::new();
    match sources.load(target) {
        Ok(id) => Ok((sources, id)),
        Err(error) => {
            eprintln!("zore: {error}");
            Err(ExitCode::FAILURE)
        }
    }
}

fn report(sources: &SourceMap, diagnostics: &[Diagnostic], verb: &str) -> ExitCode {
    for diagnostic in diagnostics {
        let rendered = diagnostic
            .render(sources)
            .expect("diagnostics refer to the loaded file");
        eprintln!("{rendered}");
    }
    let count = diagnostics.len();
    eprintln!(
        "zore: {verb} failed with {count} error{}",
        if count == 1 { "" } else { "s" }
    );
    ExitCode::FAILURE
}

/// Check one file as a complete package. Success is silent; diagnostics go
/// to stderr and make the exit status 1.
fn check(target: &Path) -> ExitCode {
    let (sources, id) = match load(target) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let checked = check_file(sources.file(id).expect("just loaded"));
    if checked.diagnostics.is_empty() {
        return ExitCode::SUCCESS;
    }
    report(&sources, &checked.diagnostics, "check")
}

fn build_to(target: &Path, output: &Path, verb: &str) -> Result<(), ExitCode> {
    let (sources, id) = load(target)?;
    match build(sources.file(id).expect("just loaded"), output) {
        Ok(()) => Ok(()),
        Err(BuildError::Diagnostics(diagnostics)) => Err(report(&sources, &diagnostics, verb)),
        Err(error) => {
            eprintln!("zore: {error}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// Build `dir/name.ore` into `./name` in the current directory.
fn build_command(target: &Path) -> ExitCode {
    let Some(stem) = target.file_stem() else {
        eprintln!("zore: {target:?} has no file name");
        return ExitCode::FAILURE;
    };
    let output = PathBuf::from(stem);
    match build_to(target, &output, "build") {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

/// Build into a temporary directory and run the program, forwarding its
/// exit status.
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
