use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use zore::build::TempDir;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
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
    assert!(output.status.success(), "{args:?}: {output:?}");
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

fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    output
}

fn failure(output: Output, message: &str) -> String {
    assert!(!output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(error.contains(message), "{output:?}");
    error
}

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
    app: PathBuf,
    cache: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("Tagged projects café with spaces");
        let app = root.join("app");
        let cache = root.join("cache");
        write(&app, "main.ore", "package main\nfunc main() {}\n");
        Self {
            _temporary: temporary,
            root,
            app,
            cache,
        }
    }

    fn repository(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn requirement(&self, name: &str, version: &str) -> String {
        format!(
            "{{ git = '{}', version = '{version}' }}",
            url(&self.repository(name))
        )
    }

    fn pin(&self, name: &str, revision: &str) -> String {
        format!(
            "{{ git = '{}', rev = '{revision}' }}",
            url(&self.repository(name))
        )
    }

    fn release(
        &self,
        name: &str,
        version: &str,
        dependencies: &str,
        source: &str,
        annotated: bool,
    ) -> String {
        let root = self.repository(name);
        if !root.exists() {
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q", "--initial-branch=main"]);
        }
        write(
            &root,
            "zore.toml",
            &format!(
                "name = '{name}'\nversion = '{version}'\nlicense = 'MIT'\n[dependencies]\n{dependencies}"
            ),
        );
        write(&root, &format!("{name}.ore"), source);
        let revision = commit(&root);
        let tag = format!("{name}/v{version}");
        if annotated {
            git(&root, &["tag", "-a", &tag, "-m", "fixture"]);
        } else {
            git(&root, &["tag", &tag]);
        }
        revision
    }

    fn leaf(&self, version: &str) -> String {
        self.release(
            "shared",
            version,
            "",
            "package shared\ntype User struct { Id int }\nfunc New() User { return User{Id: 4} }\n",
            false,
        )
    }

    fn app(&self, dependencies: &str) {
        write(
            &self.app,
            "zore.toml",
            &format!("name = 'app'\n[dependencies]\n{dependencies}"),
        );
    }

    fn wrapper(&self, name: &str, dependency: &str) -> String {
        write(
            &self.root,
            &format!("{name}/zore.toml"),
            &format!("name = '{name}'\n[dependencies]\nshared = {dependency}\n"),
        );
        format!("{name} = {{ path = '../{name}' }}\n")
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
    fn lock(&self) -> String {
        fs::read_to_string(self.app.join("zore.lock")).unwrap()
    }
    fn selected(&self, name: &str) -> String {
        self.lock()
            .lines()
            .find(|line| line.starts_with(&format!("{name} = ")))
            .unwrap()
            .into()
    }
    fn snapshot(&self, name: &str, revision: &str) -> PathBuf {
        self.cache.join(format!(
            "{:x}",
            Sha256::digest(format!("{}\0{revision}", url(&self.repository(name))).as_bytes())
        ))
    }
}

fn diamond() -> (Fixture, String) {
    let fixture = Fixture::new();
    fixture.release("extra", "1.0.0", "", "package extra\n", false);
    fixture.release("second", "1.0.0", "", "package second\n", false);
    let lower = fixture.release(
        "shared",
        "1.2.3",
        &format!("extra = {}\n", fixture.requirement("extra", "1.0.0")),
        "unselected source is deliberately invalid",
        false,
    );
    fixture.release(
        "shared",
        "1.2.4",
        &format!("second = {}\n", fixture.requirement("second", "1.0.0")),
        "package shared\ntype User struct { Id int }\nfunc New() User { return User{Id: 4} }\n",
        true,
    );
    fixture.release(
        "shared",
        "1.9.0",
        "",
        "unrequested newer source is invalid",
        false,
    );
    fixture.release(
        "greeting",
        "1.0.0",
        &format!("shared = {}\n", fixture.requirement("shared", "1.2.3")),
        "package greeting\nimport \"shared\"\nfunc New() shared.User { return shared.New() }\n",
        false,
    );
    fixture.release(
        "audit",
        "1.0.0",
        &format!("shared = {}\n", fixture.requirement("shared", "1.2.4")),
        "package audit\nimport \"shared\"\nfunc Value(user shared.User) int { return user.Id }\n",
        true,
    );
    fixture.app(&format!(
        "greeting = {}\naudit = {}\n",
        fixture.requirement("greeting", "1.0.0"),
        fixture.requirement("audit", "1.0.0")
    ));
    write(
        &fixture.app,
        "main.ore",
        "package main\nimport \"greeting\"\nimport \"audit\"\nfunc main() { println(audit.Value(greeting.New())) }\n",
    );
    (fixture, lower)
}

#[test]
fn patch_diamond_retains_superseded_and_higher_requirements_but_checks_selected_code_only() {
    let (fixture, _) = diamond();
    success(fixture.invoke("install", &[]));
    let lock = fixture.lock();
    assert!(lock.starts_with("version = 2\n"));
    assert!(fixture.selected("shared").contains("version = \"1.2.4\""));
    assert!(lock.contains("extra = {") && lock.contains("second = {"));
    assert!(lock.contains("version = \"1.2.3\"") && !lock.contains("version = \"1.9.0\""));
    success(
        fixture
            .command("check", &["--locked", "--offline"])
            .env("ZORE_CC", "missing-cc")
            .env("ZORE_RUSTC", "missing-rustc")
            .output()
            .unwrap(),
    );
    assert_eq!(fixture.lock(), lock);
    write(
        &fixture.app,
        "main.ore",
        "package main\nimport \"extra\"\nfunc main() {}\n",
    );
    failure(
        fixture.invoke("check", &["--offline"]),
        "directly declared dependency",
    );
}

#[test]
fn tagged_libraries_share_nominal_types_and_run_build_and_test_offline() {
    let (fixture, _) = diamond();
    success(fixture.invoke("install", &[]));
    for name in ["shared", "extra", "second", "greeting", "audit"] {
        fs::rename(
            fixture.repository(name),
            fixture.repository(&format!("hidden-{name}")),
        )
        .unwrap();
    }
    assert_eq!(
        success(fixture.invoke("run", &["--locked", "--offline"])).stdout,
        b"4\n"
    );
    success(fixture.invoke("build", &["--locked", "--offline"]));
    write(
        &fixture.app,
        "app_test.ore",
        "package main\nimport \"greeting\"\nimport \"audit\"\nfunc TestSharedIdentity() { if audit.Value(greeting.New()) != 4 { panic(\"identity\") } }\n",
    );
    assert!(
        String::from_utf8_lossy(
            &success(fixture.invoke("test", &["--locked", "--offline"])).stdout
        )
        .contains("1 passed")
    );
}

#[test]
fn explicit_lock_supports_root_raised_minor_minima_and_removal_downgrades() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    fixture.leaf("1.4.0");
    fixture.app(&fixture.wrapper("wrapper", &fixture.requirement("shared", "1.2.3")));
    success(fixture.invoke("install", &[]));
    let old = fixture.lock();
    fixture.app(&format!(
        "{}shared = {}\n",
        fixture.wrapper("wrapper", &fixture.requirement("shared", "1.2.3")),
        fixture.requirement("shared", "1.4.0")
    ));
    failure(fixture.invoke("install", &[]), "zore lock");
    assert_eq!(fixture.lock(), old);
    success(fixture.invoke("lock", &[]));
    assert!(fixture.selected("shared").contains("version = \"1.4.0\""));
    fixture.app(&fixture.wrapper("wrapper", &fixture.requirement("shared", "1.2.3")));
    fs::rename(
        fixture.repository("shared"),
        fixture.repository("hidden-shared"),
    )
    .unwrap();
    success(fixture.invoke("lock", &["--offline"]));
    assert!(fixture.selected("shared").contains("version = \"1.2.3\""));
    assert!(fixture.lock().contains("version = \"1.4.0\""));
    success(fixture.invoke("check", &["--locked", "--offline"]));
}

