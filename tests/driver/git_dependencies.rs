use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use zore::build::TempDir;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn remove_path(path: &Path) {
    #[cfg(windows)]
    {
        #[allow(
            clippy::permissions_set_readonly_false,
            reason = "Only Windows read-only file attributes are cleared"
        )]
        fn writable(path: &Path) {
            let metadata = fs::symlink_metadata(path).unwrap();
            if metadata.file_type().is_symlink() {
                return;
            }
            if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    writable(&entry.unwrap().path());
                }
            } else {
                let mut permissions = metadata.permissions();
                permissions.set_readonly(false);
                fs::set_permissions(path, permissions).unwrap();
            }
        }
        writable(path);
    }
    if path.is_dir() {
        fs::remove_dir_all(path).unwrap();
    } else {
        fs::remove_file(path).unwrap();
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "core.autocrlf=false",
            "-c",
            "user.name=Zore tests",
            "-c",
            "user.email=zore@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn commit(root: &Path) -> String {
    git(root, &["add", "--all"]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root, &["rev-parse", "HEAD"])
}

fn url(root: &Path) -> String {
    let path = root.to_str().unwrap().replace('\\', "/");
    format!(
        "file://{}{path}",
        if path.starts_with('/') { "" } else { "/" }
    )
}

fn selection(repository: &Path, revision: &str, subdir: &str) -> String {
    format!(
        "{{ git = '{}', rev = '{revision}', subdir = '{subdir}' }}",
        url(repository)
    )
}

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
    repository: PathBuf,
    app: PathBuf,
    cache: PathBuf,
    revision: String,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("Git projects café with spaces");
        let repository = root.join("repository");
        fs::create_dir_all(&repository).unwrap();
        git(&repository, &["init", "-q", "--initial-branch=main"]);
        write(&repository, "zore.toml", "name = 'auth'\n");
        write(
            &repository,
            "auth.ore",
            "package auth\nfunc Value() int { return 42 }\n",
        );
        let revision = commit(&repository);
        let app = root.join("app");
        let cache = root.join("cache");
        write(
            &app,
            "zore.toml",
            &format!(
                "name = 'app'\n[dependencies]\nauth = {}\n",
                selection(&repository, &revision, "")
            ),
        );
        write(
            &app,
            "main.ore",
            "package main\nimport \"auth\"\nfunc main() { println(auth.Value()) }\n",
        );
        Self {
            _temporary: temporary,
            root,
            repository,
            app,
            cache,
            revision,
        }
    }

    fn command(&self, action: &str, flags: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zore"));
        command
            .arg(action)
            .args(flags)
            .arg(if matches!(action, "install" | "lock" | "test") {
                self.app.clone()
            } else {
                self.app.join("main.ore")
            })
            .env("ZORE_CACHE_DIR", &self.cache)
            .env("ZORE_CHECK_LEAKS", "1")
            .current_dir(&self.root);
        command
    }

    fn invoke(&self, action: &str, flags: &[&str]) -> Output {
        self.command(action, flags).output().unwrap()
    }

    fn install(&self) {
        success(self.invoke("install", &[]));
    }

    fn lock(&self) -> Vec<u8> {
        fs::read(self.app.join("zore.lock")).unwrap()
    }

    fn entry(&self) -> PathBuf {
        fs::read_dir(&self.cache)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| !p.file_name().unwrap().as_encoded_bytes().starts_with(b"."))
            .unwrap()
    }
}

fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    output
}

fn failure(output: Output, message: &str) {
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{output:?}"
    );
}

