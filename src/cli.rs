//! Argument handling only; no source loading or compiler semantics live here.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(crate) enum Action {
    Check,
    Build,
    Run,
    Format,
    Test,
}

impl Action {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Build => "build",
            Self::Run => "run",
            Self::Format => "fmt",
            Self::Test => "test",
        }
    }
}

pub(crate) enum Command {
    Help(Option<Action>),
    Version,
    Run { action: Action, target: PathBuf },
}

pub(crate) fn parse(args: Vec<OsString>) -> Result<Command, String> {
    let Some(first) = args.first() else {
        return Err("expected a command".into());
    };
    if first == "--help" || first == "-h" {
        return if args.len() == 1 {
            Ok(Command::Help(None))
        } else {
            Err("help does not accept additional arguments".into())
        };
    }
    if first == "--version" || first == "-V" {
        return if args.len() == 1 {
            Ok(Command::Version)
        } else {
            Err("version does not accept additional arguments".into())
        };
    }

    let action = match first.to_str() {
        Some("check") => Action::Check,
        Some("build") => Action::Build,
        Some("run") => Action::Run,
        Some("fmt") => Action::Format,
        Some("test") => Action::Test,
        _ => return Err(format!("unknown command or option {first:?}")),
    };
    let remaining = &args[1..];
    if remaining.len() == 1 && (remaining[0] == "--help" || remaining[0] == "-h") {
        return Ok(Command::Help(Some(action)));
    }

    let (targets, literal) = match remaining.first() {
        Some(arg) if arg == "--" => (&remaining[1..], true),
        _ => (remaining, false),
    };
    if !literal {
        for arg in targets {
            if arg.as_encoded_bytes().starts_with(b"-") {
                return Err(format!("unknown option {arg:?} for {}", action.name()));
            }
        }
    }
    let [target] = targets else {
        return Err(format!("{} requires exactly one target", action.name()));
    };
    if target.is_empty() {
        return Err("target must not be empty".into());
    }
    Ok(Command::Run {
        action,
        target: PathBuf::from(target),
    })
}

pub(crate) fn help(action: Option<Action>) -> String {
    if let Some(action) = action {
        let target = match action {
            Action::Check => "<file.ore>",
            _ => "<target>",
        };
        return format!(
            "Usage: zore {} [--] {target}\n\n\
             This command is not implemented yet. No target is read or modified.\n\
             Use -- before a target whose name starts with '-'.\n",
            action.name()
        );
    }
    "Zore bootstrap compiler\n\n\
     Usage: zore <command> [--] <target>\n\
            zore --help\n\
            zore --version\n\n\
     Commands (not implemented yet):\n\
       check <file.ore>  Check a source file (first compiler target)\n\
       build <target>    Build a program\n\
       run <target>      Run a program\n\
       fmt <target>      Format source\n\
       test <target>     Run Zore tests\n\n\
     Options:\n\
       -h, --help        Show help; also accepted after a command\n\
       -V, --version     Show compiler version\n\n\
     No Zore program can be checked, built, or run yet.\n"
        .into()
}