#[test]
fn compatibility_lines_and_missing_minima_do_not_fall_back_to_other_releases() {
    for (left, right, compatible) in [
        ("1.2.3", "1.4.0", true),
        ("0.3.1", "0.3.4", true),
        ("0.3.1", "0.4.0", false),
        ("1.2.3", "2.0.0", false),
    ] {
        let fixture = Fixture::new();
        fixture.leaf(left);
        fixture.leaf(right);
        fixture.app(&format!(
            "{}{}",
            fixture.wrapper("left", &fixture.requirement("shared", left)),
            fixture.wrapper("right", &fixture.requirement("shared", right))
        ));
        if compatible {
            success(fixture.invoke("install", &[]));
            assert!(
                fixture
                    .selected("shared")
                    .contains(&format!("version = \"{right}\""))
            );
        } else {
            let message = failure(fixture.invoke("install", &[]), "incompatible version lines");
            assert!(message.contains("left -> shared") && message.contains("right -> shared"));
            assert!(!fixture.app.join("zore.lock").exists());
        }
    }
    let fixture = Fixture::new();
    fixture.leaf("1.9.0");
    fixture.app(&format!(
        "shared = {}\n",
        fixture.requirement("shared", "1.2.3")
    ));
    failure(fixture.invoke("install", &[]), "shared/v1.2.3");
    assert!(!fixture.app.join("zore.lock").exists());
}

