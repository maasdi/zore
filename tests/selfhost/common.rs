//! Shared plumbing for the tests that compare a Zore implementation with its Rust oracle.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use zore::build::{TempDir, build_project};
use zore::driver::project::{Disk, LoadError, load_project};
use zore::source::SourceMap;

pub const DEFAULT_SEED: u64 = 0x5EED_2026_1009;

pub struct Case {
    pub label: String,
    /// One ASCII letter telling the Zore program what to do with the bytes.
    pub mode: u8,
    pub bytes: Vec<u8>,
}

impl Case {
    pub fn text(label: impl Into<String>, mode: u8, text: &str) -> Self {
        Self {
            label: label.into(),
            mode,
            bytes: text.as_bytes().to_vec(),
        }
    }
}

pub fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the compiler crate is inside the repository")
        .to_path_buf()
}

pub struct Program {
    _dir: TempDir,
    pub executable: PathBuf,
}

/// Builds a program in `compiler-zore` with the Rust compiler.
pub fn build_program(entry: &str) -> Program {
    let entry = repository().join(entry);
    let mut sources = SourceMap::new();
    let project = match load_project(&mut sources, &Disk, &entry) {
        Ok(project) => project,
        Err(LoadError::Source(error)) => panic!("cannot load {}: {error}", entry.display()),
        Err(LoadError::Diagnostics(diagnostics)) => panic!(
            "{} does not load:\n{}",
            entry.display(),
            diagnostics
                .iter()
                .map(|d| d.render(&sources).unwrap_or_else(|_| d.message().into()))
                .collect::<String>()
        ),
    };
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("oracle");
    if let Err(error) = build_project(&project, &sources, &executable) {
        panic!("{} does not build: {error:?}", entry.display());
    }
    Program {
        _dir: dir,
        executable,
    }
}

/// Writes every case to `cases.bin`, runs the program once, and splits its output at `done`.
pub fn run_cases(executable: &Path, dir: &Path, cases: &[Case]) -> Vec<Vec<String>> {
    let mut input = Vec::new();
    for case in cases {
        input.extend_from_slice(format!("{} {}\n", case.bytes.len(), case.mode as char).as_bytes());
        input.extend_from_slice(&case.bytes);
    }
    fs::write(dir.join("cases.bin"), input).unwrap();
    let output = Command::new(executable)
        .current_dir(dir)
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .expect("run the Zore program");
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "the Zore program failed ({}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("records are text");
    let mut blocks = vec![Vec::new()];
    for line in stdout.lines() {
        if line == "done" {
            blocks.push(Vec::new());
        } else {
            blocks.last_mut().unwrap().push(line.to_string());
        }
    }
    assert!(
        blocks.pop().is_some_and(|rest| rest.is_empty()),
        "output does not end with `done`"
    );
    assert_eq!(blocks.len(), cases.len(), "one record block per case");
    blocks
}

/// Fails with every case whose records differ, showing the first differing record.
pub fn compare(
    executable: &Path,
    cases: &[Case],
    rust_records: impl Fn(&Path, usize, &Case) -> Vec<String>,
) {
    assert!(!cases.is_empty());
    let dir = TempDir::new().unwrap();
    let actual = run_cases(executable, dir.path(), cases);
    let mut mismatches = Vec::new();
    for (index, (case, zore)) in cases.iter().zip(&actual).enumerate() {
        let rust = rust_records(dir.path(), index, case);
        if &rust == zore {
            continue;
        }
        let at = rust
            .iter()
            .zip(zore)
            .position(|(r, z)| r != z)
            .unwrap_or(rust.len().min(zore.len()));
        mismatches.push(format!(
            "{}\n  input: {:?}\n  record {at}: rust {:?}, zore {:?}\n  rust: {rust:?}\n  zore: {zore:?}",
            case.label,
            String::from_utf8_lossy(&case.bytes),
            rust.get(at),
            zore.get(at),
        ));
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ:\n{}",
        mismatches.len(),
        cases.len(),
        mismatches
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

pub struct Generator(pub u64);

impl Generator {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }
}

pub fn seed(variable: &str) -> u64 {
    match std::env::var(variable) {
        Ok(text) => text
            .parse()
            .unwrap_or_else(|_| panic!("{variable} is a decimal number")),
        Err(_) => DEFAULT_SEED,
    }
}

pub fn ore_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            ore_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "ore") {
            found.push(path);
        }
    }
}

/// The contents of each fenced code block, with the line number of its opening fence.
pub fn code_blocks(text: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut block: Option<(usize, String)> = None;
    for (number, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            match block.take() {
                Some(finished) => blocks.push(finished),
                None => block = Some((number + 1, String::new())),
            }
        } else if let Some((_, body)) = &mut block {
            body.push_str(line);
            body.push('\n');
        }
    }
    blocks
}

pub fn byte_list(text: &str) -> String {
    text.bytes()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(".")
}
