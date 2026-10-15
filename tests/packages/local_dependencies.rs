use super::*;

fn outcome(files: &[(&str, &str)]) -> Outcome {
    let mut memory = Memory::new();
    for (path, text) in files {
        memory = memory.with(*path, *text);
    }
    let mut sources = SourceMap::new();
    match load_project(&mut sources, &memory, Path::new("/app/main.ore")) {
        Ok(project) => {
            let checked = check_project(&project, &sources);
            Outcome {
                checked: checked.package.is_some(),
                messages: checked
                    .diagnostics
                    .iter()
                    .map(|d| d.message().to_string())
                    .collect(),
            }
        }
        Err(LoadError::Diagnostics(diagnostics)) => Outcome {
            checked: false,
            messages: diagnostics
                .iter()
                .map(|d| d.message().to_string())
                .collect(),
        },
        Err(LoadError::Source(error)) => Outcome {
            checked: false,
            messages: vec![error.to_string()],
        },
    }
}

fn accepts_tree(files: &[(&str, &str)]) {
    let result = outcome(files);
    assert!(
        result.checked && result.messages.is_empty(),
        "{:#?}",
        result.messages
    );
}
fn rejects_tree(files: &[(&str, &str)], expected: &str) {
    let result = outcome(files);
    assert!(
        !result.checked && result.messages.iter().any(|m| m.contains(expected)),
        "expected {expected:?}, got {:#?}",
        result.messages
    );
}

const APP: &str = "name = 'app'\n[dependencies]\nauth = { path = '../auth' }\n";
const AUTH: &str = "name = 'auth'\n";
const MAIN: &str = "package main\nimport \"auth\"\nfunc main() { println(auth.Value()) }\n";
const LIB: &str = "package auth\nfunc Value() int { return 42 }\n";

#[test]
fn root_libraries_and_their_own_subpackages_are_importable() {
    accepts_tree(&[
        ("/app/zore.toml", APP),
        ("/app/main.ore", MAIN),
        ("/auth/zore.toml", AUTH),
        (
            "/auth/auth.ore",
            "package auth\nimport \"auth/tokens\"\nfunc Value() int { return tokens.Value() }\n",
        ),
        (
            "/auth/tokens/token.ore",
            "package tokens\nfunc Value() int { return 42 }\n",
        ),
        (
            "/auth/broken_test.ore",
            "this dependency test must not be parsed",
        ),
    ]);
}

#[test]
fn a_transitive_diamond_shares_nominal_types_without_leaking_import_permissions() {
    accepts_tree(&diamond(
        "package main\nimport \"auth\"\nimport \"audit\"\nfunc main() { let id = auth.New(); println(audit.Value(id)) }\n",
    ));
}

fn diamond(main: &str) -> Vec<(&str, &str)> {
    vec![
        (
            "/app/zore.toml",
            "name = 'app'\n[dependencies]\nauth = { path = '../auth' }\naudit = { path = '../audit' }\n",
        ),
        ("/app/main.ore", main),
        (
            "/auth/zore.toml",
            "name = 'auth'\n[dependencies]\nshared = { path = '../shared' }\n",
        ),
        (
            "/auth/auth.ore",
            "package auth\nimport \"shared\"\nfunc New() shared.ID { return shared.ID{Value: 42} }\n",
        ),
        (
            "/audit/zore.toml",
            "name = 'audit'\n[dependencies]\nshared = { path = '.././shared' }\n",
        ),
        (
            "/audit/audit.ore",
            "package audit\nimport \"shared\"\nfunc Value(id shared.ID) int { return id.Value }\n",
        ),
        ("/shared/zore.toml", "name = 'shared'\n"),
        (
            "/shared/shared.ore",
            "package shared\ntype ID struct { Value int }\n",
        ),
    ]
}

