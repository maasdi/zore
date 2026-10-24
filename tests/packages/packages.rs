mod bundled_contracts;
mod local_dependencies;
mod native_adapters;

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
        "methods can be declared only on struct or named types defined in this package",
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
            "was not found: import paths start with `myapp` or `std`",
        ),
        (
            "import \"myapp/nothing\"\n",
            "the folder `nothing` has no `.ore` files",
        ),
        (
            "import \"myapp\"\n",
            "the `main` package cannot be imported",
        ),
        ("import \"myapp//shapes\"\n", "is not a valid import path"),
        ("import \"myapp/../shapes\"\n", "is not a valid import path"),
        (
            "import \"std/nothing\"\n",
            "no standard package `std/nothing`",
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
        "package-level `var` is not supported; declare the value with `let`",
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
        (Some("name = \"std\"\n"), "has no valid `name`"),
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
            Err(LoadError::Source(error)) => {
                assert!(error.to_string().contains(message), "{message}: {error}")
            }
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
            "import \"std/time\"\nimport \"std/bufio\"\nimport \"std/os\"\nimport \"std/net\"",
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
    let net = "import \"std/net\"";
    rejects(
        &[(
            "main.ore",
            &main_with("import \"std/time\"", "time.Sleep(\"soon\")"),
        )],
        "mismatched types",
    );
    rejects(
        &[(
            "main.ore",
            &main_with("import \"std/time\"", "time.Sleep()"),
        )],
        "takes 1 argument",
    );
    rejects(
        &[(
            "main.ore",
            &main_with("import \"std/os\"", "let data = os.ReadFile(\"a\")"),
        )],
        "returns 2 values",
    );
    rejects(
        &[(
            "main.ore",
            &main_with(
                "import \"std/os\"",
                "os.WriteFile(\"a\", Array<byte>{}[:], 0o644)",
            ),
        )],
        "must be used or explicitly discarded",
    );
    rejects(
        &[(
            "main.ore",
            &main_with("import \"std/bufio\"", "let r = bufio.NewReader(\"text\")"),
        )],
        "`string` does not satisfy `io.Reader`",
    );
    rejects(
        &[("main.ore", &main_with("import \"std/cancel\"", ""))],
        "no standard package `std/cancel`",
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
            "import \"std/os/exec\"\nimport \"std/path/filepath\"\nimport \"std/unicode/utf8\"",
            "let cmd = exec.Command(\"true\", Array<string>{}[:])
            println(cmd.Path)
            println(filepath.Base(\"/a/b.txt\"))
            println(utf8.RuneLen('é'))",
        ),
    )]);
    rejects(
        &[("main.ore", &main_with("import \"std/os/nope\"", ""))],
        "no standard package `std/os/nope`",
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

#[test]
fn named_types_cross_packages_with_their_methods() {
    let units = "package units

type Celsius float64

type secret int

const Freezing Celsius = 0

func (c Celsius) Fahrenheit() Celsius {
    return c * 9 / 5 + 32
}

func (c Celsius) hidden() int { return 1 }

func Hide() secret { return secret(1) }
";
    accepts(&[
        (
            "main.ore",
            &main_with(
                "import \"myapp/units\"",
                "let c = units.Celsius(100)
            let f units.Celsius = c.Fahrenheit()
            println(f > units.Freezing)
            let raw float64 = float64(f)
            println(raw > 0.0)",
            ),
        ),
        ("units/units.ore", units),
    ]);
    for (body, message) in [
        ("let s = units.secret(1)", "not exported"),
        (
            "let c = units.Celsius(1)\nprintln(c.hidden())",
            "is not exported",
        ),
        (
            "let c = units.Celsius(1)\nlet f float64 = c",
            "mismatched types: expected `float64`, found `units.Celsius`",
        ),
    ] {
        rejects(
            &[
                ("main.ore", &main_with("import \"myapp/units\"", body)),
                ("units/units.ore", units),
            ],
            message,
        );
    }
}

#[test]
fn package_level_lets_are_exported_like_other_names() {
    let settings = "package settings

let Port int = 8080
let secret = \"hidden\"
";
    accepts(&[
        (
            "main.ore",
            &main_with("import \"myapp/settings\"", "println(settings.Port + 1)"),
        ),
        ("settings/settings.ore", settings),
    ]);
    for (body, message) in [
        ("println(settings.secret)", "not exported"),
        ("settings.Port = 1", "cannot assign"),
    ] {
        rejects(
            &[
                ("main.ore", &main_with("import \"myapp/settings\"", body)),
                ("settings/settings.ore", settings),
            ],
            message,
        );
    }
}

#[test]
fn interfaces_are_satisfied_across_packages_by_exported_methods() {
    let io = "package io

type Writer interface {
    mut Write(text string) int
}

type sealed interface {
    hidden() int
}

func Send(w mut Writer, text string) int {
    return w.Write(text)
}
";
    let sink = "package sink

type Buffer struct {
    Count int
}

func (b mut Buffer) Write(text string) int {
    b.Count += text.len()
    return b.Count
}

func (b Buffer) hidden() int { return 0 }
";
    let main = main_with(
        "import \"myapp/io\"\nimport \"myapp/sink\"",
        "var b = sink.Buffer{Count: 0}
    let n = io.Send(b, \"hello\")
    var w io.Writer = sink.Buffer{Count: 1}
    let m = w.Write(\"x\")
    println(n + m)",
    );
    accepts(&[
        ("main.ore", &main),
        ("io/io.ore", io),
        ("sink/sink.ore", sink),
    ]);
    let sealing = "package io

import \"myapp/sink\"

type sealed interface {
    hidden() int
}

func Seal(b own sink.Buffer) int {
    let s sealed = b
    return s.hidden()
}
";
    rejects(
        &[
            ("main.ore", &main_with("import \"myapp/io\"", "_ = io.Seal")),
            ("io/io.ore", sealing),
            ("sink/sink.ore", sink),
        ],
        "`sink.Buffer` does not satisfy `io.sealed`: its method `hidden` is unexported and declared outside the interface's package",
    );
}

#[test]
fn generic_functions_are_called_across_packages() {
    let util = "package util

type Sizer interface {
    Size() int
}

func Largest<T ordered>(items []T) T {
    var best = items[0]
    for _, item in items {
        if item > best {
            best = item
        }
    }
    return best
}

func keep<T any>(item own T) T {
    return item
}

func Pass<T any>(item own T) T {
    return keep(item)
}
";
    let main = main_with(
        "import \"myapp/util\"",
        "let nums = Array<int>{4, 9, 2}
    println(util.Largest(nums[:]))
    println(util.Pass(\"ok\"))
    println(Total(Box{n: 3}))",
    ) + "
type Box struct { n int }

func (b Box) Size() int { return b.n }

func Total<T util.Sizer>(item T) int {
    return item.Size()
}
";
    accepts(&[("main.ore", &main), ("util/util.ore", util)]);
    rejects(
        &[
            (
                "main.ore",
                &main_with("import \"myapp/util\"", "println(util.keep(1))"),
            ),
            ("util/util.ore", util),
        ],
        "`keep` is not exported by package `util`",
    );
}

#[test]
fn generic_types_are_used_across_packages() {
    let box_package = "package box

type Stack<T any> struct {
    items Array<T>
}

func New<T any>() Stack<T> {
    return Stack<T>{items: Array<T>{}}
}

func (s mut Stack<T>) Push(value own T) {
    s.items.push(value)
}

func (s Stack<T>) Len() int {
    return s.items.len()
}

type Pair<A any, B any> struct {
    First A
    Second B
}
";
    let main = main_with(
        "import \"myapp/box\"",
        "var s box.Stack<int> = box.New()
    s.Push(1)
    println(s.Len())
    let p = box.Pair<string, int>{First: \"a\", Second: 2}
    println(p.First)",
    );
    accepts(&[("main.ore", &main), ("box/box.ore", box_package)]);
    rejects(
        &[
            (
                "main.ore",
                &main_with(
                    "import \"myapp/box\"",
                    "let s = box.Stack<int>{items: Array<int>{}}",
                ),
            ),
            ("box/box.ore", box_package),
        ],
        "field `items` of `box.Stack<int64>` is not exported by its package",
    );
}

#[test]
fn slices_maps_and_sort_slice_check_their_element_types() {
    accepts(&[(
        "main.ore",
        &main_with(
            "import \"std/maps\"\nimport \"std/slices\"\nimport \"std/sort\"",
            "var xs = Array<int>{2, 1}
    slices.Sort(xs[:])
    sort.Slice(xs[:], func(a int, b int) bool { return a > b })
    let keys = maps.Keys(map[string]Array<int>{})
    println(slices.Contains(keys[:], \"k\"))",
        ),
    )]);
    for (body, message) in [
        (
            "var xs = Array<Array<int>>{}\n    sort.Slice(xs[:], func(a Array<int>, b Array<int>) bool { return true })",
            "`Array<int64>` does not satisfy `copyable`, the constraint of `T`",
        ),
        (
            "var xs = Array<float64>{1.5}\n    println(slices.Contains(xs[:], 1.5))",
            "`float64` does not satisfy `comparable`, the constraint of `T`",
        ),
        (
            "let xs = Array<int>{1}\n    slices.Sort(xs[:])",
            "cannot take a mutable slice",
        ),
        (
            "let v = maps.Values(map[string]Array<int>{})",
            "`Array<int64>` does not satisfy `copyable`, the constraint of `V`",
        ),
    ] {
        rejects(
            &[(
                "main.ore",
                &main_with(
                    "import \"std/maps\"\nimport \"std/slices\"\nimport \"std/sort\"",
                    &format!("{body}\n    _ = maps.Keys\n    _ = sort.Ints\n    _ = slices.Max"),
                ),
            )],
            message,
        );
    }
}

#[test]
fn initializer_dependencies_follow_interface_dispatch_and_generic_constraints() {
    let declarations = r#"
type Getter interface { Get() int }
type Wider interface { Get() int; Extra() }
type Value struct {}
func (v Value) Get() int { return Later }
func (v Value) Extra() {}
func read(g Getter) int { return g.Get() }
func narrow(g Wider) int { return read(g) }
func generic<T Getter>(g T) int { return g.Get() }
async func worker(g own Getter) int { return g.Get() }
func spawn() int { let t = go worker(Value{}); return t.wait() }
"#;
    for call in [
        "read(Value{})",
        "narrow(Value{})",
        "generic(Value{})",
        "spawn()",
    ] {
        let invalid = format!(
            "package main\n{declarations}\nlet Early = {call}\nlet Later = 42\nfunc main() {{ println(Early) }}"
        );
        rejects(&[("main.ore", &invalid)], "initialized after it");
        let valid = format!(
            "package main\n{declarations}\nlet Later = 42\nlet Early = {call}\nfunc main() {{ println(Early) }}"
        );
        accepts(&[("main.ore", &valid)]);
    }
}

#[test]
fn initializer_dispatch_ignores_implementations_only_created_in_main() {
    accepts(&[(
        "main.ore",
        r#"package main
    type Getter interface { Get() int }
    type Early struct {}
    type Late struct {}
    func (e Early) Get() int { return 42 }
    func (l Late) Get() int { return Later }
    func read(g Getter) int { return g.Get() }
    let First = read(Early{})
    let Later = 99
    func main() { println(read(Late{})); println(First) }
    "#,
    )]);
}

#[test]
fn http_facades_cannot_be_cloned_owned_or_sent_to_other_tasks() {
    let prefix = "package main\nimport \"std/net/http\"\nimport \"std/io\"\nfunc main() {}\n";
    for (declaration, message) in [
        (
            "func bad(r http.Request) { let copy = clone(r); println(copy.Path()) }",
            "clone",
        ),
        (
            "func bad(r http.Request) { let held = mutex(r); _ = held }",
            "slices",
        ),
        (
            "func bad(r own http.Request) { let messages = channel<http.Request>(1); messages.send(r) }",
            "slices",
        ),
        (
            "func worker(r own http.Request) {}\nfunc bad(r own http.Request) { go worker(r) }",
            "borrow",
        ),
        (
            "func take(r own io.Reader) {}\nfunc bad(r own http.Request) { take(r) }",
            "view",
        ),
    ] {
        let source = format!("{prefix}{declaration}").replace(
            "import \"std/io\"\n",
            if declaration.contains("io.Reader") {
                "import \"std/io\"\n"
            } else {
                ""
            },
        );
        rejects(&[("main.ore", &source)], message);
    }
}

#[test]
fn http_plain_function_adapter_and_io_views_are_supported() {
    accepts(&[(
        "main.ore",
        "package main
import \"std/net/http\"
import \"std/io\"
func copy(w mut http.ResponseWriter, r mut http.Request) error {
    let _, err = io.Copy(w, r)
    return err
}
type H struct { n int }
func (h H) Handle(w mut http.ResponseWriter, r mut http.Request) error {
    return http.CallHandler(copy, w, r)
}
func main() {}",
    )]);
}

#[test]
fn json_rejects_unsupported_fields_slices_and_name_collisions() {
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
type User struct { Lock Mutex<int> }
func main() { let _, err = json.Marshal(User{Lock: mutex(0)}); _ = err }
"#,
        )],
        "cannot encode User.Lock",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
func main() { var values = Array<int>{1}; var view = values[:]; let err = json.Unmarshal(Array<byte>{91,93}[:], view); _ = err }
"#,
        )],
        "has no JSON form",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
type Collision struct { ID int; Id int }
func main() { let _, err = json.Marshal(Collision{ID: 1, Id: 2}); _ = err }
"#,
        )],
        "field name collision",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
type Bad struct { N int }
func (b Bad) EncodeJSON(e json.Encoder) { _ = e }
func main() { let _, err = json.Marshal(Bad{N: 1}); _ = err }
"#,
        )],
        "incompatible codec signature",
    );
}

