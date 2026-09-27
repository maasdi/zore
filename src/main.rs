//! Zore driver. Only `check` runs compiler stages; other commands are not
//! implemented yet.

mod cli;

use std::path::Path;
use std::process::ExitCode;

use zore::check::check_file;
use zore::source::SourceMap;

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
        Ok(cli::Command::Run {
            action: cli::Action::Check,
            target,
        }) => check(&target),
        Ok(cli::Command::Run { action, target }) => {
            eprintln!(
                "zore: {} is not implemented yet; {:?} was not processed",
                action.name(),
                target
            );
            ExitCode::FAILURE
        }
        Err(message) => {
            eprintln!("zore: {message}\nTry 'zore --help' for usage.");
            ExitCode::from(2)
        }
    }
}

/// Check one file as a complete package. Success is silent; diagnostics go
/// to stderr and make the exit status 1.
fn check(target: &Path) -> ExitCode {
    let mut sources = SourceMap::new();
    let id = match sources.load(target) {
        Ok(id) => id,
        Err(error) => {
            eprintln!("zore: {error}");
            return ExitCode::FAILURE;
        }
    };
    let checked = check_file(sources.file(id).expect("just loaded"));
    if checked.diagnostics.is_empty() {
        return ExitCode::SUCCESS;
    }
    for diagnostic in &checked.diagnostics {
        let rendered = diagnostic
            .render(&sources)
            .expect("diagnostics refer to the checked file");
        eprintln!("{rendered}");
    }
    let count = checked.diagnostics.len();
    eprintln!(
        "zore: check failed with {count} error{}",
        if count == 1 { "" } else { "s" }
    );
    ExitCode::FAILURE
}
