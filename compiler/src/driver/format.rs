use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::source::SourceMap;

static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);

pub(super) fn run(target: &Path, check: bool) -> ExitCode {
    match format(target, check) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("zore: {message}");
            ExitCode::FAILURE
        }
    }
}

fn format(target: &Path, check: bool) -> Result<bool, String> {
    let mut paths = Vec::new();
    collect(target, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        return Err(format!("{}: no .ore files found", target.display()));
    }
    let mut sources = SourceMap::new();
    let mut changes = Vec::new();
    let mut invalid = false;
    for path in paths {
        let id = sources.load(&path).map_err(|e| e.to_string())?;
        let file = sources.file(id).expect("loaded source");
        match crate::format::format_file(file) {
            Ok(formatted) if formatted != file.text() => {
                changes.push((path, file.text().to_owned(), formatted))
            }
            Ok(_) => {}
            Err(diagnostics) => {
                invalid = true;
                for diagnostic in diagnostics {
                    eprintln!(
                        "{}",
                        diagnostic.render(&sources).map_err(|e| e.to_string())?
                    );
                }
            }
        }
    }
    if invalid {
        return Err("format failed; no files were modified".into());
    }
    if check {
        for (path, _, _) in &changes {
            println!("{}", path.display());
        }
        return Ok(changes.is_empty());
    }
    for (path, original, _) in &changes {
        if fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))? != *original {
            return Err(format!(
                "{} changed while formatting; no files were modified",
                path.display()
            ));
        }
    }
    for (path, _, formatted) in changes {
        replace(&path, &formatted)?;
    }
    Ok(true)
}

fn collect(path: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "{}: symbolic links are not formatted",
            path.display()
        ));
    }
    if metadata.is_file() {
        if path.extension().is_some_and(|ext| ext == "ore") {
            paths.push(path.to_owned());
            return Ok(());
        }
        return Err(format!(
            "{}: expected a .ore file or directory",
            path.display()
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "{}: expected a .ore file or directory",
            path.display()
        ));
    }
    let entries = fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", path.display()))?;
        let kind = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        if kind.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        if kind.is_dir() {
            if name.as_encoded_bytes().starts_with(b".")
                || name == "target"
                || name == "node_modules"
            {
                continue;
            }
            collect(&entry.path(), paths)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "ore") {
            paths.push(entry.path());
        }
    }
    Ok(())
}

fn replace(path: &Path, text: &str) -> Result<(), String> {
    let serial = NEXT_WRITE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_file_name(format!(".zore-fmt-{}-{serial}", std::process::id()));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let result = (|| -> std::io::Result<()> {
        output.write_all(text.as_bytes())?;
        output.set_permissions(fs::metadata(path)?.permissions())?;
        output.sync_all()?;
        drop(output);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("{}: {e}", path.display()))
}