#[test]
fn json_decoder_preserves_input_provenance() {
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
func invalid() json.Decoder { let bytes = Array<byte>{49}; return json.NewDecoder(bytes[:]) }
func main() {}
"#,
        )],
        "cannot return a view",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
func main() { let bytes = Array<byte>{49}; let decoder = json.NewDecoder(bytes[:]); let queue = channel<json.Decoder>(1); queue.send(decoder) }
"#,
        )],
        "a channel cannot carry slices",
    );
    accepts(&[(
        "main.ore",
        r#"package main
import "std/encoding/json"
func parse() (json.Value, error) { let bytes = Array<byte>{49}; return json.Parse(bytes[:]) }
func main() { let _, err = parse(); _ = err }
"#,
    )]);
}

#[test]
fn json_codec_validation_is_separate_from_ordinary_generics() {
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
func main() { let _, err = json.Marshal('x'); _ = err }
"#,
        )],
        "rune has no JSON form",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
func main() { let _, err = json.Marshal(map[bool]int{}); _ = err }
"#,
        )],
        "has no JSON form",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
func ordinary<T any>(value T) { _ = value }
func main() { ordinary(func() {}) }
"#,
        )],
        "cannot be the type argument",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
func main() { var bytes = Array<byte>{49}; let decoder = json.NewDecoder(bytes[:]); let shared = mutex(decoder); _ = shared }
"#,
        )],
        "a mutex cannot guard slices",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