#[test]
fn transitive_imports_require_a_direct_declaration_even_after_cache_population() {
    for imports in [
        "import \"auth\"\nimport \"shared\"",
        "import \"shared\"\nimport \"auth\"",
    ] {
        let main = format!(
            "package main\n{imports}\nfunc main() {{ let id = auth.New(); let copy shared.ID = id; println(copy.Value) }}\n"
        );
        rejects_tree(&diamond(&main), "directly declared dependency");
    }
    let main = "package main\nimport \"auth\"\nimport \"shared\"\nfunc main() { let id shared.ID = auth.New(); println(id.Value) }\n";
    let mut files = diamond(main);
    files[0].1 = "name = 'app'\n[dependencies]\nauth = { path = '../auth' }\naudit = { path = '../audit' }\nshared = { path = '../shared' }\n";
    accepts_tree(&files);
}

#[test]
fn conflicting_project_roots_are_rejected_even_when_unused() {
    let mut files = diamond(MAIN);
    files[4].1 = "name = 'audit'\n[dependencies]\nshared = { path = '../other' }\n";
    files.push(("/other/zore.toml", "name = 'shared'\n"));
    rejects_tree(&files, "conflicting project `shared`");
}

#[test]
fn declared_dependency_cycles_are_rejected_before_package_loading() {
    for (manifest, expected) in [
        (
            "name = 'auth'\n[dependencies]\napp = { path = '../app' }\n",
            "dependency cycle: app -> auth -> app",
        ),
        (
            "name = 'auth'\n[dependencies]\nauth = { path = './' }\n",
            "dependency cycle: auth -> auth",
        ),
    ] {
        rejects_tree(
            &[
                ("/app/zore.toml", APP),
                ("/app/main.ore", "package main\nfunc main() {}\n"),
                ("/auth/zore.toml", manifest),
            ],
            expected,
        );
    }
}

#[test]
fn dependency_keys_must_match_target_names_and_target_the_exact_root() {
    for (manifest, expected) in [
        ("name = 'other'", "does not match project name `other`"),
        ("name = 'std'", "has no valid `name`"),
    ] {
        rejects_tree(
            &[
                ("/app/zore.toml", APP),
                ("/app/main.ore", MAIN),
                ("/auth/zore.toml", manifest),
            ],
            expected,
        );
    }
    for path in ["../missing", "../auth/sub"] {
        let app = format!("name = 'app'\n[dependencies]\nauth = {{ path = '{path}' }}\n");
        rejects_tree(
            &[
                ("/app/zore.toml", &app),
                ("/app/main.ore", MAIN),
                ("/auth/zore.toml", AUTH),
            ],
            "must name a project root containing zore.toml",
        );
    }
}

#[test]
fn dependency_manifests_parse_comments_literal_strings_and_unicode_escapes() {
    accepts_tree(&[
        (
            "/app/zore.toml",
            "name = \"app\" # app\r\nversion = '0.1.0'\r\n[ dependencies ] # local\r\nauth = { path = \"../a\\u0075th#library\" } # path\r\n",
        ),
        ("/app/main.ore", MAIN),
        ("/auth#library/zore.toml", AUTH),
        ("/auth#library/auth.ore", LIB),
    ]);
    #[cfg(not(unix))]
    let paths = ["../auth/./", "../unused/../auth"];
    #[cfg(unix)]
    let paths = ["../auth/./", "../unused/../auth", "/auth"];
    for path in paths {
        let app = format!("name = 'app'\n[dependencies]\nauth = {{ path = '{path}' }}\n");
        accepts_tree(&[
            ("/app/zore.toml", &app),
            ("/app/main.ore", MAIN),
            ("/auth/zore.toml", AUTH),
            ("/auth/auth.ore", LIB),
        ]);
    }
}

