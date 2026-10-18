//! Validates local release metadata without resolving or executing packages.

use std::cmp::Ordering;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use super::project::manifest::{manifest_error, portable_path, valid_name, validate_url};
use super::project::{Disk, FileSystem, Manifest};
use crate::source::SourceError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Version([String; 3]);

impl FromStr for Version {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let components: Vec<_> = text.split('.').collect();
        if components.len() != 3
            || components.iter().any(|part| {
                part.is_empty()
                    || !part.bytes().all(|byte| byte.is_ascii_digit())
                    || (part.len() > 1 && part.starts_with('0'))
            })
        {
            return Err(
                "expected canonical MAJOR.MINOR.PATCH without leading zeroes or suffixes".into(),
            );
        }
        Ok(Self(std::array::from_fn(|index| components[index].into())))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .iter()
            .zip(&other.0)
            .map(|(left, right)| left.len().cmp(&right.len()).then_with(|| left.cmp(right)))
            .find(|order| *order != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tag {
    pub name: String,
    pub version: Version,
}

impl FromStr for Tag {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (name, version) = text
            .split_once("/v")
            .ok_or("expected <name>/v<version> tag")?;
        if !valid_name(name) {
            return Err("release tag has an invalid project name".into());
        }
        Ok(Self {
            name: name.into(),
            version: version.parse()?,
        })
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/v{}", self.name, self.version)
    }
}

fn valid_https_authority(url: &str) -> bool {
    let Some(authority) = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
    else {
        return false;
    };
    let valid_port = |port: &str| {
        !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok()
    };
    if let Some(address) = authority.strip_prefix('[') {
        let Some((address, suffix)) = address.split_once(']') else {
            return false;
        };
        return address.parse::<std::net::Ipv6Addr>().is_ok()
            && (suffix.is_empty() || suffix.strip_prefix(':').is_some_and(valid_port));
    }
    let host = if let Some((host, port)) = authority.split_once(':') {
        if !valid_port(port) {
            return false;
        }
        host
    } else {
        authority
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub description: Option<String>,
    pub readme: Option<String>,
    pub repository: Option<String>,
    pub documentation: Option<String>,
    pub zore_min: Option<String>,
    pub zore_max: Option<String>,
    pub platforms: Option<Vec<String>>,
}

impl Metadata {
    pub(crate) fn validate_fields(&self) -> Result<(), String> {
        if let Some(license) = &self.license {
            let mode = spdx::ParseMode {
                allow_deprecated: true,
                ..spdx::ParseMode::STRICT
            };
            let expression = spdx::Expression::parse_mode(license, mode)
                .map_err(|_| "license must be a recognized SPDX expression; use license-file for custom terms")?;
            if expression.requirements().any(|requirement| {
                requirement.req.license.id().is_none()
                    || requirement
                        .req
                        .addition
                        .as_ref()
                        .is_some_and(|addition| addition.id().is_none())
            }) {
                return Err("custom SPDX identifiers require license-file instead".into());
            }
        }
        for (field, path) in [
            ("readme", &self.readme),
            ("license-file", &self.license_file),
        ] {
            if let Some(path) = path
                && !portable_path(path)
            {
                return Err(format!(
                    "{field} must be a portable project-relative file path"
                ));
            }
        }
        if let Some(description) = &self.description
            && (description.trim().is_empty() || description.chars().any(char::is_control))
        {
            return Err("description must be nonempty text without control characters".into());
        }
        if let Some(repository) = &self.repository {
            validate_url(repository)
                .map_err(|_| "repository must be a credential-free explicit Git URL")?;
            if repository.chars().any(char::is_whitespace) {
                return Err("repository URL must not contain whitespace".into());
            }
        }
        if let Some(documentation) = &self.documentation
            && (!documentation.starts_with("https://")
                || documentation
                    .chars()
                    .any(|character| character.is_whitespace() || character.is_control())
                || documentation.contains(['?', '#', '\\'])
                || !valid_https_authority(documentation))
        {
            return Err(
                "documentation must be a credential-free HTTPS URL without query or fragment"
                    .into(),
            );
        }
        let minimum = self
            .zore_min
            .as_deref()
            .map(str::parse::<Version>)
            .transpose()
            .map_err(|message| format!("zore-min: {message}"))?;
        let maximum = self
            .zore_max
            .as_deref()
            .map(str::parse::<Version>)
            .transpose()
            .map_err(|message| format!("zore-max: {message}"))?;
        if let (Some(minimum), Some(maximum)) = (minimum, maximum)
            && minimum >= maximum
        {
            return Err("zore-min must be less than exclusive zore-max".into());
        }
        if let Some(platforms) = &self.platforms {
            let mut seen = std::collections::HashSet::new();
            if platforms.is_empty()
                || platforms.iter().any(|platform| {
                    !matches!(platform.as_str(), "linux" | "macos" | "windows")
                        || !seen.insert(platform)
                })
            {
                return Err("platforms must contain distinct linux, macos or windows names and must not be empty".into());
            }
        }
        Ok(())
    }

    /// Checks declared support against an explicit compiler version and host OS.
    pub fn validate_support(&self, compiler: &Version, platform: &str) -> Result<(), String> {
        self.validate_fields()?;
        if let Some(minimum) = &self.zore_min
            && compiler < &minimum.parse::<Version>()?
        {
            return Err(format!(
                "requires Zore >= {minimum}; current compiler is {compiler}"
            ));
        }
        if let Some(maximum) = &self.zore_max
            && compiler >= &maximum.parse::<Version>()?
        {
            return Err(format!(
                "requires Zore < {maximum}; current compiler is {compiler}"
            ));
        }
        if let Some(platforms) = &self.platforms
            && !platforms.iter().any(|supported| supported == platform)
        {
            return Err(format!(
                "declared platforms do not include current host `{platform}`"
            ));
        }
        Ok(())
    }
}

/// Validates all supplied metadata, including for local and revision-pinned projects.
pub fn validate_metadata(files: &dyn FileSystem, manifest: &Manifest) -> Result<(), SourceError> {
    manifest
        .metadata
        .validate_support(
            &env!("CARGO_PKG_VERSION")
                .parse()
                .expect("canonical compiler version"),
            std::env::consts::OS,
        )
        .map_err(|message| manifest_error(&manifest.root, message))?;
    for (field, path) in [
        ("readme", &manifest.metadata.readme),
        ("license-file", &manifest.metadata.license_file),
    ] {
        let Some(path) = path else { continue };
        let path = files.canonical(&manifest.root.join(path));
        if !path.starts_with(files.canonical(&manifest.root)) {
            return Err(manifest_error(
                &manifest.root,
                format!("{field} must stay inside its declaring project"),
            ));
        }
        let text = files.read_regular(&path)?;
        if field == "license-file" && text.trim().is_empty() {
            return Err(manifest_error(
                &manifest.root,
                "license-file must not be empty".into(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct Report {
    pub tag: Tag,
    pub warnings: Vec<String>,
}

/// Checks release identity and metadata; it never resolves Git tags or dependencies.
pub fn validate(
    files: &dyn FileSystem,
    manifest: &Manifest,
    expected_tag: Option<&str>,
) -> Result<Report, SourceError> {
    let fail = |message| manifest_error(&manifest.root, message);
    if !valid_name(&manifest.name) {
        return Err(fail("release requires a valid project name".into()));
    }
    validate_metadata(files, manifest)?;
    let version = manifest
        .version
        .as_deref()
        .ok_or_else(|| fail("release requires version".into()))?
        .parse()
        .map_err(fail)?;
    if manifest.metadata.license.is_none() && manifest.metadata.license_file.is_none() {
        return Err(fail("release requires license or license-file".into()));
    }
    let tag = Tag {
        name: manifest.name.clone(),
        version,
    };
    if let Some(expected) = expected_tag
        && expected.parse::<Tag>().map_err(fail)? != tag
    {
        return Err(fail(format!(
            "release tag does not match manifest; expected `{tag}`"
        )));
    }
    if manifest.native.is_some() {
        super::native::load(files, manifest)?;
    }
    let mut warnings = Vec::new();
    if manifest.metadata.description.is_none() && manifest.metadata.readme.is_none() {
        warnings.push("consider adding description or readme for users".into());
    }
    Ok(Report { tag, warnings })
}

pub(super) fn run(target: &Path, expected_tag: Option<&str>) -> std::process::ExitCode {
    let result = (|| {
        let directory = if target.is_dir() {
            target
        } else {
            if !target.is_file() {
                return Err(manifest_error(
                    target,
                    "release target must be an existing file or directory".into(),
                ));
            }
            target
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
        };
        let manifest = Disk.manifest(directory)?.ok_or_else(|| {
            manifest_error(directory, "release validation requires zore.toml".into())
        })?;
        validate(&Disk, &manifest, expected_tag)
    })();
    match result {
        Ok(report) => {
            for warning in report.warnings {
                eprintln!("warning: {warning}");
            }
            println!("Release metadata valid: {} (tag not looked up)", report.tag);
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("zore: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
