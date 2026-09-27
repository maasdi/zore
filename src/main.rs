//! Zore driver. Semantic checking and compilation are not implemented yet.

mod cli;

use std::process::ExitCode;

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