#[test]
fn fresh_install_is_explicit_repeated_and_offline_check_needs_no_linker() {
    let fixture = Fixture::new();
    failure(fixture.invoke("check", &[]), "run `zore install`");
    failure(
        fixture.invoke("install", &["--offline"]),
        "missing from the cache",
    );
    assert!(!fixture.app.join("zore.lock").exists());
    fixture.install();
    let lock = fixture.lock();
    assert!(String::from_utf8_lossy(&lock).contains(&fixture.revision));
    assert!(!String::from_utf8_lossy(&lock).contains(fixture.cache.to_str().unwrap()));
    fs::rename(&fixture.repository, fixture.root.join("unavailable")).unwrap();
    success(fixture.invoke("install", &["--locked", "--offline"]));
    success(
        fixture
            .command("check", &["--locked", "--offline"])
            .env("ZORE_CC", "missing-native-linker")
            .output()
            .unwrap(),
    );
    assert_eq!(fixture.lock(), lock);
    assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 1);
}

#[test]
fn lock_uses_exact_changed_pin_and_builds_never_rewrite_locks() {
    let fixture = Fixture::new();
    fixture.install();
    let old_lock = fixture.lock();
    write(
        &fixture.repository,
        "auth.ore",
        "package auth\nfunc Value() int { return 77 }\n",
    );
    let next = commit(&fixture.repository);
    success(fixture.invoke("install", &[]));
    assert_eq!(fixture.lock(), old_lock);
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nauth = {}\n",
            selection(&fixture.repository, &next, "")
        ),
    );
    failure(
        fixture.invoke("check", &["--locked"]),
        "missing from the cache",
    );
    assert_eq!(fixture.lock(), old_lock);
    failure(
        fixture.invoke("install", &["--locked"]),
        "mismatched lockfile",
    );
    assert_eq!(fixture.lock(), old_lock);
    success(fixture.invoke("lock", &["--offline"]));
    let new_lock = fixture.lock();
    assert_ne!(new_lock, old_lock);
    assert!(String::from_utf8_lossy(&new_lock).contains(&next));
    success(fixture.invoke("check", &["--locked", "--offline"]));
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app' # manifest drift\n[dependencies]\nauth = {}\n",
            selection(&fixture.repository, &next, "")
        ),
    );
    failure(fixture.invoke("check", &[]), "mismatched lockfile");
    failure(fixture.invoke("install", &[]), "mismatched lockfile");
    assert_eq!(fixture.lock(), new_lock);
    success(fixture.invoke("lock", &["--offline"]));
}

#[test]
fn missing_malformed_and_modified_locks_fail_without_writes() {
    let fixture = Fixture::new();
    failure(
        fixture.invoke("install", &["--locked"]),
        "--locked requires",
    );
    fixture.install();
    for lock in ["version = 99\n", "broken", "version = 1\n\n[projects]\n"] {
        write(&fixture.app, "zore.lock", lock);
        failure(
            fixture.invoke("check", &["--locked", "--offline"]),
            "mismatched lockfile",
        );
        failure(
            fixture.invoke("install", &["--offline"]),
            "mismatched lockfile",
        );
        assert_eq!(fixture.lock(), lock.as_bytes());
    }
    success(fixture.invoke("lock", &["--offline"]));
    fs::remove_file(fixture.app.join("zore.lock")).unwrap();
    failure(
        fixture.invoke("check", &["--offline"]),
        "missing or mismatched lockfile",
    );
    assert!(!fixture.app.join("zore.lock").exists());
    success(fixture.invoke("install", &["--offline"]));
    remove_path(&fixture.cache);
    failure(
        fixture.invoke("check", &["--locked", "--offline"]),
        "missing from the cache",
    );
    success(fixture.invoke("install", &["--locked"]));
}

