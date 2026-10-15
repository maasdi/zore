//! Parses local manifests and validates their dependency graph.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use super::{FileSystem, io_error};
use crate::source::SourceError;

#[derive(Clone)]
pub struct Manifest {
    pub root: PathBuf,
    pub name: String,
    pub dependencies: BTreeMap<String, PathBuf>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "std"
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Manifest {
    pub(super) fn parse(root: PathBuf, text: &str) -> Result<Self, SourceError> {
        let mut name = None;
        let mut version = None;
        let mut dependencies = BTreeMap::new();
        let mut in_dependencies = false;
        for (index, raw) in text.split_inclusive('\n').enumerate() {
            let line = raw
                .strip_suffix('\n')
                .map_or(raw, |line| line.strip_suffix('\r').unwrap_or(line));
            let result = (|| {
                if line
                    .chars()
                    .any(|c| (c < ' ' && c != '\t') || c == '\u{7f}')
                {
                    return Err("control character in manifest".into());
                }
                let mut input = Input(line);
                input.space();
                if input.0.is_empty() || input.0.starts_with('#') {
                    return Ok(());
                }
                if input.0.starts_with('[') {
                    input.take('[')?;
                    if input.key()? != "dependencies" {
                        return Err("unsupported manifest table".into());
                    }
                    input.take(']')?;
                    input.finish()?;
                    if in_dependencies {
                        return Err("duplicate [dependencies] table".into());
                    }
                    in_dependencies = true;
                    return Ok(());
                }
                let key = input.key()?.to_string();
                input.take('=')?;
                if in_dependencies {
                    if !valid_name(&key) {
                        return Err(format!("invalid dependency name `{key}`"));
                    }
                    input.take('{')?;
                    if input.key()? != "path" {
                        return Err("a dependency requires exactly one path field".into());
                    }
                    input.take('=')?;
                    let path = input.string()?;
                    if path.is_empty() {
                        return Err("dependency path must not be empty".into());
                    }
                    if path.contains('\0') {
                        return Err("dependency path must not contain NUL".into());
                    }
                    let path = PathBuf::from(path);
                    #[cfg(windows)]
                    if !path.is_absolute()
                        && (path.has_root()
                            || path
                                .components()
                                .any(|part| matches!(part, std::path::Component::Prefix(_))))
                    {
                        return Err(
                            "Windows dependency paths must be relative or fully absolute".into(),
                        );
                    }
                    input.take('}')?;
                    if dependencies.insert(key.clone(), path).is_some() {
                        return Err(format!("duplicate dependency `{key}`"));
                    }
                } else {
                    let target = match key.as_str() {
                        "name" => &mut name,
                        "version" => &mut version,
                        _ => return Err(format!("unsupported manifest field `{key}`")),
                    };
                    let value = input.string()?;
                    if key == "name" && !valid_name(&value) {
                        return Err("has no valid `name`".into());
                    }
                    if target.replace(value).is_some() {
                        return Err(format!("duplicate manifest field `{key}`"));
                    }
                }
                input.finish()
            })();
            if let Err(message) = result {
                return Err(manifest_error(
                    &root,
                    format!("line {}: {message}", index + 1),
                ));
            }
        }
        let name = name
            .filter(|name| valid_name(name))
            .ok_or_else(|| manifest_error(&root, "line 1: has no valid `name`".into()))?;
        Ok(Self {
            root,
            name,
            dependencies,
        })
    }
}

fn manifest_error(root: &std::path::Path, message: String) -> SourceError {
    io_error(
        &root.join("zore.toml"),
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}

struct Input<'a>(&'a str);

impl<'a> Input<'a> {
    fn space(&mut self) {
        self.0 = self.0.trim_start_matches([' ', '\t']);
    }
    fn take(&mut self, expected: char) -> Result<(), String> {
        self.space();
        self.0 = self
            .0
            .strip_prefix(expected)
            .ok_or_else(|| format!("expected `{expected}`"))?;
        Ok(())
    }
    fn key(&mut self) -> Result<&'a str, String> {
        self.space();
        let length = self
            .0
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
            .count();
        if length == 0 {
            return Err("expected a bare manifest key".into());
        }
        let (key, rest) = self.0.split_at(length);
        self.0 = rest;
        Ok(key)
    }
    fn next(&mut self) -> Result<char, String> {
        let c = self
            .0
            .chars()
            .next()
            .ok_or("unterminated manifest string")?;
        self.0 = &self.0[c.len_utf8()..];
        Ok(c)
    }
    fn string(&mut self) -> Result<String, String> {
        self.space();
        let quote = self.next()?;
        if quote != '"' && quote != '\'' {
            return Err("expected a quoted manifest string".into());
        }
        let mut value = String::new();
        loop {
            let c = self.next()?;
            if c == quote {
                return Ok(value);
            }
            if (c < ' ' && c != '\t') || c == '\u{7f}' {
                return Err("control character in manifest string".into());
            }
            if quote == '"' && c == '\\' {
                let escaped = match self.next()? {
                    'b' => '\u{8}',
                    't' => '\t',
                    'n' => '\n',
                    'f' => '\u{c}',
                    'r' => '\r',
                    '"' => '"',
                    '\\' => '\\',
                    kind @ ('u' | 'U') => {
                        let mut code = 0;
                        for _ in 0..if kind == 'u' { 4 } else { 8 } {
                            code = (code << 4)
                                | self.next()?.to_digit(16).ok_or("invalid Unicode escape")?;
                        }
                        char::from_u32(code).ok_or("invalid Unicode scalar in manifest string")?
                    }
                    _ => return Err("unsupported manifest string escape".into()),
                };
                value.push(escaped);
            } else {
                value.push(c);
            }
        }
    }
    fn finish(&mut self) -> Result<(), String> {
        self.space();
        if self.0.is_empty() || self.0.starts_with('#') {
            Ok(())
        } else {
            Err("unexpected trailing manifest input".into())
        }
    }
}