#[test]
fn malformed_manifest_fields_are_never_silently_ignored() {
    for (manifest, expected) in [
        ("name = 'app'\nname = 'other'", "duplicate manifest field"),
        (
            "name = 'app'\nversion = '1'\nversion = '2'",
            "duplicate manifest field",
        ),
        ("name = 'app'\nunknown = 'x'", "unsupported manifest field"),
        ("name = 'app'\n[other]", "unsupported manifest table"),
        ("name = 'app' # invalid\u{7f}", "control character"),
        (
            "name = 'app'\n[dependencies]\nauth = { path = \"../\\u0000\" }",
            "must not contain NUL",
        ),
        (
            "name = 'app'\n[dependencies]\n[dependencies]",
            "duplicate [dependencies]",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { git = 'url' }",
            "exactly one path field",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = '' }",
            "must not be empty",
        ),
        (
            "name = 'app'\n[dependencies]\nstd = { path = '../auth' }",
            "invalid dependency name",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = '../auth', }",
            "expected `}`",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = '../auth', path = '../other' }",
            "expected `}`",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = '../auth' }\nauth = { path = '../auth' }",
            "duplicate dependency",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = '../auth'",
            "expected `{`",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = 1 }",
            "quoted manifest string",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = \"../\\q\" }",
            "unsupported manifest string escape",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = \"../\\uD800\" }",
            "invalid Unicode scalar",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = \"../auth }",
            "unterminated manifest string",
        ),
        (
            "name = 'app'\n[dependencies]\nauth = { path = '../auth' } ignored",
            "trailing manifest input",
        ),
    ] {
        let result = outcome(&[
            ("/app/zore.toml", manifest),
            ("/app/main.ore", MAIN),
            ("/auth/zore.toml", AUTH),
        ]);
        assert!(
            !result.checked
                && result
                    .messages
                    .iter()
                    .any(|message| message.contains(expected)
                        && message.contains("zore.toml")
                        && message.contains("line ")),
            "{manifest}: {:?}",
            result.messages
        );
    }
}

#[test]
fn nested_project_roots_cannot_be_imported_as_ordinary_subfolders() {
    rejects_tree(
        &[
            ("/app/zore.toml", "name = 'app'"),
            (
                "/app/main.ore",
                "package main\nimport \"app/nested\"\nfunc main() { nested.Value() }\n",
            ),
            ("/app/nested/zore.toml", "name = 'nested'"),
            ("/app/nested/main.ore", "package nested\nfunc Value() {}\n"),
        ],
        "crosses a project boundary",
    );
}

#[test]
fn imported_main_packages_and_invalid_subpackage_names_are_rejected() {
    rejects_tree(
        &[
            ("/app/zore.toml", APP),
            ("/app/main.ore", MAIN),
            ("/auth/zore.toml", AUTH),
            (
                "/auth/auth.ore",
                "package main\nfunc Value() int { return 1 }\n",
            ),
        ],
        "the `main` package cannot be imported",
    );
    for import in [
        "auth/../tokens",
        "auth/a-b",
        "auth/1tokens",
        "auth//tokens",
        "auth/tokens/",
    ] {
        let main = format!("package main\nimport \"{import}\"\nfunc main() {{}}\n");
        rejects_tree(
            &[
                ("/app/zore.toml", APP),
                ("/app/main.ore", &main),
                ("/auth/zore.toml", AUTH),
            ],
            "not a valid import path",
        );
    }
}