#[test]
fn hard_pins_are_compared_to_the_final_selected_minimum() {
    let fixture = Fixture::new();
    let lower = fixture.leaf("1.2.3");
    let higher = fixture.leaf("1.2.4");
    fixture.app(&format!(
        "{}{}shared = {}\n",
        fixture.wrapper("lower", &fixture.requirement("shared", "1.2.3")),
        fixture.wrapper("higher", &fixture.requirement("shared", "1.2.4")),
        fixture.pin("shared", &higher)
    ));
    success(fixture.invoke("install", &[]));
    let lock = fixture.lock();
    fixture.app(&format!(
        "{}shared = {}\n",
        fixture.wrapper("higher", &fixture.requirement("shared", "1.2.4")),
        fixture.pin("shared", &lower)
    ));
    let message = failure(fixture.invoke("lock", &[]), "hard pin disagrees");
    assert!(message.contains("higher -> shared@1.2.4") && message.contains(&lower));
    assert_eq!(fixture.lock(), lock);
}

#[test]
fn declaration_order_does_not_change_selected_sources_or_release_provenance() {
    let (fixture, _) = diamond();
    success(fixture.invoke("install", &[]));
    let lock = fixture.lock();
    fixture.app(&format!(
        "audit = {}\ngreeting = {}\n",
        fixture.requirement("audit", "1.0.0"),
        fixture.requirement("greeting", "1.0.0")
    ));
    success(fixture.invoke("lock", &[]));
    let without_entry = |text: &str| {
        text.lines()
            .filter(|line| !line.starts_with("app = "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(without_entry(&lock), without_entry(&fixture.lock()));
}

#[test]
fn matching_locks_ignore_moved_and_deleted_tags_but_online_lock_rejects_retagging() {
    let fixture = Fixture::new();
    let lower = fixture.leaf("1.2.3");
    fixture.app(&format!(
        "shared = {}\n",
        fixture.requirement("shared", "1.2.3")
    ));
    success(fixture.invoke("install", &[]));
    let lock = fixture.lock();
    fixture.leaf("1.2.4");
    git(
        &fixture.repository("shared"),
        &["tag", "-f", "shared/v1.2.3", "HEAD"],
    );
    success(fixture.invoke("install", &["--locked"]));
    failure(fixture.invoke("lock", &[]), "release identity conflict");
    assert_eq!(fixture.lock(), lock);
    git(
        &fixture.repository("shared"),
        &["tag", "-d", "shared/v1.2.3"],
    );
    let fresh_cache = fixture.root.join("fresh-cache");
    success(
        fixture
            .command("install", &["--locked"])
            .env("ZORE_CACHE_DIR", fresh_cache)
            .output()
            .unwrap(),
    );
    assert!(fixture.selected("shared").contains(&lower));
    failure(fixture.invoke("lock", &[]), "unavailable");
    success(fixture.invoke("lock", &["--offline"]));
    assert_eq!(fixture.lock(), lock);
}

#[test]
fn offline_graphs_require_unselected_release_snapshots_and_can_recreate_locks() {
    let (fixture, lower) = diamond();
    success(fixture.invoke("install", &[]));
    let lock = fixture.lock();
    for name in ["shared", "extra", "second", "greeting", "audit"] {
        fs::rename(
            fixture.repository(name),
            fixture.repository(&format!("hidden-{name}")),
        )
        .unwrap();
    }
    let snapshot = fixture.snapshot("shared", &lower);
    let hidden = fixture.cache.join("hidden-snapshot");
    fs::rename(&snapshot, &hidden).unwrap();
    failure(
        fixture.invoke("check", &["--locked", "--offline"]),
        "missing from the cache",
    );
    assert_eq!(fixture.lock(), lock);
    fs::rename(hidden, snapshot).unwrap();
    success(fixture.invoke("lock", &["--offline"]));
    fs::remove_file(fixture.app.join("zore.lock")).unwrap();
    success(fixture.invoke("install", &["--offline"]));
    assert_eq!(fixture.lock(), lock);
}

#[test]
fn shared_binding_cache_prevents_retagging_across_unlocked_projects() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    fixture.app(&format!(
        "shared = {}\n",
        fixture.requirement("shared", "1.2.3")
    ));
    success(fixture.invoke("install", &[]));
    fixture.leaf("1.2.4");
    git(
        &fixture.repository("shared"),
        &["tag", "-f", "shared/v1.2.3", "HEAD"],
    );
    fs::remove_file(fixture.app.join("zore.lock")).unwrap();
    failure(fixture.invoke("install", &[]), "release identity conflict");
    assert!(!fixture.app.join("zore.lock").exists());
}

#[test]
fn malformed_versions_and_mixed_selectors_are_manifest_errors() {
    let fixture = Fixture::new();
    for fields in [
        "git = 'https://example.invalid/repo', version = '^1.2.3'",
        "git = 'https://example.invalid/repo', version = '01.2.3'",
        "git = 'https://example.invalid/repo', version = '1.2.3', rev = '0000000000000000000000000000000000000000'",
        "path = '../shared', git = 'https://example.invalid/repo', version = '1.2.3'",
    ] {
        fixture.app(&format!("shared = {{ {fields} }}\n"));
        failure(fixture.invoke("install", &["--offline"]), "zore.toml");
        assert!(!fixture.app.join("zore.lock").exists());
    }
}

#[test]
fn tag_manifest_identity_and_noncommit_tags_are_rejected_without_lock_writes() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    git(&fixture.repository("shared"), &["tag", "shared/v1.2.4"]);
    fixture.app(&format!(
        "shared = {}\n",
        fixture.requirement("shared", "1.2.4")
    ));
    failure(fixture.invoke("install", &[]), "does not match manifest");
    let blob = git(
        &fixture.repository("shared"),
        &["rev-parse", "HEAD:zore.toml"],
    );
    git(
        &fixture.repository("shared"),
        &["tag", "-f", "shared/v1.2.4", &blob],
    );
    failure(fixture.invoke("install", &[]), "must resolve to a commit");
    fixture.app(&format!(
        "wrong = {}\n",
        fixture.requirement("shared", "1.2.3")
    ));
    git(
        &fixture.repository("shared"),
        &["tag", "wrong/v1.2.3", "HEAD"],
    );
    failure(
        fixture.invoke("install", &[]),
        "does not match project name",
    );
    assert!(!fixture.app.join("zore.lock").exists());
}

#[test]
fn source_url_identity_and_selected_graph_cycles_show_dependency_paths() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    let duplicate = fixture.repository("duplicate");
    git(
        &fixture.root,
        &[
            "clone",
            "--bare",
            fixture.repository("shared").to_str().unwrap(),
            duplicate.to_str().unwrap(),
        ],
    );
    fixture.app(&format!(
        "{}{}",
        fixture.wrapper("left", &fixture.requirement("shared", "1.2.3")),
        fixture.wrapper(
            "right",
            &format!("{{ git = '{}', version = '1.2.3' }}", url(&duplicate))
        )
    ));
    let message = failure(
        fixture.invoke("install", &[]),
        "conflicting source identities",
    );
    assert!(message.contains("left -> shared") && message.contains("right -> shared"));
    fixture.app("left = { path = '../left' }\n");
    write(
        &fixture.root,
        "left/zore.toml",
        "name = 'left'\n[dependencies]\nright = { path = '../right' }\n",
    );
    write(
        &fixture.root,
        "right/zore.toml",
        "name = 'right'\n[dependencies]\nleft = { path = '../left' }\n",
    );
    failure(fixture.invoke("install", &[]), "dependency cycle");
}

