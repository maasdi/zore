//! Argument handling only; no source loading or compiler semantics live here.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Copy, Eq, PartialEq)]
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
        let text = match action {
            Action::Check => {
                "Usage: zore check [--] <file.ore>\n\n\
                Check one source file as a complete package. Only part of the\n\
                language is supported so far; unsupported features are reported\n\
                as errors. Prints nothing and exits 0 when the file is valid.\n"
            }
            Action::Build => {
                "Usage: zore build [--] <file.ore>\n\n\
                Check one source file and compile it to a native executable named\n\
                after the file (main.ore builds ./main) in the current directory.\n\
                Requires clang with LLVM 15 or newer; set ZORE_CC to choose it.\n"
            }
            Action::Run => {
                "Usage: zore run [--] <file.ore>\n\n\
                Build one source file into a temporary directory and run it. The\n\
                program's exit status is returned. Requires clang, as for build.\n"
            }
            _ => {
                return format!(
                    "Usage: zore {} [--] <target>\n\n\
                     This command is not implemented yet. No target is read or modified.\n\
                     Use -- before a target whose name starts with '-'.\n",
                    action.name()
                );
            }
        };
        return format!("{text}Use -- before a target whose name starts with '-'.\n");
    }
    "Zore bootstrap compiler\n\n\
     Usage: zore <command> [--] <target>\n\
            zore --help\n\
            zore --version\n\n\
     Commands (a subset of the language is supported):\n\
       check <file.ore>  Check a source file\n\
       build <file.ore>  Build a native executable (requires clang)\n\
       run <file.ore>    Build and run a program (requires clang)\n\n\
     Not implemented yet:\n\
       fmt <target>      Format source\n\
       test <target>     Run Zore tests\n\n\
     Options:\n\
       -h, --help        Show help; also accepted after a command\n\
       -V, --version     Show compiler version\n\n\
     Programs are compiled one file at a time.\n"
        .into()
}