#[test]
fn corrupt_cache_bytes_metadata_and_extra_paths_are_rejected() {
    let fixture = Fixture::new();
    fixture.install();
    let lock = fixture.lock();
    let entry = fixture.entry();
    let source = entry.join("sources/auth.ore");
    let bytes = fs::read(&source).unwrap();
    let mut changed = bytes.clone();
    let index = changed.iter().position(|c| *c == b'4').unwrap();
    changed[index] = b'9';
    fs::write(&source, &changed).unwrap();
    failure(
        fixture.invoke("check", &["--offline"]),
        "cached file bytes differ",
    );
    failure(fixture.invoke("install", &[]), "remove this cache entry");
    assert_eq!(fs::read(&source).unwrap(), changed);
    fs::write(&source, &bytes).unwrap();
    write(&entry, "sources/extra/ignored", "extra");
    failure(
        fixture.invoke("check", &["--offline"]),
        "unexpected file or directory",
    );
    fs::remove_dir_all(entry.join("sources/extra")).unwrap();
    fs::write(entry.join("source"), "wrong identity").unwrap();
    failure(fixture.invoke("check", &["--offline"]), "source identity");
    assert_eq!(fixture.lock(), lock);
    remove_path(&entry);
    success(fixture.invoke("install", &["--locked"]));
    success(fixture.invoke("check", &["--locked", "--offline"]));
}

#[test]
fn corrupt_git_objects_are_rejected_even_when_checkout_is_unchanged() {
    let fixture = Fixture::new();
    fixture.install();
    let object = fixture
        .entry()
        .join("repository.git/objects")
        .join(&fixture.revision[..2])
        .join(&fixture.revision[2..]);
    assert!(object.exists());
    remove_path(&object);
    failure(fixture.invoke("check", &["--offline"]), "corrupt Git cache");
}

#[test]
fn subdirectories_and_transitive_git_pins_are_resolved() {
    let fixture = Fixture::new();
    let shared = fixture.root.join("shared repository");
    fs::create_dir_all(&shared).unwrap();
    git(&shared, &["init", "-q"]);
    write(&shared, "zore.toml", "name = 'shared'\n");
    write(
        &shared,
        "shared.ore",
        "package shared\nfunc Value() int { return 42 }\n",
    );
    let shared_rev = commit(&shared);
    write(
        &fixture.repository,
        "library/zore.toml",
        &format!(
            "name = 'auth'\n[dependencies]\nshared = {}\n",
            selection(&shared, &shared_rev, "")
        ),
    );
    write(
        &fixture.repository,
        "library/auth.ore",
        "package auth\nimport \"shared\"\nfunc Value() int { return shared.Value() }\n",
    );
    let revision = commit(&fixture.repository);
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nauth = {}\n",
            selection(&fixture.repository, &revision, "library")
        ),
    );
    fixture.install();
    assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 2);
    let lock = String::from_utf8(fixture.lock()).unwrap();
    assert!(lock.contains("subdir = \"library\""));
    assert!(lock.contains(&shared_rev));
    success(fixture.invoke("check", &["--locked", "--offline"]));
    write(
        &fixture.app,
        "main.ore",
        "package main\nimport \"shared\"\nfunc main() { println(shared.Value()) }\n",
    );
    failure(
        fixture.invoke("check", &["--offline"]),
        "directly declared dependency",
    );
}

#[test]
fn fetched_path_dependencies_cannot_escape_or_use_absolute_paths() {
    for absolute in [false, true] {
        let fixture = Fixture::new();
        let path = if absolute {
            fixture.root.join("outside").to_str().unwrap().to_string()
        } else {
            "../../outside".into()
        };
        write(
            &fixture.repository,
            "zore.toml",
            &format!("name = 'auth'\n[dependencies]\nshared = {{ path = '{path}' }}\n"),
        );
        let revision = commit(&fixture.repository);
        write(
            &fixture.app,
            "zore.toml",
            &format!(
                "name = 'app'\n[dependencies]\nauth = {}\n",
                selection(&fixture.repository, &revision, "")
            ),
        );
        failure(
            fixture.invoke("install", &[]),
            "must stay inside its Git checkout",
        );
        assert!(!fixture.app.join("zore.lock").exists());
    }
}

