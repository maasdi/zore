use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use zore::build::TempDir;
use zore::driver::project::{FileSystem, Memory};
use zore::driver::release::{Metadata, Tag, Version, validate};

const MINIMAL: &str = "name = 'greeting'\nversion = '1.2.3'\nlicense = 'MIT'\n";

fn memory(text: &str) -> Memory {
    Memory::new().with("release/zore.toml", text)
}

fn validate_text(text: &str, tag: Option<&str>) -> Result<String, String> {
    let files = memory(text);
    let manifest = files
        .manifest(Path::new("release"))
        .map_err(|error| error.to_string())?
        .unwrap();
    validate(&files, &manifest, tag)
        .map(|report| report.tag.to_string())
        .map_err(|error| error.to_string())
}

fn rejected(text: &str, message: &str) {
    let error = validate_text(text, None).unwrap_err();
    assert!(error.contains(message), "{error}");
}

#[test]
fn versions_are_canonical_and_order_numerically_without_integer_overflow() {
    for text in ["0.0.0", "1.2.3", "999999999999999999999999999999.0.0"] {
        assert_eq!(text.parse::<Version>().unwrap().to_string(), text);
    }
    for text in [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "+1.2.3",
        "v1.2.3",
        " 1.2.3",
        "1.2.3\n",
        "1.2.3-alpha",
        "1.2.3+build",
        "^1.2.3",
        "１.2.3",
        "1.-2.3",
    ] {
        assert!(text.parse::<Version>().is_err(), "{text:?}");
    }
    let versions = [
        "0.9.99",
        "0.10.0",
        "1.0.0",
        "1.0.9",
        "1.0.10",
        "1.9.100",
        "1.10.0",
        "9.0.0",
        "10.0.0",
        "999999999999999999999999999999.0.0",
    ];
    for pair in versions.windows(2) {
        assert!(pair[0].parse::<Version>().unwrap() < pair[1].parse::<Version>().unwrap());
    }
}

#[test]
fn tags_and_manifest_release_identity_agree_exactly() {
    assert_eq!(
        validate_text(MINIMAL, Some("greeting/v1.2.3")).unwrap(),
        "greeting/v1.2.3"
    );
    for tag in [
        "other/v1.2.3",
        "greeting/v1.2.4",
        "v1.2.3",
        "greeting/1.2.3",
        "greeting/v01.2.3",
        "std/v1.2.3",
        "greeting/v1.2.3/extra",
    ] {
        assert!(validate_text(MINIMAL, Some(tag)).is_err(), "{tag}");
    }
    assert_eq!(
        "hello-world_2/v0.0.0".parse::<Tag>().unwrap().to_string(),
        "hello-world_2/v0.0.0"
    );
}

#[test]
fn only_release_version_and_licensing_are_required_beyond_project_name() {
    assert_eq!(validate_text(MINIMAL, None).unwrap(), "greeting/v1.2.3");
    rejected(
        "name = 'greeting'\nlicense = 'MIT'\n",
        "release requires version",
    );
    rejected(
        "name = 'greeting'\nversion = '1.2.3'\n",
        "release requires license or license-file",
    );
    rejected(
        &MINIMAL.replace("1.2.3", "development"),
        "canonical MAJOR.MINOR.PATCH",
    );
    rejected(&MINIMAL.replace("greeting", "std"), "valid `name`");
}

#[test]
fn spdx_expressions_use_recognized_licenses_and_exceptions() {
    for license in [
        "MIT",
        "Apache-2.0",
        "MIT OR Apache-2.0",
        "MIT and Apache-2.0",
        "GPL-2.0",
        "GPL-2.0+",
        "GPL-3.0+",
        "LGPL-2.1+",
        "MIT OR GPL-2.0+",
        "GPL-2.0+ WITH Classpath-exception-2.0",
        "(MIT OR Apache-2.0) AND BSD-3-Clause",
        "GPL-2.0-only WITH Classpath-exception-2.0",
    ] {
        assert!(
            validate_text(&MINIMAL.replace("MIT", license), None).is_ok(),
            "{license}"
        );
    }
    for license in [
        "",
        "made-up-license",
        "MIT OR",
        "GPL-2.0++",
        "GPL-3.0-or-later+",
        "MIT / Apache-2.0",
        "apache2",
        "MIT WITH made-up-exception",
        "LicenseRef-custom",
        "MIT WITH AdditionRef-custom",
    ] {
        assert!(
            validate_text(&MINIMAL.replace("MIT", license), None).is_err(),
            "{license}"
        );
    }
}

