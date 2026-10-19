use std::collections::BTreeMap;
use std::path::Path;

use super::{error, quote};
use crate::driver::project::manifest::{Input, portable_path, valid_name, validate_url};
use crate::driver::release::Version;
use crate::source::SourceError;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct ReleaseKey {
    pub name: String,
    pub url: String,
    pub subdir: String,
    pub version: Version,
}

impl ReleaseKey {
    pub fn tag(&self) -> String {
        format!("{}/v{}", self.name, self.version)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Binding {
    pub key: ReleaseKey,
    pub revision: String,
    pub tree: String,
    pub manifest: String,
    pub native: Option<(String, String)>,
}

pub(super) type Bindings = BTreeMap<ReleaseKey, Binding>;

fn invalid(message: &str) -> SourceError {
    error(Path::new("zore.lock"), message)
}

fn fields(text: &str) -> Result<BTreeMap<String, String>, SourceError> {
    let parse = || -> Result<BTreeMap<String, String>, String> {
        let mut input = Input(text);
        input.take('{')?;
        let mut fields = BTreeMap::new();
        loop {
            let key = input.key()?.to_string();
            input.take('=')?;
            let value = input.string()?;
            if fields.insert(key, value).is_some() {
                return Err("duplicate lock field".into());
            }
            input.space();
            if input.0.starts_with('}') {
                break;
            }
            input.take(',')?;
        }
        input.take('}')?;
        input.finish()?;
        Ok(fields)
    };
    parse().map_err(|_| invalid("malformed version-2 lock record"))
}

fn take(fields: &mut BTreeMap<String, String>, name: &str) -> Result<String, SourceError> {
    fields
        .remove(name)
        .ok_or_else(|| invalid("missing version-2 lock field"))
}

fn hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn parse_binding(text: &str) -> Result<Binding, SourceError> {
    let mut fields = fields(text)?;
    let key = ReleaseKey {
        name: take(&mut fields, "name")?,
        url: take(&mut fields, "git")?,
        subdir: take(&mut fields, "subdir")?,
        version: take(&mut fields, "version")?
            .parse()
            .map_err(|_| invalid("invalid locked release version"))?,
    };
    if !valid_name(&key.name)
        || validate_url(&key.url).is_err()
        || (!key.subdir.is_empty() && !portable_path(&key.subdir))
        || take(&mut fields, "tag")? != key.tag()
    {
        return Err(invalid("invalid locked release identity"));
    }
    let revision = take(&mut fields, "rev")?;
    let tree = take(&mut fields, "tree")?;
    let manifest = take(&mut fields, "manifest")?;
    let native = if let Some(adapter) = fields.remove("adapter") {
        Some((adapter, take(&mut fields, "recipe")?))
    } else {
        None
    };
    if !hex(&revision, 40)
        || !hex(&tree, 40)
        || !hex(&manifest, 64)
        || native
            .as_ref()
            .is_some_and(|(id, recipe)| id.is_empty() || !hex(recipe, 64))
        || !fields.is_empty()
    {
        return Err(invalid("invalid version-2 release record"));
    }
    let binding = Binding {
        key,
        revision,
        tree,
        manifest,
        native,
    };
    if render_binding(&binding) != text {
        return Err(invalid(
            "version-2 release records must use canonical generated formatting",
        ));
    }
    Ok(binding)
}

pub(super) fn render_binding(binding: &Binding) -> String {
    let key = &binding.key;
    let mut record = format!(
        "{{ name = {}, git = {}, subdir = {}, version = {}, tag = {}, rev = {}, tree = {}, manifest = {}",
        quote(&key.name),
        quote(&key.url),
        quote(&key.subdir),
        quote(&key.version.to_string()),
        quote(&key.tag()),
        quote(&binding.revision),
        quote(&binding.tree),
        quote(&binding.manifest)
    );
    if let Some((adapter, recipe)) = &binding.native {
        record.push_str(&format!(
            ", adapter = {}, recipe = {}",
            quote(adapter),
            quote(recipe)
        ));
    }
    record.push_str(" }");
    record
}

pub(super) fn render_bindings(bindings: &Bindings) -> String {
    let mut text = String::from("\n[releases]\n");
    for (index, binding) in bindings.values().enumerate() {
        text.push_str(&format!("{index} = {}\n", render_binding(binding)));
    }
    text
}

fn project_record(name: &str, record: &str) -> Result<String, SourceError> {
    let values = fields(record)?;
    let order = [
        "manifest", "path", "git", "rev", "subdir", "tree", "version", "tag", "adapter", "recipe",
    ];
    if !valid_name(name)
        || values.keys().any(|key| !order.contains(&key.as_str()))
        || !values.get("manifest").is_some_and(|value| hex(value, 64))
        || values.contains_key("adapter") != values.contains_key("recipe")
        || values.get("recipe").is_some_and(|value| !hex(value, 64))
    {
        return Err(invalid("invalid version-2 project record"));
    }
    if values.contains_key("path") {
        if ["git", "rev", "subdir", "tree", "version", "tag"]
            .iter()
            .any(|key| values.contains_key(*key))
            || values["path"].is_empty()
            || values["path"].contains('\0')
        {
            return Err(invalid("invalid local project record"));
        }
    } else {
        if !values
            .get("git")
            .is_some_and(|value| validate_url(value).is_ok())
            || !values.get("rev").is_some_and(|value| hex(value, 40))
            || !values.get("tree").is_some_and(|value| hex(value, 40))
            || !values
                .get("subdir")
                .is_some_and(|value| value.is_empty() || portable_path(value))
            || values.contains_key("version") != values.contains_key("tag")
        {
            return Err(invalid("invalid Git project record"));
        }
        if let Some(version) = values.get("version") {
            let version: Version = version
                .parse()
                .map_err(|_| invalid("invalid selected release version"))?;
            if values["tag"] != format!("{name}/v{version}") {
                return Err(invalid("invalid selected release tag"));
            }
        }
    }
    let body = order
        .iter()
        .filter_map(|key| {
            values
                .get(*key)
                .map(|value| format!("{key} = {}", quote(value)))
        })
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!("{name} = {{ {body} }}\n"))
}

pub(super) fn read(text: Option<&str>) -> Result<Bindings, SourceError> {
    let Some(text) = text else {
        return Ok(BTreeMap::new());
    };
    if text.starts_with("version = 1\n") {
        return Ok(BTreeMap::new());
    }
    let content = text
        .strip_prefix("version = 2\n\n[projects]\n")
        .ok_or_else(|| {
            invalid("missing or mismatched lockfile; unsupported or malformed header; restore a generated zore.lock")
        })?;
    let (projects, releases) = content
        .split_once("\n[releases]\n")
        .ok_or_else(|| invalid("version-2 lockfile requires release provenance"))?;
    let mut project_records = BTreeMap::new();
    for line in projects.lines() {
        let (name, record) = line
            .split_once(" = ")
            .ok_or_else(|| invalid("malformed locked project"))?;
        if project_records
            .insert(name, project_record(name, record)?)
            .is_some()
        {
            return Err(invalid("duplicate locked project"));
        }
    }
    let mut bindings = BTreeMap::new();
    for (index, line) in releases.lines().enumerate() {
        let (number, record) = line
            .split_once(" = ")
            .ok_or_else(|| invalid("malformed locked release"))?;
        if number != index.to_string() {
            return Err(invalid("noncanonical release numbering"));
        }
        let binding = parse_binding(record)?;
        if bindings.insert(binding.key.clone(), binding).is_some() {
            return Err(invalid("duplicate locked release identity"));
        }
    }
    if bindings.is_empty() {
        return Err(invalid("version-2 lockfile requires release provenance"));
    }
    let canonical = format!(
        "version = 2\n\n[projects]\n{}{}",
        project_records.into_values().collect::<String>(),
        render_bindings(&bindings)
    );
    if text != canonical {
        return Err(invalid(
            "version-2 lockfile must use canonical generated formatting",
        ));
    }
    Ok(bindings)
}

pub(super) fn selected(text: &str) -> Result<Vec<Binding>, SourceError> {
    let bindings = read(Some(text))?;
    if text.starts_with("version = 1\n") {
        return Ok(Vec::new());
    }
    let projects = text
        .strip_prefix("version = 2\n\n[projects]\n")
        .and_then(|content| content.split_once("\n[releases]\n"))
        .expect("validated lockfile")
        .0;
    let mut selected = Vec::new();
    for line in projects.lines() {
        let (name, record) = line.split_once(" = ").expect("validated project");
        let fields = fields(record)?;
        let Some(version) = fields.get("version") else {
            continue;
        };
        let key = ReleaseKey {
            name: name.into(),
            url: fields["git"].clone(),
            subdir: fields["subdir"].clone(),
            version: version.parse().expect("validated version"),
        };
        let binding = bindings
            .get(&key)
            .ok_or_else(|| invalid("selected release has no locked provenance"))?;
        if fields["rev"] != binding.revision
            || fields["tree"] != binding.tree
            || fields["manifest"] != binding.manifest
            || fields.get("adapter").zip(fields.get("recipe"))
                != binding
                    .native
                    .as_ref()
                    .map(|(adapter, recipe)| (adapter, recipe))
        {
            return Err(invalid("selected release disagrees with locked provenance"));
        }
        selected.push(binding.clone());
    }
    Ok(selected)
}