#[test]
fn incompatible_pins_and_project_name_mismatches_are_diagnosed() {
    let fixture = Fixture::new();
    write(
        &fixture.repository,
        "auth.ore",
        "package auth\nfunc Value() int { return 99 }\n",
    );
    let next = commit(&fixture.repository);
    write(
        &fixture.root,
        "wrapper/zore.toml",
        &format!(
            "name = 'wrapper'\n[dependencies]\nauth = {}\n",
            selection(&fixture.repository, &next, "")
        ),
    );
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nauth = {}\nwrapper = {{ path = '../wrapper' }}\n",
            selection(&fixture.repository, &fixture.revision, "")
        ),
    );
    failure(fixture.invoke("install", &[]), "conflicting project `auth`");
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nother = {}\n",
            selection(&fixture.repository, &fixture.revision, "")
        ),
    );
    failure(
        fixture.invoke("install", &[]),
        "does not match project name",
    );
}

#[test]
fn incomplete_fetches_are_ignored_and_concurrent_cache_publication_is_safe() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.cache.join(".zore-stage-interrupted/sources")).unwrap();
    write(
        &fixture.root,
        "second/zore.toml",
        &fs::read_to_string(fixture.app.join("zore.toml")).unwrap(),
    );
    let mut first = fixture
        .command("install", &[])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let second = Command::new(env!("CARGO_BIN_EXE_zore"))
        .arg("install")
        .arg(fixture.root.join("second"))
        .env("ZORE_CACHE_DIR", &fixture.cache)
        .output()
        .unwrap();
    let status = first.wait().unwrap();
    assert!(status.success());
    success(second);
    assert_eq!(
        fixture.lock(),
        fs::read(fixture.root.join("second/zore.lock")).unwrap()
    );
    success(fixture.invoke("check", &["--locked", "--offline"]));
    write(&fixture.app, ".zore-lock.guard", "");
    failure(
        fixture.invoke("lock", &[]),
        "confirming no resolver is running",
    );
    fs::remove_file(fixture.app.join(".zore-lock.guard")).unwrap();
    success(fixture.invoke("lock", &["--offline"]));
}

#[test]
fn dependency_options_are_explicit_and_fmt_stays_independent() {
    let fixture = Fixture::new();
    for (action, flags) in [
        ("fmt", vec!["--locked"]),
        ("fmt", vec!["--offline"]),
        ("lock", vec!["--locked"]),
        ("check", vec!["--locked", "--locked"]),
        ("install", vec!["--offline", "--offline"]),
    ] {
        let output = fixture.invoke(action, &flags);
        assert_eq!(output.status.code(), Some(2));
    }
    success(fixture.command("fmt", &[]).output().unwrap());
    assert!(!fixture.app.join("zore.lock").exists());
}

#[test]
fn unsupported_pins_layouts_and_credentials_fail_without_echoing_secrets() {
    let fixture = Fixture::new();
    for fields in [
        "git = 'https://token:TOP_SECRET@example.invalid/repo', rev = '0000000000000000000000000000000000000000'",
        "git = 'ssh://git:TOP_SECRET@example.invalid/repo', rev = '0000000000000000000000000000000000000000'",
        "git = 'https://example.invalid/repo?token=TOP_SECRET', rev = '0000000000000000000000000000000000000000'",
        "git = 'https://example.invalid/repo', rev = 'main'",
        "git = 'https://example.invalid/repo', rev = '0123456'",
        "git = 'https://example.invalid/repo', rev = '0000000000000000000000000000000000000000', subdir = '../auth'",
        "git = 'https://example.invalid/repo', rev = '0000000000000000000000000000000000000000', path = '../auth'",
        "git = 'https://example.invalid/repo', rev = '0000000000000000000000000000000000000000', branch = 'main'",
    ] {
        write(
            &fixture.app,
            "zore.toml",
            &format!("name = 'app'\n[dependencies]\nauth = {{ {fields} }}\n"),
        );
        let output = fixture.invoke("install", &["--offline"]);
        assert!(!output.status.success(), "{fields}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("TOP_SECRET"),
            "{output:?}"
        );
        assert!(!fixture.app.join("zore.lock").exists());
    }
}