#[test]
fn optional_fields_parse_and_validate_without_becoming_required() {
    let files = memory(&format!("{MINIMAL}description = 'A greeting library'\nreadme = 'README.md'\nlicense-file = 'licenses/COPYING.txt'\nrepository = 'https://example.org/team/libraries.git'\ndocumentation = 'https://example.org'\nzore-min = '0.1.0'\nzore-max = '0.2.0'\nplatforms = ['linux', 'macos', 'windows',] # supported hosts\n"))
        .with("release/README.md", "Usage")
        .with("release/licenses/COPYING.txt", "License terms");
    let manifest = files.manifest(Path::new("release")).unwrap().unwrap();
    let report = validate(&files, &manifest, None).unwrap();
    assert!(report.warnings.is_empty());
    assert_eq!(manifest.version.as_deref(), Some("1.2.3"));
    assert_eq!(manifest.metadata.platforms.unwrap().len(), 3);
    for field in [
        "description = ''",
        "description = \"bad\\ntext\"",
        "platforms = []",
        "platforms = ['linux', 'linux']",
        "platforms = ['freebsd']",
        "platforms = ['linux' 'windows']",
        "platforms = ['linux',,]",
        "platforms = [1]",
        "zore-min = '0.01.0'",
        "zore-max = '0.2'",
        "zore-min = '0.2.0'\nzore-max = '0.2.0'",
        "license = 'Apache-2.0'",
        "readme = 'a'\nreadme = 'b'",
        "platforms = ['linux']\nplatforms = ['windows']",
        "unknown = 'field'",
    ] {
        assert!(
            validate_text(&format!("{MINIMAL}{field}\n"), None).is_err(),
            "{field}"
        );
    }
}