#[test]
fn monorepo_subdirectories_and_snapshot_path_dependencies_remain_confined() {
    let fixture = Fixture::new();
    let repo = fixture.repository("repository");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "--initial-branch=main"]);
    write(
        &repo,
        "libraries/shared/zore.toml",
        "name = 'shared'\nversion = '1.2.3'\nlicense = 'MIT'\n[dependencies]\nhelper = { path = '../helper' }\n",
    );
    write(&repo, "libraries/shared/shared.ore", "package shared\n");
    write(&repo, "libraries/helper/zore.toml", "name = 'helper'\n");
    let revision = commit(&repo);
    git(&repo, &["tag", "-a", "shared/v1.2.3", "-m", "fixture"]);
    fixture.app(&format!(
        "shared = {{ git = '{}', version = '1.2.3', subdir = 'libraries/shared' }}\n",
        url(&repo)
    ));
    success(fixture.invoke("install", &[]));
    assert!(
        fixture
            .selected("shared")
            .contains("subdir = \"libraries/shared\"")
    );
    assert!(fixture.selected("helper").contains(&revision));
    let lock = fixture.lock();
    fixture.app(&format!(
        "shared = {{ git = '{}', version = '1.2.3', subdir = 'libraries' }}\n",
        url(&repo)
    ));
    failure(fixture.invoke("lock", &[]), "must name a project root");
    assert_eq!(fixture.lock(), lock);
    write(
        &repo,
        "libraries/shared/zore.toml",
        "name = 'shared'\nversion = '1.2.4'\nlicense = 'MIT'\n[dependencies]\nhelper = { path = '../../../outside' }\n",
    );
    commit(&repo);
    git(&repo, &["tag", "shared/v1.2.4"]);
    fixture.app(&format!(
        "shared = {{ git = '{}', version = '1.2.4', subdir = 'libraries/shared' }}\n",
        url(&repo)
    ));
    failure(
        fixture.invoke("lock", &[]),
        "must stay inside its Git checkout",
    );
    assert_eq!(fixture.lock(), lock);
}