#[test]
fn exported_generic_and_interface_values_keep_cross_project_types_and_visibility() {
    accepts_tree(&[
        ("/app/zore.toml", APP),
        (
            "/app/main.ore",
            "package main\nimport \"auth\"\nfunc main() { let id = auth.ID{Value: 42}; println(auth.Echo(id).Value); println(auth.Read(id)) }\n",
        ),
        ("/auth/zore.toml", AUTH),
        (
            "/auth/auth.ore",
            "package auth\ntype ID struct { Value int }\nfunc (id ID) Number() int { return id.Value }\ntype Numbered interface { Number() int }\nfunc Read(id Numbered) int { return id.Number() }\nfunc Echo<T any>(value own T) T { return value }\n",
        ),
    ]);
    rejects_tree(
        &[
            ("/app/zore.toml", APP),
            (
                "/app/main.ore",
                "package main\nimport \"auth\"\nfunc main() { auth.secret() }\n",
            ),
            ("/auth/zore.toml", AUTH),
            ("/auth/auth.ore", "package auth\nfunc secret() {}\n"),
        ],
        "not exported",
    );
    rejects_tree(
        &[
            ("/app/zore.toml", APP),
            (
                "/app/main.ore",
                "package main\nimport \"auth\"\nfunc main() { let values = Array<int>{1}; auth.Take(values); println(values.len()) }\n",
            ),
            ("/auth/zore.toml", AUTH),
            (
                "/auth/auth.ore",
                "package auth\nfunc Take(values own Array<int>) {}\n",
            ),
        ],
        "moved",
    );
}