#[test]
fn compiler_distribution_suffixes_do_not_change_numeric_support_bounds() {
    let core = format!(
        "{}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    assert!(validate_text(&format!("{MINIMAL}zore-min = '{core}'\n"), None).is_ok());
    let failure = validate_text(&format!("{MINIMAL}zore-max = '{core}'\n"), None).unwrap_err();
    assert!(
        failure.contains(&format!("requires Zore < {core}")),
        "{failure}"
    );
}

#[test]
fn optional_links_cannot_embed_credentials_or_redirect_source_selection() {
    for field in [
        "documentation = 'http://example.org/'",
        "documentation = 'https://user:secret@example.org/'",
        "documentation = 'https:///'",
        "documentation = 'https://example.org/?token=secret'",
        "documentation = 'https://example.org/#fragment'",
        "documentation = 'https://exa mple.org/'",
        "documentation = 'https://example.org:bad/'",
        "documentation = 'https://example.org:65536/'",
        "documentation = 'https://[bad]/'",
        "documentation = 'https://-invalid.example/'",
        "documentation = 'https://bad_host/'",
        "documentation = 'https://example.org\\bad/'",
        "repository = 'https://user:secret@example.org/repo'",
        "repository = 'https://example.org/repo?secret'",
        "repository = 'https://example.org/re po'",
    ] {
        let error = validate_text(&format!("{MINIMAL}{field}\n"), None).unwrap_err();
        assert!(!error.contains("secret"), "{error}");
    }
    for url in [
        "https://example.org",
        "https://example.org/",
        "https://example.org:443/docs",
        "https://[::1]:443/docs",
        "https://127.0.0.1/docs",
    ] {
        assert!(
            validate_text(&format!("{MINIMAL}documentation = '{url}'\n"), None).is_ok(),
            "{url}"
        );
    }
    let files = memory(&format!(
        "{MINIMAL}repository = 'https://descriptive.example/repo'\n[dependencies]\nchild = {{ git = 'https://actual.example/repo', rev = '{}' }}\n",
        "a".repeat(40)
    ));
    let manifest = files.manifest(Path::new("release")).unwrap().unwrap();
    let zore::driver::project::Dependency::Git { url, .. } = &manifest.dependencies["child"] else {
        panic!()
    };
    assert_eq!(url, "https://actual.example/repo");
    assert!(validate(&files, &manifest, None).is_ok());
}

#[test]
fn compiler_bounds_are_inclusive_minimum_exclusive_maximum() {
    let metadata = Metadata {
        zore_min: Some("1.2.3".into()),
        zore_max: Some("2.0.0".into()),
        platforms: Some(vec!["linux".into()]),
        ..Default::default()
    };
    for version in ["1.2.3", "1.2.4", "1.10.0", "1.999.999"] {
        assert!(
            metadata
                .validate_support(&version.parse().unwrap(), "linux")
                .is_ok()
        );
    }
    for version in ["0.99.99", "1.2.2", "2.0.0", "2.0.1"] {
        assert!(
            metadata
                .validate_support(&version.parse().unwrap(), "linux")
                .is_err()
        );
    }
    for platform in ["macos", "windows"] {
        assert!(
            metadata
                .validate_support(&"1.2.3".parse().unwrap(), platform)
                .is_err()
        );
    }
}

#[test]
fn supplied_metadata_is_enforced_for_every_project_without_requiring_release_fields() {
    let files = Memory::new()
        .with("app/zore.toml", "name = 'app'\nversion = 'work in progress'\n[dependencies]\nchild = { path = '../child' }\n")
        .with("child/zore.toml", "name = 'child'\nzore-min = '999.0.0'\n");
    let root = files.manifest(Path::new("app")).unwrap().unwrap();
    assert!(
        files
            .projects(root)
            .err()
            .unwrap()
            .to_string()
            .contains("requires Zore >= 999.0.0")
    );
    let files = memory("name = 'greeting'\nversion = 'unchecked legacy'\n");
    let root = files.manifest(Path::new("release")).unwrap().unwrap();
    assert_eq!(root.version.as_deref(), Some("unchecked legacy"));
    assert!(files.projects(root).is_ok());
    let unsupported = if std::env::consts::OS == "windows" {
        "linux"
    } else {
        "windows"
    };
    let files = memory(&format!(
        "name = 'greeting'\nplatforms = ['{unsupported}']\n"
    ));
    let root = files.manifest(Path::new("release")).unwrap().unwrap();
    assert!(
        files
            .projects(root)
            .err()
            .unwrap()
            .to_string()
            .contains("current host")
    );
}

#[test]
fn metadata_paths_are_portable_confined_readable_and_utf8() {
    for path in [
        "",
        ".",
        "..",
        "../COPYING",
        "/COPYING",
        "C:/COPYING",
        "a\\b",
        "a//b",
        "a/./b",
        "a/../b",
        ".git/config",
        "NUL",
        "a./COPYING",
    ] {
        rejected(
            &format!("{MINIMAL}license-file = '{path}'\n"),
            "project-relative file path",
        );
    }
    for field in ["readme", "license-file"] {
        rejected(&format!("{MINIMAL}{field} = 'missing'\n"), "no such file");
    }
    let files = memory("name = 'greeting'\nversion = '1.2.3'\nlicense-file = 'COPYING'\n")
        .with("release/COPYING", "Custom licensing terms");
    let manifest = files.manifest(Path::new("release")).unwrap().unwrap();
    assert!(validate(&files, &manifest, None).is_ok());
    let files = files.with("release/COPYING", " \n\t");
    assert!(
        validate(&files, &manifest, None)
            .unwrap_err()
            .to_string()
            .contains("must not be empty")
    );
}

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("release café with spaces");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("zore.toml"), MINIMAL).unwrap();
        Self {
            _temporary: temporary,
            root,
        }
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_zore"))
            .args(args)
            .arg(&self.root)
            .env("PATH", "")
            .env("ZORE_CC", self.root.join("missing-cc"))
            .env("ZORE_RUSTC", self.root.join("missing-rustc"))
            .env("ZORE_CACHE_DIR", self.root.join("cache"))
            .output()
            .unwrap()
    }
}