#[test]
fn local_only_projects_can_lock_manifests_while_sources_remain_mutable() {
    let fixture = Fixture::new();
    write(
        &fixture.app,
        "zore.toml",
        "name = 'app'\n[dependencies]\nauth = { path = '../repository' }\n",
    );
    success(fixture.invoke("check", &[]));
    failure(fixture.invoke("check", &["--locked"]), "--locked requires");
    success(fixture.invoke("install", &["--offline"]));
    let lock = fixture.lock();
    write(
        &fixture.repository,
        "auth.ore",
        "package auth\nfunc Value() int { return 17 }\n",
    );
    success(fixture.invoke("check", &["--locked", "--offline"]));
    assert_eq!(lock, fixture.lock());
    write(
        &fixture.repository,
        "zore.toml",
        "name = 'auth'\nversion = '1'\n",
    );
    failure(
        fixture.invoke("check", &["--locked"]),
        "mismatched lockfile",
    );
}

#[test]
fn populated_offline_diamond_checks_builds_runs_and_tests_with_shared_identity() {
    let fixture = Fixture::new();
    for (path, text) in [
        (
            "left/zore.toml",
            "name = 'left'\n[dependencies]\nshared = { path = '../shared' }\n",
        ),
        (
            "right/zore.toml",
            "name = 'right'\n[dependencies]\nshared = { path = '../shared' }\n",
        ),
        ("shared/zore.toml", "name = 'shared'\n"),
        ("shared/zore.lock", "ignored library lock"),
        (
            "shared/shared.ore",
            "package shared\nlet Seed = initialize()\nfunc initialize() int { println(\"init:shared\"); return 42 }\ntype Value struct { Number int }\nfunc New() Value { return Value{Number: Seed} }\n",
        ),
        (
            "left/left.ore",
            "package left\nimport \"shared\"\nfunc New() shared.Value { return shared.New() }\n",
        ),
        (
            "right/right.ore",
            "package right\nimport \"shared\"\nfunc Read(value shared.Value) int { return value.Number }\n",
        ),
    ] {
        write(&fixture.repository, path, text);
    }
    let revision = commit(&fixture.repository);
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nleft = {}\nright = {}\n",
            selection(&fixture.repository, &revision, "left"),
            selection(&fixture.repository, &revision, "right")
        ),
    );
    write(
        &fixture.app,
        "main.ore",
        "package main\nimport \"left\"\nimport \"right\"\nfunc main() { println(right.Read(left.New())) }\n",
    );
    write(
        &fixture.app,
        "main_test.ore",
        "package main\nimport \"left\"\nimport \"right\"\nfunc TestShared() { if right.Read(left.New()) != 42 { panic(\"identity\") } }\n",
    );
    fixture.install();
    let lock = String::from_utf8(fixture.lock()).unwrap();
    assert_eq!(lock.matches("shared = {").count(), 1);
    assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 1);
    fs::rename(&fixture.repository, fixture.root.join("unavailable")).unwrap();
    for action in ["check", "build", "run", "test"] {
        let output = success(fixture.invoke(action, &["--locked", "--offline"]));
        let stdout = String::from_utf8(output.stdout).unwrap();
        match action {
            "run" => assert_eq!(stdout, "init:shared\n42\n"),
            "test" => assert!(stdout.contains("Tests: 1 passed, 0 failed"), "{stdout}"),
            _ => assert!(stdout.is_empty()),
        }
    }
    let output = success(
        Command::new(
            fixture
                .root
                .join(format!("main{}", std::env::consts::EXE_SUFFIX)),
        )
        .output()
        .unwrap(),
    );
    assert_eq!(output.stdout, b"init:shared\n42\n");
}

