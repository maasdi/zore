use std::path::Path;

use zore::check::{Checked, check_project};
use zore::driver::project::{LoadError, Memory, load_project};
use zore::source::SourceMap;

const MANIFEST: &str = "name = \"myapp\"\nversion = \"0.1.0\"\n";

const SHAPES: &str = "package shapes

const Pi = 3

type Circle struct {
    Radius int
}

type Rect struct {
    w int
    h int
}

func Area(c Circle) int { return c.Radius * Pi }

func area(c Circle) int { return c.Radius }

func NewRect(w int, h int) Rect { return Rect{w: w, h: h} }

func (c Circle) Name() string { return \"circle\" }

func (c Circle) secret() int { return 1 }

func (r Rect) Size() int { return r.w * r.h }
";

struct Outcome {
    messages: Vec<String>,
    checked: bool,
}

/// Checks the project in `files`, whose entry is `main.ore` under `/p`.
fn check(files: &[(&str, &str)]) -> Outcome {
    let mut memory = Memory::new().with("/p/zore.toml", MANIFEST);
    for (path, text) in files {
        memory = memory.with(format!("/p/{path}"), *text);
    }
    let mut sources = SourceMap::new();
    match load_project(&mut sources, &memory, Path::new("/p/main.ore")) {
        Ok(project) => {
            let Checked {
                package,
                diagnostics,
            } = check_project(&project, &sources);
            Outcome {
                messages: diagnostics
                    .iter()
                    .map(|d| d.message().to_string())
                    .collect(),
                checked: package.is_some(),
            }
        }
        Err(LoadError::Diagnostics(diagnostics)) => Outcome {
            messages: diagnostics
                .iter()
                .map(|d| d.message().to_string())
                .collect(),
            checked: false,
        },
        Err(LoadError::Source(error)) => panic!("{error}"),
    }
}

fn main_with(imports: &str, body: &str) -> String {
    format!("package main\n{imports}\nfunc main() {{\n{body}\n}}\n")
}

fn accepts(files: &[(&str, &str)]) {
    let outcome = check(files);
    assert!(
        outcome.checked && outcome.messages.is_empty(),
        "{files:#?}\n{:#?}",
        outcome.messages
    );
}

fn rejects(files: &[(&str, &str)], message: &str) {
    let outcome = check(files);
    assert!(!outcome.checked, "{files:#?} was accepted");
    assert!(
        outcome.messages.iter().any(|m| m.contains(message)),
        "{files:#?}\nexpected {message:?}, got {:#?}",
        outcome.messages
    );
}

fn with_shapes(main: &str) -> Vec<(&str, &str)> {
    vec![("main.ore", main), ("shapes/shapes.ore", SHAPES)]
}

const IMPORT_SHAPES: &str = "import \"myapp/shapes\"\n";

#[test]
fn exported_functions_types_methods_and_constants_work_across_packages() {
    let main = main_with(
        IMPORT_SHAPES,
        "let c = shapes.Circle{Radius: 2}
        println(shapes.Area(c))
        println(c.Name())
        println(shapes.Pi)
        const Twice = shapes.Pi * 2
        println(Twice)
        var rect shapes.Rect = shapes.NewRect(2, 3)
        println(rect.Size())
        let copy = c
        println(copy.Radius)",
    );
    accepts(&with_shapes(&main));
}

#[test]
fn exported_methods_become_function_values_across_packages() {
    let main = main_with(
        IMPORT_SHAPES,
        "let c = shapes.Circle{Radius: 2}
        let name = c.Name
        println(name())
        let rect = shapes.NewRect(2, 3)
        let size = rect.Size
        println(size())",
    );
    accepts(&with_shapes(&main));
}

#[test]
fn package_functions_convert_to_function_values() {
    let main = main_with(
        IMPORT_SHAPES,
        "let area = shapes.Area
        let c = shapes.Circle{Radius: 2}
        println(area(c))",
    );
    accepts(&with_shapes(&main));
}

#[test]
fn files_in_one_folder_share_a_package_and_sibling_folders_do_not() {
    accepts(&[
        ("main.ore", "package main\nfunc main() { helper() }\n"),
        ("helper.ore", "package main\nfunc helper() { println(1) }\n"),
    ]);
    rejects(
        &[
            ("main.ore", "package main\nfunc main() { helper() }\n"),
            ("other/helper.ore", "package main\nfunc helper() {}\n"),
        ],
        "cannot find `helper` in this scope",
    );
    rejects(
        &[
            ("main.ore", "package main\nfunc main() {}\nfunc dup() {}\n"),
            ("two.ore", "package main\nfunc dup() {}\n"),
        ],
        "duplicate declaration `dup`",
    );
    rejects(
        &[
            ("main.ore", "package main\nfunc main() {}\n"),
            ("two.ore", "package extra\n"),
        ],
        "another file in the folder declares `main`",
    );
}

#[test]
fn unexported_names_cannot_be_used_from_another_package() {
    for (body, message) in [
        (
            "println(shapes.area(shapes.Circle{Radius: 1}))",
            "`area` is not exported by package `shapes`",
        ),
        (
            "println(shapes.Nope)",
            "package `shapes` does not declare `Nope`",
        ),
        (
            "let r = shapes.NewRect(1, 2)\nprintln(r.w)",
            "field `w` of `shapes.Rect` is not exported",
        ),
        (
            "let r = shapes.Rect{w: 1, h: 2}\n_ = r",
            "field `w` of `shapes.Rect` is not exported",
        ),
        (
            "let c = shapes.Circle{Radius: 1}\nprintln(c.secret())",
            "method `secret` of `shapes.Circle` is not exported",
        ),
        (
            "let c = shapes.Circle{Radius: 1}\nlet f = c.secret\n_ = f",
            "method `secret` of `shapes.Circle` is not exported",
        ),
    ] {
        rejects(&with_shapes(&main_with(IMPORT_SHAPES, body)), message);
    }
}

#[test]
fn qualified_names_must_name_a_package_and_the_right_kind_of_member() {
    for (body, message) in [
        (
            "let x = shapes\n_ = x",
            "use of package `shapes` without selector",
        ),
        (
            "let x = other.Thing\n_ = x",
            "cannot find `other` in this scope",
        ),
        ("let c = shapes.Circle(1)", "is constructed with"),
        ("shapes.Pi()", "is not a function"),
    ] {
        rejects(&with_shapes(&main_with(IMPORT_SHAPES, body)), message);
    }
    rejects(
        &with_shapes(&main_with(IMPORT_SHAPES, "let x shapes.Pi = 1\n_ = x")),
        "`shapes.Pi` is not a type",
    );
    rejects(
        &with_shapes(&main_with(
            IMPORT_SHAPES,
            "let x = shapes.Pi{Radius: 1}\n_ = x",
        )),
        "is not a struct type",
    );
}

#[test]
fn methods_cannot_be_declared_on_imported_types() {
    let main = format!(
        "{}\nfunc (c shapes.Circle) Extra() int {{ return 1 }}\n",
        main_with(IMPORT_SHAPES, "println(shapes.Pi)")
    );
    rejects(
        &with_shapes(&main),
        "methods can be declared only on struct types defined in this package",
    );
}

#[test]
fn import_declarations_are_checked() {
    for (imports, message) in [
        (
            "import \"myapp/shapes\"\nimport \"myapp/shapes\"\n",
            "is imported twice",
        ),
        (
            "import \"other/shapes\"\n",
            "was not found: import paths start with `myapp` or `zore`",
        ),
        (
            "import \"myapp/nothing\"\n",
            "the folder `nothing` has no `.ore` files",
        ),
        (
            "import \"myapp\"\n",
            "names the project, not a package inside it",
        ),
        ("import \"myapp//shapes\"\n", "is not a valid import path"),
        ("import \"myapp/../shapes\"\n", "is not a valid import path"),
        (
            "import \"zore/nothing\"\n",
            "no standard package `zore/nothing`",
        ),
    ] {
        rejects(&with_shapes(&main_with(imports, "println(1)")), message);
    }
    rejects(
        &with_shapes(&main_with(IMPORT_SHAPES, "println(1)")),
        "package `shapes` is imported and not used",
    );
    rejects(
        &with_shapes(&format!(
            "{}\nfunc shapes() {{}}\n",
            main_with(IMPORT_SHAPES, "println(1)")
        )),
        "import `shapes` conflicts with a declaration of the same name",
    );
    rejects(
        &[
            (
                "main.ore",
                "package main\nimport \"myapp/int\"\nfunc main() {}\n",
            ),
            ("int/int.ore", "package int\nfunc F() {}\n"),
        ],
        "import `int` shadows a predeclared name",
    );
    rejects(
        &[
            (
                "main.ore",
                "package main\nimport \"myapp/a/util\"\nimport \"myapp/b/util\"\nfunc main() { util.F() }\n",
            ),
            ("a/util/u.ore", "package util\nfunc F() {}\n"),
            ("b/util/u.ore", "package util\nfunc F() {}\n"),
        ],
        "two imports in this file are both named `util`",
    );
    for (syntax, message) in [
        (
            "import s \"myapp/shapes\"\n",
            "expected a double-quoted import path",
        ),
        (
            "import . \"myapp/shapes\"\n",
            "expected a double-quoted import path",
        ),
        (
            "import _ \"myapp/shapes\"\n",
            "expected a double-quoted import path",
        ),
        (
            "import (\n\"myapp/shapes\"\n)\n",
            "grouped imports are not supported",
        ),
    ] {
        rejects(&with_shapes(&main_with(syntax, "println(1)")), message);
    }
}

#[test]
fn package_names_match_folders_and_cannot_cycle() {
    rejects(
        &[
            (
                "main.ore",
                "package main\nimport \"myapp/shapes\"\nfunc main() { println(shapes.Pi) }\n",
            ),
            ("shapes/s.ore", "package other\nconst Pi = 1\n"),
        ],
        "so it must be named `shapes`",
    );
    let cycle = [
        (
            "main.ore",
            "package main\nimport \"myapp/a\"\nfunc main() { a.F() }\n",
        ),
        (
            "a/a.ore",
            "package a\nimport \"myapp/b\"\nfunc F() { b.G() }\n",
        ),
        (
            "b/b.ore",
            "package b\nimport \"myapp/a\"\nfunc G() { a.F() }\n",
        ),
    ];
    rejects(&cycle, "import cycle: myapp/a -> myapp/b -> myapp/a");
    rejects(
        &[
            (
                "main.ore",
                "package main\nimport \"myapp/a\"\nfunc main() { a.F() }\n",
            ),
            ("a/a.ore", "package a\nimport \"myapp/a\"\nfunc F() {}\n"),
        ],
        "import cycle: myapp/a -> myapp/a",
    );
}

#[test]
fn types_are_nominal_per_package_and_may_share_a_name() {
    let a = ("a/a.ore", "package a\ntype T struct { N int }\n");
    let b = (
        "b/b.ore",
        "package b\ntype T struct { N int }\nfunc Make() T { return T{N: 2} }\n",
    );
    accepts(&[
        (
            "main.ore",
            "package main\nimport \"myapp/a\"\nimport \"myapp/b\"\nfunc main() {\nvar x a.T = a.T{N: 1}\nvar y b.T = b.Make()\nprintln(x.N + y.N)\n}\n",
        ),
        a,
        b,
    ]);
    rejects(
        &[
            (
                "main.ore",
                "package main\nimport \"myapp/a\"\nimport \"myapp/b\"\nfunc main() {\nvar x a.T = b.Make()\n_ = x\n}\n",
            ),
            a,
            b,
        ],
        "expected `a.T`, found `b.T`",
    );
}

#[test]
fn package_level_variables_and_native_functions_are_rejected_in_imports() {
    rejects(
        &[
            (
                "main.ore",
                "package main\nimport \"myapp/a\"\nfunc main() { a.F() }\n",
            ),
            ("a/a.ore", "package a\nvar X = 1\nfunc F() {}\n"),
        ],
        "package-level `let` and `var` are not supported",
    );
    rejects(
        &[(
            "main.ore",
            "package main\nfunc native(s string) string\nfunc main() {}\n",
        )],
        "function declarations require a body",
    );
}

#[test]
fn a_package_imported_twice_is_loaded_once() {
    let outcome = check(&[
        (
            "main.ore",
            "package main\nimport \"myapp/a\"\nimport \"myapp/b\"\nfunc main() { a.F()\nb.G() }\n",
        ),
        (
            "a/a.ore",
            "package a\nimport \"myapp/shared\"\nfunc F() { shared.H() }\n",
        ),
        (
            "b/b.ore",
            "package b\nimport \"myapp/shared\"\nfunc G() { shared.H() }\n",
        ),
        ("shared/s.ore", "package shared\nfunc H() {}\n"),
    ]);
    assert!(outcome.checked, "{:#?}", outcome.messages);
}

#[test]
fn imports_need_a_named_project() {
    let files = [
        (
            "main.ore",
            "package main\nimport \"myapp/shapes\"\nfunc main() { println(shapes.Pi) }\n",
        ),
        ("shapes/s.ore", "package shapes\nconst Pi = 1\n"),
    ];
    for (manifest, message) in [
        (None, "no `zore.toml` project file was found"),
        (Some("version = \"1\"\n"), "has no valid `name`"),
        (Some("name = \"zore\"\n"), "has no valid `name`"),
        (Some("name = \"my app\"\n"), "has no valid `name`"),
    ] {
        let mut memory = Memory::new();
        if let Some(text) = manifest {
            memory = memory.with("/p/zore.toml", text);
        }
        for (path, text) in files {
            memory = memory.with(format!("/p/{path}"), text);
        }
        let mut sources = SourceMap::new();
        match load_project(&mut sources, &memory, Path::new("/p/main.ore")) {
            Err(LoadError::Diagnostics(diagnostics)) => assert!(
                diagnostics.iter().any(|d| d.message().contains(message)),
                "{message}: {diagnostics:#?}"
            ),
            Err(LoadError::Source(error)) => panic!("{error}"),
            Ok(_) => panic!("{manifest:?} was accepted"),
        }
    }
}

#[test]
fn diagnostics_in_imported_files_name_that_file() {
    let mut memory = Memory::new().with("/p/zore.toml", MANIFEST);
    memory = memory
        .with(
            "/p/main.ore",
            "package main\nimport \"myapp/a\"\nfunc main() { a.F() }\n",
        )
        .with("/p/a/a.ore", "package a\nfunc F() { println(missing) }\n");
    let mut sources = SourceMap::new();
    let Ok(project) = load_project(&mut sources, &memory, Path::new("/p/main.ore")) else {
        panic!("the project loads");
    };
    let checked = check_project(&project, &sources);
    let rendered = checked.diagnostics[0].render(&sources).unwrap();
    assert!(rendered.contains("/p/a/a.ore:2:"), "{rendered}");
}

#[test]
fn the_time_os_bufio_and_net_packages_type_check_their_uses() {
    accepts(&[(
        "main.ore",
        &main_with(
            "import \"zore/time\"\nimport \"zore/bufio\"\nimport \"zore/os\"\nimport \"zore/net\"",
            "time.Sleep(10 * time.Millisecond)
            let started = time.Now()
            var input = bufio.NewScanner(os.Stdin())
            let more bool = input.Scan()
            let line string = input.Text()
            _ = input.Err()
            let data, readErr = os.ReadFile(\"a.txt\")
            _ = readErr
            let wrote = os.WriteFile(\"b.txt\", data[:], 0o644)
            _ = wrote
            let listener, listenErr = net.Listen(\"tcp\", \"127.0.0.1:0\")
            _ = listenErr
            let conn, acceptErr = listener.Accept()
            _ = acceptErr
            var chunk = Array<byte>{0, 0, 0, 0}
            let count, chunkErr = conn.Read(chunk[:])
            _ = chunkErr
            let sent, sendErr = conn.Write(chunk[:count])
            _ = sendErr
            let closed = conn.CloseWrite()
            _ = closed
            _ = conn.SetReadDeadline(time.Now() + time.Second)
            let address string = listener.Addr()
            println(address + line)
            println(more)
            println(sent + time.Since(started))
            _ = conn.Close()",
        ),
    )]);
}

#[test]
fn the_io_packages_reject_misuse() {
    let net = "import \"zore/net\"";
    rejects(
        &[(
            "main.ore",
            &main_with("import \"zore/time\"", "time.Sleep(\"soon\")"),
        )],
        "mismatched types",
    );
    rejects(
        &[(
            "main.ore",
            &main_with("import \"zore/time\"", "time.Sleep()"),
        )],
        "takes 1 argument",
    );
    rejects(
        &[(
            "main.ore",
            &main_with("import \"zore/os\"", "let data = os.ReadFile(\"a\")"),
        )],
        "returns 2 values",
    );
    rejects(
        &[(
            "main.ore",
            &main_with(
                "import \"zore/os\"",
                "os.WriteFile(\"a\", Array<byte>{}[:], 0o644)",
            ),
        )],
        "must be used or explicitly discarded",
    );
    rejects(
        &[("main.ore", &main_with("import \"zore/io\"", ""))],
        "no standard package `zore/io`",
    );
    rejects(
        &[("main.ore", &main_with("import \"zore/cancel\"", ""))],
        "no standard package `zore/cancel`",
    );
    rejects(
        &[("main.ore", &main_with(net, "let conn = net.Conn{id: 1}"))],
        "not exported",
    );
    rejects(
        &[("main.ore", &main_with(net, "let id = net.listen(0, \"x\")"))],
        "not exported",
    );
    rejects(
        &[(
            "main.ore",
            &main_with(
                net,
                "let conn, err = net.Dial(\"tcp\", \"x\")\n_ = err\nlet other = conn\nlet again = conn",
            ),
        )],
        "use of moved value",
    );
    rejects(
        &[(
            "main.ore",
            &main_with(
                net,
                "let conn, err = net.Dial(\"tcp\", \"x\")\n_ = err\n_ = conn.Close()\n_ = conn.Close()",
            ),
        )],
        "use of moved value",
    );
}

#[test]
fn nested_standard_packages_are_named_by_their_last_segment() {
    accepts(&[(
        "main.ore",
        &main_with(
            "import \"zore/os/exec\"\nimport \"zore/path/filepath\"\nimport \"zore/unicode/utf8\"",
            "let cmd = exec.Command(\"true\", Array<string>{}[:])
            println(cmd.Path)
            println(filepath.Base(\"/a/b.txt\"))
            println(utf8.RuneLen('é'))",
        ),
    )]);
    rejects(
        &[("main.ore", &main_with("import \"zore/os/nope\"", ""))],
        "no standard package `zore/os/nope`",
    );
}

#[test]
fn exported_async_functions_become_async_function_values_across_packages() {
    let shapes = "package shapes

async func Load(id int) int { return id }

func local() int { return 1 }
";
    let main = "package main
import \"myapp/shapes\"

async func run() int {
    let load = shapes.Load
    return await load(3)
}

func main() {
    let t = go run()
    println(t.wait())
}
";
    accepts(&[("main.ore", main), ("shapes/shapes.ore", shapes)]);
}