fn failure(output: Output, code: i32, message: &str) {
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{output:?}"
    );
}

#[test]
fn preflight_is_local_metadata_only_without_fetch_execution_or_lock_writes() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("zore.toml"), format!("{MINIMAL}[dependencies]\nchild = {{ git = 'https://unavailable.invalid/repo', rev = '{}' }}\n", "a".repeat(40))).unwrap();
    fs::write(
        fixture.root.join("main.ore"),
        "this is deliberately invalid source",
    )
    .unwrap();
    fs::write(fixture.root.join("zore.lock"), "lock sentinel").unwrap();
    let output = fixture.command(&[
        "check",
        "--release",
        "--offline",
        "--tag",
        "greeting/v1.2.3",
    ]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("greeting/v1.2.3 (tag not looked up)")
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("warning:"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("zore.lock")).unwrap(),
        "lock sentinel"
    );
    assert!(!fixture.root.join("cache").exists());
    fs::remove_file(fixture.root.join("zore.lock")).unwrap();
    assert!(fixture.command(&["check", "--release"]).status.success());
    assert!(!fixture.root.join("zore.lock").exists());
    failure(
        fixture.command(&["check", "--release", "--tag", "other/v1.2.3"]),
        1,
        "expected `greeting/v1.2.3`",
    );
}

#[test]
fn release_flags_have_explicit_usage_rules_and_help() {
    let fixture = Fixture::new();
    for args in [
        vec!["build", "--release"],
        vec!["check", "--release", "--release"],
        vec![
            "check",
            "--release",
            "--tag",
            "greeting/v1.2.3",
            "--tag",
            "greeting/v1.2.3",
        ],
    ] {
        failure(fixture.command(&args), 2, "unknown option");
    }
    failure(
        fixture.command(&["check", "--tag", "greeting/v1.2.3"]),
        2,
        "--tag requires --release",
    );
    failure(
        fixture.command(&["check", "--release", "--locked"]),
        2,
        "does not accept --locked",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_zore"))
        .args(["check", "--release", "--tag"])
        .output()
        .unwrap();
    failure(output, 2, "--tag requires a UTF-8");
    let help = Command::new(env!("CARGO_BIN_EXE_zore"))
        .args(["check", "--help"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).contains("--tag checks a supplied string only"));
}

#[test]
fn disk_metadata_rejects_directories_and_invalid_utf8_files() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("README")).unwrap();
    fs::write(
        fixture.root.join("zore.toml"),
        format!("{MINIMAL}readme = 'README'\n"),
    )
    .unwrap();
    failure(fixture.command(&["check", "--release"]), 1, "regular file");
    fs::remove_dir(fixture.root.join("README")).unwrap();
    fs::write(fixture.root.join("README"), [0xff, 0xfe]).unwrap();
    failure(fixture.command(&["check", "--release"]), 1, "UTF-8");
}

#[test]
fn ordinary_checks_and_version_one_locks_preserve_legacy_version_metadata() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("zore.toml"),
        "name = 'app'\nversion = 'development snapshot'\n",
    )
    .unwrap();
    fs::write(
        fixture.root.join("main.ore"),
        "package main\nfunc main() {}\n",
    )
    .unwrap();
    let output = fixture.command(&["lock", "--offline"]);
    assert!(output.status.success(), "{output:?}");
    let lock = fs::read(fixture.root.join("zore.lock")).unwrap();
    assert!(lock.starts_with(b"version = 1\n"));
    let output = Command::new(env!("CARGO_BIN_EXE_zore"))
        .args(["check", "--locked", "--offline"])
        .arg(fixture.root.join("main.ore"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(fixture.root.join("zore.lock")).unwrap(), lock);
}