func read(decoder own json.Decoder) {}
func main() { let bytes = Array<byte>{49}; let decoder = json.NewDecoder(bytes[:]); let task = go read(decoder); task.wait() }
"#,
        )],
        "task",
    );
}

#[test]
fn json_fresh_interface_values_require_a_concrete_decoder() {
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
type Codec struct {}
func (c mut Codec) DecodeJSON(d mut json.Decoder) error { return d.Null() }
func main() { var values = [json.Unmarshaler; 1]{Codec{}}; let err = json.Unmarshal(Array<byte>{91,93}[:], values); _ = err }
"#,
        )],
        "a fresh codec interface needs a concrete type or custom decoder",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
func main() { var values = Array<json.Unmarshaler>{}; let err = json.Unmarshal(Array<byte>{91,93}[:], values); _ = err }
"#,
        )],
        "a fresh codec interface needs a concrete type or custom decoder",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/encoding/json"
type Holder struct { Codec json.Unmarshaler }
func main() { var values = map[string]Holder{}; let err = json.Unmarshal(Array<byte>{123,125}[:], values); _ = err }
"#,
        )],
        "Holder[value].Codec: a fresh codec interface",
    );
}

#[test]
fn http_direct_handlers_reject_unsafe_and_opaque_captures() {
    for binding in [
        "var count = 0\n    routes.Handle(\"GET /\", func(w mut http.ResponseWriter, r mut http.Request) error { count += 1; return nil })",
        "let data = Array<int>{1}\n    let view = data[:]\n    routes.Handle(\"GET /\", func(w mut http.ResponseWriter, r mut http.Request) error { println(view.len()); return nil })",
        "let data = Array<int>{1}\n    routes.Handle(\"GET /\", func(w mut http.ResponseWriter, r mut http.Request) error { println(data.len()); return nil })",
    ] {
        let source = format!(
            "package main\nimport \"std/net/http\"\nfunc main() {{ var routes = http.NewRouterBuilder()\n{binding}\n}}\n"
        );
        rejects(&[("main.ore", &source)], "HTTP handler capture");
    }
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/net/http"
func h(w mut http.ResponseWriter, r mut http.Request) error { return nil }
func main() { let f = h; var routes = http.NewRouterBuilder(); routes.Handle("GET /", f) }
"#,
        )],
        "HTTP registration needs a declared function name or inline function literal",
    );
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/net/http"
func main() { var routes = http.NewRouterBuilder(); let f = routes.Handle }
"#,
        )],
        "HTTP registration needs a declared function name or inline function literal",
    );
}

#[test]
fn http_client_frontend_checks_without_native_linker() {
    accepts(&[(
        "main.ore",
        include_str!("../../examples/http-client/main.ore"),
    )]);
}

#[test]
fn http_client_response_cannot_escape_body_borrow() {
    rejects(
        &[(
            "main.ore",
            r#"package main
import "std/context"
import "std/net/http"
func escape(client http.Client) ([]byte, error) {
    let body = Array<byte>{}
    let response = client.Do(context.Background(), "GET", "http://localhost/", http.NewHeader(), body[:])?
    return response.Body[:], nil
}
func main() {}
"#,
        )],
        "cannot return a view",
    );
}

mod sqlite;