#[test]
fn malformed_release_metadata_and_compatibility_claims_are_not_decoration() {
    for extra in [
        "license = 'made-up'",
        "license-file = '../COPYING'",
        "zore-min = '999.0.0'",
        "readme = 'missing'",
        "platforms = []",
    ] {
        let fixture = Fixture::new();
        fixture.leaf("1.2.3");
        write(
            &fixture.repository("shared"),
            "zore.toml",
            &format!("name = 'shared'\nversion = '1.2.3'\n{extra}\n"),
        );
        commit(&fixture.repository("shared"));
        git(
            &fixture.repository("shared"),
            &["tag", "-f", "shared/v1.2.3", "HEAD"],
        );
        fixture.app(&format!(
            "shared = {}\n",
            fixture.requirement("shared", "1.2.3")
        ));
        failure(fixture.invoke("install", &[]), "zore.toml");
        assert!(!fixture.app.join("zore.lock").exists());
    }
}

#[test]
fn lock_and_cache_corruption_are_rejected_without_repair_or_writes() {
    let (fixture, lower) = diamond();
    success(fixture.invoke("install", &[]));
    let lock = fixture.lock();
    for changed in [
        lock.replace("version = 2\n", "version = 3\n"),
        lock.replacen("[releases]", "[other]", 1),
        lock.replacen("0 = {", "9 = {", 1),
        lock.replacen("name = \"audit\"", "name = \"wrong\"", 1),
        lock.replacen("tag = \"audit/v1.0.0\"", "tag = \"audit/v1.0.1\"", 1),
    ] {
        write(&fixture.app, "zore.lock", &changed);
        assert!(
            !fixture
                .invoke("check", &["--locked", "--offline"])
                .status
                .success()
        );
        assert_eq!(fixture.lock(), changed);
    }
    write(&fixture.app, "zore.lock", &lock);
    write(
        &fixture.snapshot("shared", &lower),
        "sources/shared.ore",
        "tampered unselected code",
    );
    failure(fixture.invoke("check", &["--offline"]), "corrupt Git cache");
    assert_eq!(fixture.lock(), lock);
    write(
        &fixture.snapshot("shared", &lower),
        "sources/shared.ore",
        "unselected source is deliberately invalid",
    );
    let bindings = fixture.cache.join("releases");
    let directory = fs::read_dir(&bindings)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let binding = fs::read_to_string(directory.join("binding")).unwrap();
    fs::write(directory.join("binding"), "corrupt").unwrap();
    failure(
        fixture.invoke("check", &["--offline"]),
        "corrupt release binding",
    );
    assert_eq!(fixture.lock(), lock);
    fs::write(directory.join("binding"), binding).unwrap();
    fs::rename(&bindings, fixture.cache.join("hidden-bindings")).unwrap();
    success(fixture.invoke("check", &["--locked", "--offline"]));
    assert!(!bindings.exists());
    success(fixture.invoke("install", &["--locked", "--offline"]));
    assert!(bindings.exists());
}

