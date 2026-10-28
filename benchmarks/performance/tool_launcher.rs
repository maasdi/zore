use std::path::PathBuf;
use std::process::Command;

fn main() {
    let result = (|| {
        let executable = std::env::current_exe()?;
        let name = executable.file_stem().expect("launcher name");
        let mut script = PathBuf::from(
            std::env::var_os("ZORE_BENCH_SCRIPT_DIRECTORY").expect("script directory"),
        );
        script.push(name);
        script.set_extension("py");
        Command::new(std::env::var_os("ZORE_BENCH_PYTHON").expect("Python executable"))
            .arg(script)
            .args(std::env::args_os().skip(1))
            .status()
    })();
    match result {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("could not launch benchmark tool: {error}");
            std::process::exit(1);
        }
    }
}
