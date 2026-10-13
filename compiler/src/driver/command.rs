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
    Run {
        action: Action,
        target: PathBuf,
        format_check: bool,
    },
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

    let mut targets = Vec::new();
    let mut literal = false;
    let mut format_check = false;
    for arg in remaining {
        if !literal && arg == "--" {
            if !targets.is_empty() {
                return Err(format!("unknown option {arg:?} for {}", action.name()));
            }
            literal = true;
            continue;
        }
        if !literal && action == Action::Format && arg == "--check" && !format_check {
            format_check = true;
            continue;
        }
        if !literal && arg.as_encoded_bytes().starts_with(b"-") {
            return Err(format!("unknown option {arg:?} for {}", action.name()));
        }
        targets.push(arg);
    }
    let [target] = targets.as_slice() else {
        return Err(format!("{} requires exactly one target", action.name()));
    };
    if target.is_empty() {
        return Err("target must not be empty".into());
    }
    Ok(Command::Run {
        action,
        target: PathBuf::from(target),
        format_check,
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
            Action::Format => {
                "Usage: zore fmt [--check] [--] <file.ore|directory>\n\n\
                Format a source file or recursively format .ore files in a directory.\n\
                Validates syntax before writing; requires no imports or native toolchain.\n\
                --check lists files that need formatting and exits 1 without writing.\n\
                Skips symbolic links, hidden directories, target, and node_modules.\n"
            }
            Action::Test => {
                "Usage: zore test [--] <file.ore|directory>\n\n\
                Run TestName functions in _test.ore files, one process per test.\n\
                Tests take no parameters and return nothing or error.\n\
                A directory selects packages recursively; a test file selects its tests.\n\
                Requires clang and rustc, as for build. Exits 1 on any failure.\n"
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
       fmt <target>      Format a source file or directory (--check for CI)\n\
       test <target>     Run application tests (requires clang and rustc)\n\n\
     Options:\n\
       -h, --help        Show help; also accepted after a command\n\
       -V, --version     Show compiler version\n\n\
     A program is the folder of the file you name, with the packages it imports.\n"
        .into()
}
