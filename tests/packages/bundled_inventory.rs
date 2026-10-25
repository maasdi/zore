use std::fs;
use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn readme_lists_exactly_the_bundled_packages() {
    let stdlib = read("compiler/src/driver/stdlib.rs");
    let mut bundled: Vec<String> = stdlib
        .lines()
        .filter_map(|line| {
            let (path, _) = line.trim().strip_prefix('"')?.split_once("\" => [")?;
            Some(format!("std/{path}"))
        })
        .collect();
    bundled.sort();
    assert!(
        !bundled.is_empty(),
        "no bundled packages found in stdlib.rs"
    );

    let readme = read("README.md");
    let (_, rest) = readme
        .split_once("Bundled packages: ")
        .expect("README bundled package list");
    let (list, _) = rest
        .split_once(". ")
        .expect("end of README bundled package list");
    let listed: Vec<String> = list
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();

    assert_eq!(listed, bundled);
}