fn disk_outcome(entry: &Path) -> Outcome {
    let mut sources = SourceMap::new();
    match load_project(&mut sources, &zore::driver::project::Disk, entry) {
        Ok(project) => {
            let checked = check_project(&project, &sources);
            Outcome {
                checked: checked.package.is_some(),
                messages: checked
                    .diagnostics
                    .iter()
                    .map(|d| d.message().to_string())
                    .collect(),
            }
        }
        Err(LoadError::Diagnostics(diagnostics)) => Outcome {
            checked: false,
            messages: diagnostics
                .iter()
                .map(|d| d.message().to_string())
                .collect(),
        },
        Err(LoadError::Source(error)) => Outcome {
            checked: false,
            messages: vec![error.to_string()],
        },
    }
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[cfg(unix)]
#[test]
fn canonical_project_aliases_deduplicate_and_package_symlinks_cannot_escape_roots() {
    use std::os::unix::fs::symlink;
    let directory = zore::build::TempDir::new().unwrap();
    let root = directory.path();
    for (path, text) in diamond(
        "package main\nimport \"auth\"\nimport \"audit\"\nfunc main() { println(audit.Value(auth.New())) }\n",
    ) {
        write(root, path.trim_start_matches('/'), text);
    }
    symlink(root.join("shared"), root.join("alias")).unwrap();
    write(
        root,
        "audit/zore.toml",
        "name = 'audit'\n[dependencies]\nshared = { path = '../alias' }\n",
    );
    let result = disk_outcome(&root.join("app/main.ore"));
    assert!(
        result.checked && result.messages.is_empty(),
        "{:?}",
        result.messages
    );
    symlink(root.join("shared"), root.join("app/escape")).unwrap();
    write(
        root,
        "app/main.ore",
        "package main\nimport \"app/escape\"\nfunc main() {}\n",
    );
    let result = disk_outcome(&root.join("app/main.ore"));
    assert!(
        result
            .messages
            .iter()
            .any(|m| m.contains("escapes project root")),
        "{:?}",
        result.messages
    );
    symlink(root.join("app"), root.join("app/self")).unwrap();
    write(
        root,
        "app/main.ore",
        "package main\nimport \"app/self\"\nfunc main() {}\n",
    );
    let result = disk_outcome(&root.join("app/main.ore"));
    assert!(
        result
            .messages
            .iter()
            .any(|m| m.contains("the `main` package cannot be imported")),
        "{:?}",
        result.messages
    );
}

#[cfg(unix)]
#[test]
fn package_folder_aliases_do_not_acquire_distinct_nominal_identities() {
    use std::os::unix::fs::symlink;
    let directory = zore::build::TempDir::new().unwrap();
    let root = directory.path();
    write(root, "app/zore.toml", APP);
    write(
        root,
        "app/main.ore",
        "package main\nimport \"app/a\"\nimport \"app/b\"\nfunc main() { a.Call(); b.Call() }\n",
    );
    write(
        root,
        "app/a/a.ore",
        "package a\nimport \"auth/first/value\"\nfunc Call() { value.Call() }\n",
    );
    write(
        root,
        "app/b/b.ore",
        "package b\nimport \"auth/second/value\"\nfunc Call() { value.Call() }\n",
    );
    write(root, "auth/zore.toml", AUTH);
    write(
        root,
        "auth/first/value/value.ore",
        "package value\nfunc Call() {}\n",
    );
    std::fs::create_dir_all(root.join("auth/second")).unwrap();
    symlink(
        root.join("auth/first/value"),
        root.join("auth/second/value"),
    )
    .unwrap();
    let result = disk_outcome(&root.join("app/main.ore"));
    assert!(
        result
            .messages
            .iter()
            .any(|m| m.contains("canonical package folder") && m.contains("already imported")),
        "{:?}",
        result.messages
    );
}

#[test]
fn disk_dependencies_resolve_from_manifests_with_unicode_paths_and_absolute_entries() {
    let directory = zore::build::TempDir::new().unwrap();
    let root = directory.path().join("projects café with spaces");
    write(&root, "auth/zore.toml", AUTH);
    write(&root, "auth/auth.ore", LIB);
    let library = root.join("auth");
    let manifest = format!(
        "name = 'app'\n[dependencies]\nauth = {{ path = '{}' }}\n",
        library.display()
    );
    write(&root, "app/zore.toml", &manifest);
    write(&root, "app/main.ore", MAIN);
    let result = disk_outcome(&root.join("app/main.ore"));
    assert!(
        result.checked && result.messages.is_empty(),
        "{:?}",
        result.messages
    );
    write(
        &root,
        "app/main.ore",
        "package main\nimport \"auth/missing\"\nfunc main() {}\n",
    );
    let result = disk_outcome(&root.join("app/main.ore"));
    assert!(result.messages.iter().any(|message| message.contains("was not found") && message.contains("no `.ore` files")), "{:?}", result.messages);
}

#[cfg(windows)]
#[test]
fn windows_drive_relative_paths_are_rejected_and_case_aliases_share_identity() {
    for path in [r"C:auth", r"\auth"] {
        let app = format!("name = 'app'\n[dependencies]\nauth = {{ path = '{path}' }}\n");
        rejects_tree(
            &[("/app/zore.toml", &app), ("/app/main.ore", MAIN)],
            "Windows dependency paths must be relative or fully absolute",
        );
    }
    let directory = zore::build::TempDir::new().unwrap();
    let root = directory.path();
    for (path, text) in diamond(
        "package main\nimport \"auth\"\nimport \"audit\"\nfunc main() { println(audit.Value(auth.New())) }\n",
    ) {
        write(root, path.trim_start_matches('/'), text);
    }
    let alias = root.join("shared").display().to_string().to_uppercase();
    write(
        root,
        "audit/zore.toml",
        &format!("name = 'audit'\n[dependencies]\nshared = {{ path = '{alias}' }}\n"),
    );
    let result = disk_outcome(&root.join("app/main.ore"));
    assert!(
        result.checked && result.messages.is_empty(),
        "{:?}",
        result.messages
    );
}

#[test]
fn canonical_memory_lookup_preserves_source_paths_in_diagnostics() {
    let memory = Memory::new()
        .with("/app/zore.toml", APP)
        .with("/app/main.ore", MAIN)
        .with("/auth/zore.toml", AUTH)
        .with(
            "/auth/source.ore",
            "package auth\nfunc Value() int { return missing }\n",
        );
    let mut sources = SourceMap::new();
    let project = load_project(&mut sources, &memory, Path::new("/app/main.ore"))
        .unwrap_or_else(|_| panic!("valid project"));
    let checked = check_project(&project, &sources);
    let rendered = checked.diagnostics[0].render(&sources).unwrap();
    assert!(rendered.contains("/auth/source.ore:2:"), "{rendered}");
}