#[test]
fn gitlinks_are_rejected_without_fetching_submodules() {
    let fixture = Fixture::new();
    git(
        &fixture.repository,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},module", fixture.revision),
        ],
    );
    git(&fixture.repository, &["commit", "-qm", "submodule fixture"]);
    let revision = git(&fixture.repository, &["rev-parse", "HEAD"]);
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nauth = {}\n",
            selection(&fixture.repository, &revision, "")
        ),
    );
    failure(
        fixture.invoke("install", &[]),
        "do not support symlinks or submodules",
    );
    assert!(!fixture.app.join("zore.lock").exists());
    assert!(fs::read_dir(&fixture.cache).unwrap().next().is_none());
}

#[test]
fn fetched_project_cycles_are_reported_and_missing_subdirectories_are_not_guessed() {
    let fixture = Fixture::new();
    write(
        &fixture.repository,
        "zore.toml",
        "name = 'auth'\n[dependencies]\nshared = { path = 'shared' }\n",
    );
    write(
        &fixture.repository,
        "shared/zore.toml",
        "name = 'shared'\n[dependencies]\nauth = { path = '..' }\n",
    );
    let revision = commit(&fixture.repository);
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nauth = {}\n",
            selection(&fixture.repository, &revision, "")
        ),
    );
    failure(
        fixture.invoke("install", &[]),
        "dependency cycle: auth -> shared -> auth",
    );
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nauth = {}\n",
            selection(&fixture.repository, &revision, "missing")
        ),
    );
    failure(
        fixture.invoke("install", &[]),
        "must name a project root containing zore.toml",
    );
}

#[test]
fn local_spelling_of_a_cached_git_project_cannot_change_its_source_identity() {
    let fixture = Fixture::new();
    fixture.install();
    let source = fs::canonicalize(fixture.entry().join("sources")).unwrap();
    let path = source.to_str().unwrap();
    write(
        &fixture.root,
        "wrapper/zore.toml",
        &format!("name = 'wrapper'\n[dependencies]\nauth = {{ path = '{path}' }}\n"),
    );
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nauth = {}\nwrapper = {{ path = '../wrapper' }}\n",
            selection(&fixture.repository, &fixture.revision, "")
        ),
    );
    failure(
        fixture.invoke("lock", &["--offline"]),
        "conflicting local/Git source selections",
    );
}

#[cfg(unix)]
#[test]
fn symlink_sources_and_cached_symlink_replacements_are_rejected() {
    let fixture = Fixture::new();
    fixture.install();
    let source = fixture.entry().join("sources/auth.ore");
    fs::remove_file(&source).unwrap();
    std::os::unix::fs::symlink(fixture.repository.join("auth.ore"), &source).unwrap();
    failure(
        fixture.invoke("check", &["--offline"]),
        "regular files and directories",
    );
    std::os::unix::fs::symlink("auth.ore", fixture.repository.join("link.ore")).unwrap();
    let revision = commit(&fixture.repository);
    write(
        &fixture.app,
        "zore.toml",
        &format!(
            "name = 'app'\n[dependencies]\nauth = {}\n",
            selection(&fixture.repository, &revision, "")
        ),
    );
    failure(
        fixture.invoke("lock", &[]),
        "do not support symlinks or submodules",
    );
}

#[cfg(unix)]
#[test]
fn credential_helper_or_server_stderr_is_never_in_diagnostics() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let script = fixture.root.join("ssh-failure");
    fs::write(&script, "#!/bin/sh\necho TOP_SECRET >&2\nexit 1\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    write(
        &fixture.app,
        "zore.toml",
        "name = 'app'\n[dependencies]\nauth = { git = 'ssh://git@example.invalid/library', rev = '0000000000000000000000000000000000000000' }\n",
    );
    let output = fixture
        .command("install", &[])
        .env("GIT_SSH_COMMAND", format!("'{}'", script.display()))
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("TOP_SECRET"));
    failure(output, "Git fetch failed");
    assert!(!fixture.app.join("zore.lock").exists());
}