#[test]
fn native_release_selection_reuses_recipe_locks_and_frontend_checks_need_no_toolchain() {
    let fixture = Fixture::new();
    fixture.release("adler32", "1.0.0", "", "package adler32\n", false);
    let repository = fixture.repository("adler32");
    write(
        &repository,
        "zore.toml",
        "name = 'adler32'\nversion = '1.0.1'\nlicense = 'MIT'\n[native]\nadapter = 'adler32-v1'\n",
    );
    for (path, text) in [
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
        write(&repository, &format!("native/{path}"), text);
    }
    commit(&repository);
    git(&repository, &["tag", "adler32/v1.0.1"]);
    fixture.app(&format!(
        "adler32 = {}\n",
        fixture.requirement("adler32", "1.0.1")
    ));
    success(fixture.invoke("install", &[]));
    assert_eq!(fixture.lock().matches("recipe = ").count(), 2);
    success(
        fixture
            .command("check", &["--locked", "--offline"])
            .env("ZORE_CC", "missing-cc")
            .env("ZORE_RUSTC", "missing-rustc")
            .output()
            .unwrap(),
    );
    let lock = fixture.lock();
    write(&repository, "native/adler32.c", "unapproved source");
    write(
        &repository,
        "zore.toml",
        "name = 'adler32'\nversion = '1.0.2'\nlicense = 'MIT'\n[native]\nadapter = 'adler32-v1'\n",
    );
    commit(&repository);
    git(&repository, &["tag", "adler32/v1.0.2"]);
    fixture.app(&format!(
        "adler32 = {}\n",
        fixture.requirement("adler32", "1.0.2")
    ));
    failure(fixture.invoke("lock", &[]), "do not match reviewed adapter");
    assert_eq!(fixture.lock(), lock);
}

#[test]
fn request_graph_cycles_are_finite_and_only_selected_graph_cycles_are_rejected() {
    let fixture = Fixture::new();
    fixture.release(
        "foo",
        "1.0.0",
        &format!("bar = {}\n", fixture.requirement("bar", "1.0.0")),
        "package foo\n",
        false,
    );
    fixture.release("foo", "1.1.0", "", "package foo\n", false);
    fixture.release(
        "bar",
        "1.0.0",
        &format!("foo = {}\n", fixture.requirement("foo", "1.0.0")),
        "package bar\n",
        false,
    );
    fixture.app(&format!(
        "bar = {}\nfoo = {}\n",
        fixture.requirement("bar", "1.0.0"),
        fixture.requirement("foo", "1.1.0")
    ));
    success(fixture.invoke("install", &[]));
    assert!(fixture.selected("foo").contains("version = \"1.1.0\""));
    let lock = fixture.lock();
    fixture.app(&format!("bar = {}\n", fixture.requirement("bar", "1.0.0")));
    failure(fixture.invoke("lock", &["--offline"]), "dependency cycle");
    assert_eq!(fixture.lock(), lock);
}