#[cfg(unix)]
#[test]
fn symlinked_metadata_cannot_escape_the_project_but_internal_links_work() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    fs::write(fixture._temporary.path().join("outside"), "terms").unwrap();
    symlink(
        fixture._temporary.path().join("outside"),
        fixture.root.join("COPYING"),
    )
    .unwrap();
    fs::write(
        fixture.root.join("zore.toml"),
        format!("{MINIMAL}license-file = 'COPYING'\n"),
    )
    .unwrap();
    failure(fixture.command(&["check", "--release"]), 1, "stay inside");
    fs::remove_file(fixture.root.join("COPYING")).unwrap();
    fs::write(fixture.root.join("terms"), "terms").unwrap();
    symlink("terms", fixture.root.join("COPYING")).unwrap();
    assert!(fixture.command(&["check", "--release"]).status.success());
}

#[test]
fn native_release_preflight_reuses_reviewed_source_recipe_without_a_toolchain() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("zore.toml"), "name = 'adler32'\nversion = '1.0.0'\nlicense = 'Apache-2.0'\n[native]\nadapter = 'adler32-v1'\n").unwrap();
    fs::create_dir(fixture.root.join("native")).unwrap();
    for (file, text) in [
        (
            "bindings.abi",
            include_str!("../../examples/native-library/adler32/native/bindings.abi"),
        ),
        (
            "adler32.c",
            include_str!("../../examples/native-library/adler32/native/adler32.c"),
        ),
        (
            "adler32.h",
            include_str!("../../examples/native-library/adler32/native/adler32.h"),
        ),
        (
            "bridge.rs",
            include_str!("../../examples/native-library/adler32/native/bridge.rs"),
        ),
    ] {
        fs::write(fixture.root.join("native").join(file), text).unwrap();
    }
    let output = fixture.command(&["check", "--release"]);
    assert!(output.status.success(), "{output:?}");
    fs::write(fixture.root.join("native/adler32.c"), "unapproved source").unwrap();
    failure(
        fixture.command(&["check", "--release"]),
        1,
        "do not match reviewed adapter",
    );
}

#[test]
fn preflight_supports_option_shaped_names_and_nearest_manifest_targets() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("zore.toml"),
        MINIMAL.replace("greeting", "-greeting"),
    )
    .unwrap();
    let output = fixture.command(&["check", "--release", "--tag", "-greeting/v1.2.3"]);
    assert!(output.status.success(), "{output:?}");
    fs::write(
        fixture.root.join("source.ore"),
        "not parsed by metadata preflight",
    )
    .unwrap();
    fs::create_dir(fixture.root.join("nested")).unwrap();
    for target in ["source.ore", "nested"] {
        let output = Command::new(env!("CARGO_BIN_EXE_zore"))
            .args(["check", "--release", "--tag", "-greeting/v1.2.3", target])
            .current_dir(&fixture.root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_zore"))
        .args(["check", "--release", "missing"])
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    failure(output, 1, "existing file or directory");
}

#[cfg(unix)]
#[test]
fn non_utf8_tag_values_are_usage_errors() {
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_zore"))
        .args(["check", "--release", "--tag"])
        .arg(std::ffi::OsString::from_vec(vec![0xff]))
        .arg(&fixture.root)
        .output()
        .unwrap();
    failure(output, 2, "--tag requires a UTF-8");
}

#[test]
fn deprecated_gnu_licenses_work_in_release_and_ordinary_checks() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("main.ore"),
        "package main\nfunc main() {}\n",
    )
    .unwrap();
    for license in [
        "GPL-2.0+",
        "GPL-3.0+",
        "LGPL-2.1+",
        "MIT OR GPL-2.0+",
        "GPL-2.0+ WITH Classpath-exception-2.0",
    ] {
        fs::write(
            fixture.root.join("zore.toml"),
            format!("name = 'greeting'\nversion = '1.2.3'\nlicense = '{license}'\ndescription = 'Greeting library'\n"),
        ).unwrap();
        for release in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_zore"));
            command.args(["check", "--offline"]).env("PATH", "");
            if release {
                command.arg("--release").arg(&fixture.root);
            } else {
                command.arg(fixture.root.join("main.ore"));
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{license}, release={release}: {output:?}"
            );
            assert!(output.stderr.is_empty(), "{output:?}");
            if release {
                assert!(String::from_utf8_lossy(&output.stdout).contains("greeting/v1.2.3"));
            } else {
                assert!(output.stdout.is_empty(), "{output:?}");
            }
        }
    }
}