pub(super) fn resolve(
    files: &dyn FileSystem,
    root: Manifest,
) -> Result<HashMap<String, Manifest>, SourceError> {
    let mut projects = HashMap::new();
    visit(files, root, &mut projects, &mut Vec::new())?;
    Ok(projects)
}

fn visit(
    files: &dyn FileSystem,
    mut manifest: Manifest,
    projects: &mut HashMap<String, Manifest>,
    visiting: &mut Vec<String>,
) -> Result<(), SourceError> {
    manifest.root = files.canonical(&manifest.root);
    if let Some(previous) = projects.get(&manifest.name) {
        if previous.root != manifest.root {
            return Err(manifest_error(
                &manifest.root,
                format!(
                    "conflicting project `{}`: {} and {}",
                    manifest.name,
                    previous.root.display(),
                    manifest.root.display()
                ),
            ));
        }
        if let Some(index) = visiting.iter().position(|name| name == &manifest.name) {
            let mut chain = visiting[index..].to_vec();
            chain.push(manifest.name.clone());
            return Err(manifest_error(
                &manifest.root,
                format!("dependency cycle: {}", chain.join(" -> ")),
            ));
        }
        return Ok(());
    }
    projects.insert(manifest.name.clone(), manifest.clone());
    visiting.push(manifest.name.clone());
    for (name, path) in &manifest.dependencies {
        let target = files.canonical(&manifest.root.join(path));
        let dependency = files
            .manifest(&target)
            .map_err(|error| {
                manifest_error(&manifest.root, format!("dependency `{name}`: {error}"))
            })?
            .filter(|dependency| files.canonical(&dependency.root) == target)
            .ok_or_else(|| {
                manifest_error(
                    &manifest.root,
                    format!(
                        "dependency `{name}` path {} must name a project root containing zore.toml",
                        target.display()
                    ),
                )
            })?;
        if dependency.name != *name {
            return Err(manifest_error(
                &manifest.root,
                format!(
                    "dependency key `{name}` does not match project name `{}` in {}",
                    dependency.name,
                    target.display()
                ),
            ));
        }
        visit(files, dependency, projects, visiting)?;
    }
    visiting.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_keep_literal_windows_paths_and_decode_basic_escapes() {
        let manifest = Manifest::parse(
            PathBuf::from("project"),
            "name = 'app'\n[dependencies]\nauth = { path = 'C:\\work\\auth' }\n",
        )
        .unwrap();
        assert_eq!(
            manifest.dependencies["auth"],
            PathBuf::from(r"C:\work\auth")
        );
        for (encoded, decoded) in [
            (r#""a\\b\"c\t\n\r\b\f""#, "a\\b\"c\t\n\r\u{8}\u{c}"),
            (r#""\u0041\U0001F600""#, "A😀"),
            ("'a#b=c'", "a#b=c"),
        ] {
            assert_eq!(Input(encoded).string().unwrap(), decoded);
        }
        for encoded in [
            r#""\uDFFF""#,
            r#""\U00110000""#,
            r#""\uXX00""#,
            "\"raw\rcontrol\"",
            "\"raw\u{7f}control\"",
            "\"unterminated",
            "'''multiline'''",
        ] {
            let result = (|| {
                let mut input = Input(encoded);
                input.string()?;
                input.finish()
            })();
            assert!(result.is_err(), "{encoded:?}");
        }
    }
}
