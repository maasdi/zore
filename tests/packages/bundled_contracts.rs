use super::*;
use zore::ast::{self, Item, ParamMode};
use zore::diagnostic::Diagnostic;
use zore::resolve::{FileUnit, PackageUnit, initializers, resolve};
use zore::source::Layered;

fn check_mutated_bundle(
    source: &str,
    path: &str,
    mutate: impl Fn(&mut ast::File),
) -> Vec<Diagnostic> {
    let memory = Memory::new()
        .with("/p/zore.toml", MANIFEST)
        .with("/p/main.ore", source);
    let mut sources = SourceMap::new();
    let project = load_project(&mut sources, &memory, Path::new("/p/main.ore"))
        .unwrap_or_else(|_| panic!("load bundled contract fixture"));
    let original = project.units();
    let mut files: Vec<Vec<_>> = original
        .iter()
        .map(|unit| unit.files.iter().map(|file| file.file.clone()).collect())
        .collect();
    for (unit, files) in original.iter().zip(&mut files) {
        if unit.path == path {
            for file in files {
                mutate(file);
            }
        }
    }
    let units: Vec<_> = original
        .into_iter()
        .zip(&files)
        .map(|(unit, files)| PackageUnit {
            path: unit.path,
            name: unit.name,
            files: unit
                .files
                .into_iter()
                .zip(files)
                .map(|(file, replacement)| FileUnit {
                    file: replacement,
                    imports: file.imports,
                })
                .collect(),
        })
        .collect();
    let order = initializers(&units);
    let resolution = resolve(&units, &order);
    let (package, diagnostics) =
        zore::hir::lower::check(resolution, &Layered(&sources, project.std_sources()));
    assert_eq!(package.is_some(), diagnostics.is_empty());
    diagnostics
}

const JSON: &str = r#"package main
import "std/encoding/json"
type Row struct { N int; Maybe json.Optional<string> }
func main() {
    var row = Row{N: 3, Maybe: json.Some("ok")}
    let data, err = json.MarshalWith(row, json.SnakeCase)
    _ = err
    let decodeErr = json.UnmarshalStrict(data[:], row, json.SnakeCase)
    _ = decodeErr
}
"#;

const HTTP: &str = r#"package main
import "std/net/http"
func handler(w mut http.ResponseWriter, r mut http.Request) error { return w.Text(200, "ok") }
func main() { var routes = http.NewRouterBuilder(); routes.Handle("GET /", handler) }
"#;

#[test]
fn current_bundled_json_and_http_match_compiler_contracts() {
    for (source, path) in [(JSON, "std/encoding/json"), (HTTP, "std/net/http")] {
        let diagnostics = check_mutated_bundle(source, path, |_| {});
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }
}

#[test]
fn json_reports_missing_compiler_only_helpers() {
    for name in ["fieldIndex", "sortedNames"] {
        let diagnostics = check_mutated_bundle(JSON, "std/encoding/json", |file| {
            for item in &mut file.items {
                if let Item::Func(function) = item
                    && function.name.text == name
                {
                    function.name.text = format!("renamed{name}");
                }
            }
        });
        let expected =
            format!("bundled JSON codec contract mismatch: missing std/encoding/json.{name}");
        assert!(
            diagnostics.iter().any(|d| d.message() == expected),
            "{diagnostics:#?}"
        );
    }
}

#[test]
fn json_reports_helper_ownership_and_hook_signature_drift() {
    for name in ["sortedNames", "encode", "decode"] {
        let diagnostics = check_mutated_bundle(JSON, "std/encoding/json", |file| {
            for item in &mut file.items {
                if let Item::Func(function) = item
                    && function.name.text == name
                {
                    function.params[0].mode = if name == "sortedNames" {
                        ParamMode::Borrow
                    } else {
                        ParamMode::Own
                    };
                }
            }
        });
        assert!(
            diagnostics.iter().any(|d| d
                .message()
                .starts_with("bundled JSON codec contract mismatch:")),
            "{name}: {diagnostics:#?}"
        );
    }
}

#[test]
fn json_reports_optional_field_order_drift() {
    let diagnostics = check_mutated_bundle(JSON, "std/encoding/json", |file| {
        for item in &mut file.items {
            if let Item::Struct(declaration) = item
                && declaration.name.text == "Optional"
            {
                declaration.fields.swap(0, 1);
            }
        }
    });
    assert!(
        diagnostics.iter().any(|d| d
            .message()
            .contains("Optional needs Value T followed by Set bool")),
        "{diagnostics:#?}"
    );
}

#[test]
fn http_reports_missing_registration_helper() {
    let diagnostics = check_mutated_bundle(HTTP, "std/net/http", |file| {
        file.items
            .retain(|item| !matches!(item, Item::Func(f) if f.name.text == "register"));
    });
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostics[0].message(),
        "bundled HTTP adapter contract mismatch: missing RouterBuilder.register"
    );
}

#[test]
fn http_reports_hook_ownership_drift() {
    let diagnostics = check_mutated_bundle(HTTP, "std/net/http", |file| {
        for item in &mut file.items {
            if let Item::Func(function) = item
                && function.name.text == "Handle"
                && function.receiver.as_ref().is_some_and(|receiver| {
                    matches!(&receiver.ty, ast::Type::Named(name) if name.text == "RouterBuilder")
                })
            {
                function.params[1].mode = ParamMode::Borrow;
            }
        }
    });
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.message()
            == "bundled HTTP adapter contract mismatch: RouterBuilder.Handle/register signature"),
        "{diagnostics:#?}"
    );
}