#[test]
fn concurrent_projects_share_immutable_release_bindings_and_source_snapshots() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    fixture.app(&format!(
        "shared = {}\n",
        fixture.requirement("shared", "1.2.3")
    ));
    let second = fixture.root.join("second-app");
    write(
        &second,
        "zore.toml",
        &fs::read_to_string(fixture.app.join("zore.toml")).unwrap(),
    );
    let first = fixture
        .command("install", &[])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let other = Command::new(env!("CARGO_BIN_EXE_zore"))
        .arg("install")
        .arg(&second)
        .env("ZORE_CACHE_DIR", &fixture.cache)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    success(first.wait_with_output().unwrap());
    success(other.wait_with_output().unwrap());
    assert_eq!(
        fixture.lock(),
        fs::read_to_string(second.join("zore.lock")).unwrap()
    );
    assert_eq!(
        fs::read_dir(fixture.cache.join("releases"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn tagged_subdirectories_and_manifest_names_use_exact_git_spelling_on_all_hosts() {
    let fixture = Fixture::new();
    let repository = fixture.repository("repository");
    fs::create_dir_all(&repository).unwrap();
    git(&repository, &["init", "-q", "--initial-branch=main"]);
    write(
        &repository,
        "library/zore.toml",
        "name = 'shared'\nversion = '1.2.3'\nlicense = 'MIT'\n",
    );
    commit(&repository);
    git(&repository, &["tag", "shared/v1.2.3"]);
    fixture.app(&format!(
        "shared = {{ git = '{}', version = '1.2.3', subdir = 'LIBRARY' }}\n",
        url(&repository)
    ));
    failure(fixture.invoke("install", &[]), "exact Git path spelling");
    fixture.app(&format!(
        "shared = {{ git = '{}', version = '1.2.3', subdir = 'library' }}\n",
        url(&repository)
    ));
    success(fixture.invoke("install", &[]));
}

#[test]
fn removing_tagged_requirements_retains_history_without_selecting_historical_projects() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    fixture.app(&format!(
        "shared = {}\n",
        fixture.requirement("shared", "1.2.3")
    ));
    success(fixture.invoke("install", &[]));
    fixture.app("");
    fs::rename(
        fixture.repository("shared"),
        fixture.repository("hidden-shared"),
    )
    .unwrap();
    success(fixture.invoke("lock", &["--offline"]));
    let lock = fixture.lock();
    assert!(lock.starts_with("version = 2\n"));
    assert!(!lock.contains("shared = {"));
    assert!(lock.contains("name = \"shared\""));
    success(fixture.invoke("check", &["--locked", "--offline"]));
    success(fixture.invoke("install", &["--locked", "--offline"]));
    assert_eq!(fixture.lock(), lock);
}

#[test]
fn tagged_graphs_require_locks_for_checking_and_known_bindings_for_offline_install() {
    let fixture = Fixture::new();
    fixture.leaf("1.2.3");
    fixture.app(&format!(
        "shared = {}\n",
        fixture.requirement("shared", "1.2.3")
    ));
    failure(fixture.invoke("install", &["--offline"]), "release binding");
    failure(fixture.invoke("check", &[]), "matching lockfile");
    failure(
        fixture.invoke("install", &["--locked"]),
        "--locked requires",
    );
    assert!(!fixture.app.join("zore.lock").exists());
    success(fixture.invoke("install", &[]));
    fs::remove_file(fixture.app.join("zore.lock")).unwrap();
    failure(fixture.invoke("check", &["--offline"]), "matching lockfile");
    assert!(!fixture.app.join("zore.lock").exists());
    success(fixture.invoke("install", &["--offline"]));
}
