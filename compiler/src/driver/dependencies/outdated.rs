use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use super::lock::{Binding, Bindings};
use super::{Mode, Options, cache, error, lock, root_manifest, versioned};
use crate::driver::release::Version;
use crate::source::SourceError;

pub(crate) fn run(target: &Path, offline: bool) -> ExitCode {
    match report(target, offline) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("zore: {error}");
            ExitCode::FAILURE
        }
    }
}

fn report(target: &Path, offline: bool) -> Result<String, SourceError> {
    let root = root_manifest(target)?;
    let path = root.root.join("zore.lock");
    let text = fs::read_to_string(&path).map_err(|_| {
        error(
            &path,
            "outdated requires a matching zore.lock; run `zore install` first",
        )
    })?;
    versioned::resolve(
        root,
        Options {
            mode: Mode::Build,
            locked: true,
            offline: true,
        },
        Some(&text),
    )?;
    let selected = lock::selected(&text)?;
    if selected.is_empty() {
        return Ok("No tagged dependencies are selected.\n".into());
    }
    let known = if offline {
        let mut known = lock::read(Some(&text))?;
        for (key, binding) in versioned::cached_bindings()? {
            if known.get(&key).is_some_and(|locked| locked != &binding) {
                return Err(error(
                    &path,
                    "cached release binding conflicts with zore.lock",
                ));
            }
            known.insert(key, binding);
        }
        known
    } else {
        BTreeMap::new()
    };
    let mut report = if offline {
        "Stale and incomplete cached releases; no remote lookup.\n".to_string()
    } else {
        "Advertised tags only; source metadata is unverified until explicit lock.\n".to_string()
    };
    report.push_str("PROJECT\tLOCKED\tNEWEST COMPATIBLE\tNEWER INCOMPATIBLE LINES\n");
    for binding in selected {
        let mut versions = if offline {
            cached_versions(&binding, &known)
        } else {
            cache::advertised(&binding.key.url, &binding.key.name)?
        };
        versions.sort();
        let current = &binding.key.version;
        let compatible = versions
            .iter()
            .filter(|version| version.compatible_with(current))
            .max();
        let mut incompatible: Vec<Version> = Vec::new();
        for version in &versions {
            if version <= current || version.compatible_with(current) {
                continue;
            }
            if let Some(latest) = incompatible.last_mut()
                && latest.compatible_with(version)
            {
                *latest = version.clone();
            } else {
                incompatible.push(version.clone());
            }
        }
        let incompatible = incompatible
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        report.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            binding.key.name,
            current,
            compatible.map_or_else(|| "-".into(), ToString::to_string),
            if incompatible.is_empty() {
                "-"
            } else {
                &incompatible
            },
        ));
    }
    Ok(report)
}

fn cached_versions(selected: &Binding, bindings: &Bindings) -> Vec<Version> {
    bindings
        .keys()
        .filter(|key| {
            key.name == selected.key.name
                && key.url == selected.key.url
                && key.subdir == selected.key.subdir
        })
        .map(|key| key.version.clone())
        .collect()
}
