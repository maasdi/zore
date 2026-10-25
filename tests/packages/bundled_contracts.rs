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
    let count = diagnostics
        .iter()
        .filter(|d| {
            d.message()
                == "bundled JSON codec contract mismatch: Optional needs Value T followed by Set bool"
        })
        .count();
    assert_eq!(count, 2, "{diagnostics:#?}");
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
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostics[0].message(),
        "bundled HTTP adapter contract mismatch: RouterBuilder.Handle: parameter 3 is `func(mut http.ResponseWriter, mut http.Request) error`, expected `own func(mut http.ResponseWriter, mut http.Request) error`"
    );
}

const JSON_MANY_TYPES: &str = r#"package main
import "std/encoding/json"
type Row struct { N int }
func main() {
    var row = Row{N: 3}
    let first, firstErr = json.Marshal(1)
    _ = first
    _ = firstErr
    let second, secondErr = json.Marshal("x")
    _ = second
    _ = secondErr
    let third, thirdErr = json.MarshalWith(row, json.SnakeCase)
    _ = third
    _ = thirdErr
    var count = 0
    var text = ""
    let decodeFirst = json.Unmarshal(first[:], count)
    _ = decodeFirst
    let decodeSecond = json.Unmarshal(second[:], text)
    _ = decodeSecond
    let decodeThird = json.Unmarshal(third[:], row)
    _ = decodeThird
}
"#;

fn rename_json_function(source: &str, name: &'static str) -> Vec<Diagnostic> {
    check_mutated_bundle(source, "std/encoding/json", |file| {
        for item in &mut file.items {
            if let Item::Func(function) = item
                && function.name.text == name
            {
                function.name.text = format!("renamed{name}");
            }
        }
    })
}

#[test]
fn json_contract_drift_is_reported_once() {
    for name in ["sortedNames", "fieldIndex"] {
        let diagnostics = rename_json_function(JSON_MANY_TYPES, name);
        let expected =
            format!("bundled JSON codec contract mismatch: missing std/encoding/json.{name}");
        let count = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message() == expected)
            .count();
        assert_eq!(count, 1, "{name}: {diagnostics:#?}");
    }
}

const USER_HTTP: &str = r#"package http

type Request struct { target string }
type ResponseWriter struct { status int }
type Handler interface { Handle(w mut ResponseWriter, r mut Request) error }
type HandlerFactory interface { NewHandler() (Handler, error) }
type RouterBuilder struct { count int }

func NewRouterBuilder() RouterBuilder { return RouterBuilder{count: 0} }

func (b mut RouterBuilder) Handle(pattern string, handler own func(mut ResponseWriter, mut Request) error) {
    b.count += 1
}

func (b mut RouterBuilder) register(pattern string, factory own HandlerFactory) {
    b.count += 1
}

func (b RouterBuilder) Count() int { return b.count }
"#;

const USER_HTTP_MAIN: &str = r#"package main
import "myapp/http"
func main() {
    var builder = http.NewRouterBuilder()
    builder.Handle("GET /", func(w mut http.ResponseWriter, r mut http.Request) error { return nil })
    println(builder.Count())
    println(routes())
}
"#;

const STD_HTTP_ROUTES: &str = r#"package main
import "std/net/http"
func routes() int {
    var builder = http.NewRouterBuilder()
    builder.Handle("GET /", func(w mut http.ResponseWriter, r mut http.Request) error { return w.Text(200, "ok") })
    return 1
}
"#;

#[test]
fn user_package_named_http_is_not_bundled_http() {
    let memory = Memory::new()
        .with("/p/zore.toml", MANIFEST)
        .with("/p/http/http.ore", USER_HTTP)
        .with("/p/main.ore", USER_HTTP_MAIN)
        .with("/p/routes.ore", STD_HTTP_ROUTES);
    let mut sources = SourceMap::new();
    let project = load_project(&mut sources, &memory, Path::new("/p/main.ore"))
        .unwrap_or_else(|_| panic!("load user http package fixture"));
    let Checked {
        package,
        diagnostics,
    } = check_project(&project, &sources);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let package = package.expect("a checked package");
    let bundled = package.declared_type("std/net/http", "Handler").unwrap();
    let user = package.declared_type("myapp/http", "Handler").unwrap();
    assert_ne!(bundled, user);
    assert_eq!(package.types.display(bundled).to_string(), "http.Handler");
    assert_eq!(package.types.display(user).to_string(), "http.Handler");
    let adapters = |prefix: &str| {
        package
            .functions
            .iter()
            .filter(|function| function.name.starts_with(prefix))
            .count()
    };
    assert_eq!(adapters("http$invoke"), 1);
    assert_eq!(adapters("http$factory"), 1);
    let user_handle = package
        .functions
        .iter()
        .find(|function| function.name == "myapp/http.RouterBuilder.Handle")
        .expect("the user's Handle method");
    assert!(!user_handle.probe);
}
