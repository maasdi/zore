use super::local_dependencies;

const NATIVE_SOURCES: &[(&str, &str)] = &[
    (
        "/adler32/native/bindings.abi",
        include_str!("../../examples/native-library/adler32/native/bindings.abi"),
    ),
    (
        "/adler32/native/adler32.c",
        include_str!("../../examples/native-library/adler32/native/adler32.c"),
    ),
    (
        "/adler32/native/adler32.h",
        include_str!("../../examples/native-library/adler32/native/adler32.h"),
    ),
    (
        "/adler32/native/bridge.rs",
        include_str!("../../examples/native-library/adler32/native/bridge.rs"),
    ),
];

fn accepts_tree(files: &[(&str, &str)]) {
    let mut files = files.to_vec();
    files.extend_from_slice(NATIVE_SOURCES);
    local_dependencies::accepts_tree(&files);
}

fn rejects_tree(files: &[(&str, &str)], expected: &str) {
    let mut files = files.to_vec();
    files.extend_from_slice(NATIVE_SOURCES);
    local_dependencies::rejects_tree(&files, expected);
}

const APP: &str = "name = 'app'\n[dependencies]\nadler32 = { path = '../adler32' }\n";
const LIBRARY: &str = include_str!("../../examples/native-library/adler32/adler32.ore");
const MANIFEST: &str = include_str!("../../examples/native-library/adler32/zore.toml");
const MAIN: &str = "package main\nimport \"adler32\"\nfunc main() { let h, err = adler32.New(); if err != nil { panic(\"create\") }; let value, failure = h.Sum(); if failure != nil { panic(\"sum\") }; println(value) }\n";

#[test]
fn curated_wrapper_checks_without_a_native_toolchain() {
    accepts_tree(&[
        ("/app/zore.toml", APP),
        ("/app/main.ore", MAIN),
        ("/adler32/zore.toml", MANIFEST),
        ("/adler32/adler32.ore", LIBRARY),
    ]);
}

#[test]
fn generated_bindings_and_resource_fields_stay_private() {
    for (main, message) in [
        (
            "package main\nimport \"adler32\"\nfunc main() { let handle, err = adler32.nativeCreate(); if err != nil { panic(\"create\") }; println(handle) }",
            "not exported",
        ),
        (
            "package main\nimport \"adler32\"\nfunc main() { let h, err = adler32.New(); if err != nil { panic(\"create\") }; println(h.handle) }",
            "not exported",
        ),
    ] {
        rejects_tree(
            &[
                ("/app/zore.toml", APP),
                ("/app/main.ore", main),
                ("/adler32/zore.toml", MANIFEST),
                ("/adler32/adler32.ore", LIBRARY),
            ],
            message,
        );
    }
}

#[test]
fn registration_is_explicit_root_only_and_uses_normal_duplicate_checks() {
    rejects_tree(
        &[
            ("/app/zore.toml", APP),
            ("/app/main.ore", MAIN),
            ("/adler32/zore.toml", "name = 'adler32'\n"),
            ("/adler32/adler32.ore", LIBRARY),
        ],
        "nativeCreate",
    );
    let duplicate = format!("{LIBRARY}\nfunc nativeCreate() int {{ return 0 }}\n");
    rejects_tree(
        &[
            ("/app/zore.toml", APP),
            ("/app/main.ore", MAIN),
            ("/adler32/zore.toml", MANIFEST),
            ("/adler32/adler32.ore", &duplicate),
        ],
        "duplicate declaration",
    );
    rejects_tree(
        &[
            ("/app/zore.toml", APP),
            (
                "/app/main.ore",
                "package main\nimport \"adler32/internal\"\nfunc main() { internal.Call() }",
            ),
            ("/adler32/zore.toml", MANIFEST),
            (
                "/adler32/internal/internal.ore",
                "package internal\nfunc Call() { nativeClose(0) }",
            ),
        ],
        "nativeClose",
    );
}

#[test]
fn ordinary_source_cannot_supply_bodyless_native_functions() {
    let invalid = format!("{LIBRARY}\nfunc arbitrary() int\n");
    rejects_tree(
        &[
            ("/app/zore.toml", APP),
            ("/app/main.ore", MAIN),
            ("/adler32/zore.toml", MANIFEST),
            ("/adler32/adler32.ore", &invalid),
        ],
        "require a body",
    );
}

#[test]
fn native_metadata_rejects_unknown_or_unrestricted_configuration() {
    for (manifest, message) in [
        ("name = 'adler32'\n[native]\n", "requires an adapter"),
        (
            "name = 'adler32'\n[native]\nadapter = 'unknown-v1'",
            "unknown native adapter",
        ),
        (
            "name = 'other'\n[native]\nadapter = 'adler32-v1'",
            "requires project name `adler32`",
        ),
        (
            "name = 'adler32'\n[native]\nadapter = 'adler32-v1'\nadapter = 'adler32-v1'",
            "duplicate native adapter",
        ),
        (
            "name = 'adler32'\n[native]\nadapter = 'adler32-v1'\n[native]",
            "duplicate [native]",
        ),
        (
            "name = 'adler32'\n[native]\nsource = 'arbitrary.c'",
            "unsupported native field",
        ),
        (
            "name = 'adler32'\n[native]\nadapter = 'adler32-v1'\nflags = '-lunknown'",
            "unsupported native field",
        ),
    ] {
        rejects_tree(
            &[
                ("/app/zore.toml", APP),
                ("/app/main.ore", MAIN),
                ("/adler32/zore.toml", manifest),
                ("/adler32/adler32.ore", LIBRARY),
            ],
            message,
        );
    }
}

#[test]
fn native_and_dependency_tables_work_in_either_order() {
    for manifest in [
        "name = 'adler32'\n[native]\nadapter = 'adler32-v1'\n[dependencies]\nshared = { path = '../shared' }",
        "name = 'adler32'\n[dependencies]\nshared = { path = '../shared' }\n[native]\nadapter = 'adler32-v1'",
    ] {
        accepts_tree(&[
            ("/app/zore.toml", APP),
            ("/app/main.ore", MAIN),
            ("/adler32/zore.toml", manifest),
            ("/adler32/adler32.ore", LIBRARY),
            ("/shared/zore.toml", "name = 'shared'"),
        ]);
    }
}

#[test]
fn owned_native_wrappers_keep_normal_move_and_async_rules() {
    let accepted = "package main\nimport \"adler32\"\nimport \"std/time\"\nasync func consume(h own adler32.Hasher) error { time.Sleep(time.Millisecond); let value = h.Sum()?; println(value); return nil }\nfunc main() { let h, err = adler32.New(); if err != nil { panic(\"create\") }; let task = go consume(h); let failure = task.wait(); if failure != nil { panic(\"consume\") } }";
    accepts_tree(&[
        ("/app/zore.toml", APP),
        ("/app/main.ore", accepted),
        ("/adler32/zore.toml", MANIFEST),
        ("/adler32/adler32.ore", LIBRARY),
    ]);
    let invalid = accepted.replace("let failure = task.wait();", "let failure = task.wait(); let value, another = h.Sum(); if another != nil { panic(\"moved\") }; println(value);");
    rejects_tree(
        &[
            ("/app/zore.toml", APP),
            ("/app/main.ore", &invalid),
            ("/adler32/zore.toml", MANIFEST),
            ("/adler32/adler32.ore", LIBRARY),
        ],
        "moved",
    );
}
