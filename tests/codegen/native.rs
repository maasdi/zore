//! Requires clang (LLVM 15+) and rustc 1.98+; a missing toolchain fails rather than skips.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use zore::build::{BuildError, TempDir, build, emit_llvm};
use zore::source::SourceMap;

fn run(source: &str) -> Output {
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("program");
    build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    Command::new(&executable)
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .expect("run built program")
}

fn build_file(source: &str, output: &Path) -> Result<(), BuildError> {
    let mut sources = SourceMap::new();
    let id = sources.add("test.ore", source.into()).unwrap();
    build(sources.file(id).unwrap(), output)
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn main_body(stmts: &str) -> String {
    format!("package main\n\nfunc main() {{\n{stmts}\n}}\n")
}

fn prints(source: &str, expected: &str) {
    let output = run(source);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), expected);
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn error_values_preserve_nil_and_message_equality() {
    let source = "package main
func make(message string) error { return error(message) }
func ok() error { return nil }
func pair() (int, error) { return 7, error(\"failed\") }
func main() {
    println(error(\"x\") == error(\"x\"))
    println(error(\"\") != nil)
    println(nil == ok())
    println(make(\"x\") == error(\"x\"))
    println(make(\"y\") != error(\"x\"))
    let value, _ = pair()
    println(value)
    _ = ok()
}";
    prints(source, "true\ntrue\ntrue\ntrue\ntrue\n7\n");
}

#[test]
fn named_error_can_be_checked_and_replaced_after_use() {
    let source = "package main
func main() {
    var err = error(\"first\")
    if err != nil { println(\"handled first\") }
    err = error(\"second\")
    _ = err
}";
    prints(source, "handled first\n");
}

#[test]
fn propagation_forwards_results_and_zero_fills_on_error() {
    let source = "package main
func source(fail bool) (int, string, error) {
    if fail { return 9, \"discarded\", error(\"failed\") }
    return 7, \"ok\", nil
}
func forward(fail bool) (int, string, error) { return source(fail)? }
func main() {
    let good, text, goodErr = forward(false)
    println(good)
    println(text)
    println(goodErr == nil)
    let bad, empty, badErr = forward(true)
    println(bad)
    println(empty == \"\")
    println(badErr == error(\"failed\"))
}";
    prints(source, "7\nok\ntrue\n0\ntrue\ntrue\n");
}

#[test]
fn propagation_skips_later_arguments_and_drops_owned_values() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func source(fail bool) (int, error) {
    if fail { return 0, error(\"failed\") }
    return 7, nil
}
func later() int { println(99); return 2 }
func sum(a int, b int) int { return a + b }
func work(fail bool) (int, error) {
    let first = Guard{id: 1}
    let second = Guard{id: 2}
    let value = sum(source(fail)?, later())
    println(value)
    return value, nil
}
func main() {
    let failed, failure = work(true)
    println(failed)
    println(failure == error(\"failed\"))
    let good, success = work(false)
    println(good)
    println(success == nil)
}";
    prints(source, "2\n1\n0\ntrue\n99\n9\n2\n1\n9\ntrue\n");
}

#[test]
fn propagation_cleans_failed_move_results_and_returns_zero_move_value() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func source(fail bool) (Guard, error) {
    if fail { return Guard{id: 3}, error(\"failed\") }
    return Guard{id: 4}, nil
}
func forward(fail bool) (Guard, error) {
    let outer = Guard{id: 1}
    return (source(fail)?)
}
func main() {
    let failed, failure = forward(true)
    println(failed.id)
    println(failure == error(\"failed\"))
    drop(failed)
    let good, success = forward(false)
    println(good.id)
    println(success == nil)
    drop(good)
}";
    prints(source, "3\n1\n0\ntrue\n0\n1\n4\ntrue\n4\n");
}

#[test]
fn propagation_of_error_only_call_returns_nil_on_success() {
    let source = "package main
func source(fail bool) error {
    if fail { return error(\"failed\") }
    return nil
}
func forward(fail bool) error { return source(fail)? }
func constructed() error { return error(\"constructed\")? }
func main() {
    println(forward(false) == nil)
    println(forward(true) == error(\"failed\"))
    println(constructed() == error(\"constructed\"))
}";
    prints(source, "true\ntrue\ntrue\n");
}

fn panics(source: &str, message: &str, stdout_before: &str) {
    let output = run(source);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{source}\n{}",
        stderr(&output)
    );
    assert_eq!(stdout(&output), stdout_before, "{source}");
    let err = stderr(&output);
    assert!(err.starts_with("panic in the main task: "), "{err}");
    assert!(
        err.contains(message),
        "{source}\nexpected {message:?} in {err:?}"
    );
}

#[test]
fn move_values_drop_on_scope_return_and_transfer() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func consume(g own Guard) { println(100) }
func early() {
    let first = Guard{id: 1}
    {
        let second = Guard{id: 2}
        let third = Guard{id: 3}
    }
    return
}
func main() {
    early()
    let transferred = Guard{id: 4}
    consume(transferred)
    drop(Guard{id: 5})
}";
    prints(source, "3\n2\n1\n100\n4\n5\n");
}

#[test]
fn move_values_drop_on_branches_and_loop_exits() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func main() {
    if true { let a = Guard{id: 1} } else { let b = Guard{id: 2} }
    var count = 0
    for {
        let guard = Guard{id: 3}
        count += 1
        if count == 1 { continue }
        break
    }
}";
    prints(source, "1\n3\n3\n");
}

#[test]
fn panic_unwinds_custom_drops_in_reverse_order() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func inner() {
    let first = Guard{id: 1}
    let second = Guard{id: 2}
    var zero = 0
    println(5 / zero)
}
func main() {
    let outer = Guard{id: 3}
    inner()
}";
    panics(source, "division by zero", "2\n1\n3\n");
}

#[test]
fn panic_during_unwind_aborts_before_other_drops() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() {
    if g.id == 2 {
        var zero = 0
        println(1 / zero)
    } else {
        println(g.id)
    }
}
func main() {
    let first = Guard{id: 1}
    let second = Guard{id: 2}
    var zero = 0
    println(1 / zero)
}";
    let output = run(source);
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn custom_drop_precedes_fields_and_replacement_drops_old_value() {
    let source = "package main
type Inner struct { id int }
func (i mut Inner) drop() { println(i.id) }
type Outer struct {
    first Inner
    second Inner
}
func (o mut Outer) drop() { println(100) }
func main() {
    var outer = Outer{first: Inner{id: 1}, second: Inner{id: 2}}
    outer.second = Inner{id: 3}
}";
    prints(source, "2\n100\n3\n1\n");
}

#[test]
fn partial_move_through_a_call_drops_only_the_remaining_field() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
type Wrapper struct { a Guard; b Guard }
func consume(g own Guard) { println(100) }
func main() {
    var w = Wrapper{a: Guard{id: 1}, b: Guard{id: 2}}
    consume(w.a)
    drop(w.b)
}";
    prints(source, "100\n1\n2\n");
}

#[test]
fn partial_move_then_reinitialize_drops_each_value_once() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
type Wrapper struct { a Guard }
func main() {
    var w = Wrapper{a: Guard{id: 1}}
    let taken = w.a
    drop(taken)
    w.a = Guard{id: 2}
}";
    prints(source, "1\n2\n");
}

#[test]
fn partial_move_without_reinitialization_skips_cleanup_for_that_field() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
type Wrapper struct { a Guard; b Guard }
func main() {
    var w = Wrapper{a: Guard{id: 1}, b: Guard{id: 2}}
    drop(w.a)
}";
    prints(source, "1\n2\n");
}

#[test]
fn arrays_construct_index_and_print() {
    let source = main_body(
        "var xs = [int; 3]{1, 2, 3}
        println(xs[0])
        println(xs[1])
        println(xs[2])
        xs[1] = 9
        println(xs[1])",
    );
    prints(&source, "1\n2\n3\n9\n");
}

#[test]
fn array_index_out_of_range_panics() {
    panics(
        &main_body("let xs = [int; 3]{1, 2, 3}\nvar i = -1\nprintln(xs[i])"),
        "index out of range",
        "",
    );
    panics(
        &main_body("let xs = [int; 3]{1, 2, 3}\nvar i = 3\nprintln(xs[i])"),
        "index out of range",
        "",
    );
    // A huge unsigned index must not wrap to a valid one when widened to int64.
    panics(
        &main_body(
            "let xs = [int; 3]{1, 2, 3}\nvar i uint64 = 18446744073709551615\nprintln(xs[i])",
        ),
        "index out of range",
        "",
    );
}

#[test]
fn array_of_custom_drop_elements_drops_in_reverse_order_at_scope_exit() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func main() {
    var xs = [Guard; 3]{Guard{id: 1}, Guard{id: 2}, Guard{id: 3}}
}";
    prints(source, "3\n2\n1\n");
}

#[test]
fn array_index_assignment_drops_the_old_element_first() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
func main() {
    var xs = [Guard; 2]{Guard{id: 1}, Guard{id: 2}}
    xs[0] = Guard{id: 3}
}";
    prints(source, "1\n2\n3\n");
}

#[test]
fn struct_containing_array_field_drops_elements_then_struct() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }
type Wrapper struct { items [Guard; 2] }
func (w mut Wrapper) drop() { println(100) }
func main() {
    var w = Wrapper{items: [Guard; 2]{Guard{id: 1}, Guard{id: 2}}}
}";
    prints(source, "100\n2\n1\n");
}

#[test]
fn replacing_an_array_element_through_a_custom_drop_ancestor_aborts_on_panic() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() {
    if g.id == 99 {
        var zero = 0
        println(1 / zero)
    } else {
        println(g.id)
    }
}
type Container struct { items [Guard; 1] }
func (c mut Container) drop() { println(100) }
func main() {
    var c = Container{items: [Guard; 1]{Guard{id: 99}}}
    c.items[0] = Guard{id: 1}
}";
    let output = run(source);
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

/// A `Guard` whose destructor prints its id and panics for id 1.
const PANICKING_GUARD: &str = "type Guard struct { id int }
func (g mut Guard) drop() {
    println(g.id)
    if g.id == 1 {
        var zero = 0
        println(1 / zero)
    }
}";

#[test]
fn a_panicking_replaced_element_is_never_dropped_twice() {
    panics(
        &format!(
            "package main\n{PANICKING_GUARD}\nfunc main() {{\nvar gs = [Guard; 2]{{Guard{{id: 1}}, Guard{{id: 2}}}}\ngs[0] = Guard{{id: 3}}\n}}\n"
        ),
        "division by zero",
        "1\n2\n3\n",
    );
    panics(
        &format!(
            "package main\n{PANICKING_GUARD}\nfunc replace(g mut Guard) {{ g = Guard{{id: 3}} }}\nfunc main() {{\nvar gs = [Guard; 2]{{Guard{{id: 1}}, Guard{{id: 2}}}}\nreplace(gs[0])\n}}\n"
        ),
        "division by zero",
        "1\n2\n3\n",
    );
}

#[test]
fn scratch_flag_allocas_live_in_the_entry_block() {
    let source = "package main
type Guard struct { id int }
func (g mut Guard) drop() {}
func inspect(g Guard) {}
func main() {
    var gs = [Guard; 2]{Guard{id: 1}, Guard{id: 2}}
    for var i = 0; i < 2; i += 1 {
        inspect(gs[i])
        gs[i] = Guard{id: i}
    }
}
";
    let mut sources = SourceMap::new();
    let id = sources.add("test.ore", source.into()).unwrap();
    let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
    for function in ir.split("define ").skip(1) {
        let body = function
            .split_once("br label %bb0")
            .map_or("", |(_, rest)| rest);
        assert!(
            !body.contains("alloca"),
            "alloca after the entry block:\n{function}"
        );
    }
}

/// A `Guard` whose destructor prints its id.
const GUARD: &str = "type Guard struct { id int }
func (g mut Guard) drop() { println(g.id) }";

#[test]
fn dynamic_arrays_store_index_and_slice() {
    prints(
        &slice_main(
            "func bump(xs mut Array<int>) { xs[0] += 100 }
            func make() Array<int> { return Array<int>{7, 8, 9} }
            func first(xs Array<int>) int { return xs[0] }",
            "var data = Array<int>{1, 2, 3}
            println(data[1])
            data[1] = 20
            bump(data)
            show(data[:], 3)
            var tail mut []int = data[1:]
            tail[0] = 5
            println(data[1])
            println(first(make()))
            show(make()[1:], 2)
            let empty = Array<int>{}
            show(empty[:], 0)
            var i uint8 = 2
            println(data[i])",
        ),
        "2\n101\n20\n3\n5\n7\n8\n9\n3\n",
    );
}

#[test]
fn dynamic_array_elements_are_dropped_once_in_reverse_order() {
    prints(
        &format!(
            "package main\n{GUARD}
func consume(gs own Array<Guard>) {{ println(0) }}
func id(g Guard) int {{ return g.id }}
func main() {{
    var gs = Array<Guard>{{Guard{{id: 1}}, Guard{{id: 2}}, Guard{{id: 3}}}}
    gs[1] = Guard{{id: 22}}
    println(id(Array<Guard>{{Guard{{id: 9}}}}[0]))
    consume(Array<Guard>{{Guard{{id: 50}}}})
    let nested = Array<Array<Guard>>{{Array<Guard>{{Guard{{id: 41}}, Guard{{id: 42}}}}, Array<Guard>{{Guard{{id: 43}}}}}}
    var replaced = Array<Guard>{{Guard{{id: 60}}}}
    replaced = Array<Guard>{{Guard{{id: 61}}}}
}}
"
        ),
        "2\n9\n9\n0\n50\n60\n61\n43\n42\n41\n3\n22\n1\n",
    );
}

#[test]
fn dynamic_array_fields_and_empty_arrays_drop_safely() {
    prints(
        &format!(
            "package main\n{GUARD}
type Bag struct {{ Name Guard; Items Array<Guard> }}
func fail() error {{ return error(\"failed\") }}
func gather() (Array<Guard>, error) {{ fail()?\nreturn Array<Guard>{{Guard{{id: 7}}}}, nil }}
func main() {{
    let empty = Array<Guard>{{}}
    let zero, _ = gather()
    let bag = Bag{{Name: Guard{{id: 1}}, Items: Array<Guard>{{Guard{{id: 2}}, Guard{{id: 3}}}}}}
    let name = bag.Name
    println(0)
}}
"
        ),
        "0\n1\n3\n2\n",
    );
}

#[test]
fn invalid_dynamic_array_access_panics() {
    for (setup, expr) in [
        ("var i = 3", "xs[i]"),
        ("var i = -1", "xs[i]"),
        ("var i uint64 = 18446744073709551615", "xs[i]"),
        ("var i = 0", "Array<int>{}[i]"),
    ] {
        panics(
            &main_body(&format!(
                "let xs = Array<int>{{1, 2, 3}}\n{setup}\nprintln(1)\nprintln({expr})"
            )),
            "index out of range",
            "1\n",
        );
    }
    panics(
        &slice_main(
            "",
            "let xs = Array<int>{1, 2, 3}\nvar hi = 4\nprintln(1)\nshow(xs[1:hi], 0)",
        ),
        "slice bounds out of range",
        "1\n",
    );
}

#[test]
fn dynamic_array_construction_and_replacement_clean_up_on_panic() {
    panics(
        &format!(
            "package main\n{GUARD}
func boom() Guard {{ var zero = 0\nreturn Guard{{id: 1 / zero}} }}
func main() {{ let gs = Array<Guard>{{Guard{{id: 1}}, Guard{{id: 2}}, boom()}} }}
"
        ),
        "division by zero",
        "2\n1\n",
    );
    panics(
        &format!(
            "package main\n{PANICKING_GUARD}\nfunc main() {{\nvar gs = Array<Guard>{{Guard{{id: 1}}, Guard{{id: 2}}}}\ngs[0] = Guard{{id: 3}}\n}}\n"
        ),
        "division by zero",
        "1\n2\n3\n",
    );
}

#[test]
fn dynamic_array_drop_loops_hoist_their_allocas() {
    let source = format!(
        "package main\n{GUARD}\nfunc main() {{ let gs = Array<Array<Guard>>{{Array<Guard>{{Guard{{id: 1}}}}}} }}\n"
    );
    let mut sources = SourceMap::new();
    let id = sources.add("test.ore", source).unwrap();
    let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
    for function in ir.split("define ").skip(1) {
        let body = function
            .split_once("br label %bb0")
            .map_or("", |(_, rest)| rest);
        assert!(
            !body.contains("alloca"),
            "alloca after the entry block:\n{function}"
        );
    }
}

#[test]
fn maps_look_up_assign_and_remove() {
    prints(
        &main_body(
            "var scores = map[string]int{\"Ada\": 10, \"Lin\": 0}
            let found, score = scores[\"Ada\"]
            println(found)
            println(score)
            let zero_found, zero = scores[\"Lin\"]
            println(zero_found)
            println(zero)
            let missing, none = scores[\"Bob\"]
            println(missing)
            println(none)
            scores[\"Ada\"] = 30
            scores[\"Bob\"] = 5
            let _, ada = scores[\"Ada\"]
            let _, bob = scores[\"Bob\"]
            println(ada + bob)
            let removed, old = scores.remove(\"Ada\")
            println(removed)
            println(old)
            let again, gone = scores.remove(\"Ada\")
            println(again)
            println(gone)
            var flags = map[bool]rune{true: 'y', false: 'n'}
            let _, yes = flags[1 == 1]
            println(yes)
            var small = map[int8]int{-1: 7}
            let _, seven = small[-1]
            println(seven)
            let empty = map[string]int{}
            let present, _ = empty[\"x\"]
            println(present)",
        ),
        "true\n10\ntrue\n0\nfalse\n0\n35\ntrue\n30\nfalse\n0\ny\n7\nfalse\n",
    );
}

#[test]
fn string_keys_match_by_content() {
    prints(
        "package main
func key(choice bool) string { if choice { return \"same\" }\nreturn \"other\" }
func main() {
    var m = map[string]int{key(true): 1}
    let found, value = m[\"same\"]
    println(found)
    println(value)
}
",
        "true\n1\n",
    );
}

#[test]
fn map_values_are_dropped_exactly_once() {
    prints(
        &format!(
            "package main\n{GUARD}
func take(gs mut map[string]Guard) {{
    let found, g = gs.remove(\"a\")
    if found {{ println(g.id + 100) }}
}}
type Bag struct {{ Items map[int]Guard }}
func gather() (map[int]Guard, error) {{ var zero = 0\nif zero == 0 {{ return map[int]Guard{{}}, error(\"x\") }}\nreturn map[int]Guard{{1: Guard{{id: 9}}}}, nil }}
func lift() (map[int]Guard, error) {{ let m = gather()?\nreturn m, nil }}
func main() {{
    var gs = map[string]Guard{{\"a\": Guard{{id: 1}}}}
    take(gs)
    take(gs)
    gs[\"b\"] = Guard{{id: 2}}
    gs[\"b\"] = Guard{{id: 3}}
    println(0)
    let bag = Bag{{Items: map[int]Guard{{5: Guard{{id: 5}}}}}}
    let nested = map[int]map[int]Guard{{1: map[int]Guard{{6: Guard{{id: 6}}}}}}
    let zero, _ = lift()
}}
"
        ),
        "101\n1\n0\n2\n0\n6\n5\n3\n",
    );
}

#[test]
fn map_literal_duplicates_and_failures_clean_up() {
    panics(
        &format!(
            "package main\n{GUARD}
func key() string {{ return \"a\" }}
func main() {{ let gs = map[string]Guard{{\"a\": Guard{{id: 1}}, key(): Guard{{id: 2}}}} }}
"
        ),
        "duplicate key in map literal",
        "2\n1\n",
    );
    panics(
        &format!(
            "package main\n{GUARD}
func boom() Guard {{ var zero = 0\nreturn Guard{{id: 1 / zero}} }}
func main() {{ let gs = map[int]Guard{{1: Guard{{id: 1}}, 2: boom()}} }}
"
        ),
        "division by zero",
        "1\n",
    );
}

#[test]
fn a_panicking_replaced_map_value_leaves_its_key_absent() {
    panics(
        &format!(
            "package main\n{PANICKING_GUARD}
type Holder struct {{ Items map[string]Guard }}
func (h mut Holder) drop() {{ let found, _ = h.Items.remove(\"k\")\nprintln(found) }}
func main() {{
    var holder = Holder{{Items: map[string]Guard{{\"k\": Guard{{id: 1}}}}}}
    holder.Items[\"k\"] = Guard{{id: 3}}
}}
"
        ),
        "division by zero",
        "1\n3\n0\nfalse\n",
    );
}

fn examples() -> Vec<(String, std::path::PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let mut examples: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("main.ore").is_file())
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                path,
            )
        })
        .collect();
    examples.sort();
    examples
}

#[test]
fn every_example_prints_its_expected_output() {
    let examples = examples();
    assert!(examples.len() >= 15, "{examples:?}");
    for (name, path) in examples {
        let expected = std::fs::read_to_string(path.join("expected-output.txt"))
            .unwrap_or_else(|_| panic!("examples/{name} needs expected-output.txt"));
        let output = zore()
            .arg("run")
            .arg(path.join("main.ore"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "examples/{name}: {}",
            stderr(&output)
        );
        assert_eq!(stdout(&output), expected, "examples/{name}");
    }
}

/// Writes a project into a fresh folder and runs `main.ore` through the command line.
fn run_project(files: &[(&str, &str)]) -> Output {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("zore.toml"), "name = \"app\"\n").unwrap();
    for (path, text) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    zore()
        .arg("run")
        .arg(dir.path().join("main.ore"))
        .output()
        .unwrap()
}

#[test]
fn packages_share_declarations_types_methods_and_cleanup() {
    let output = run_project(&[
        (
            "main.ore",
            "package main

import \"app/store\"
import \"app/store/audit\"

func main() {
    var items = store.New()
    items.Add(\"first\")
    items.Add(\"second\")
    println(items.Count())
    audit.Report(items)
    let guard = audit.Guard{Name: \"guard\"}
    println(guard.Name)
}
",
        ),
        (
            "store/store.ore",
            "package store

type Items struct {
    names Array<string>
}

func New() Items {
    return Items{names: Array<string>{}}
}

func (i mut Items) Add(name string) {
    i.names.push(name)
}

func (i Items) Count() int {
    return i.names.len()
}

func (i Items) At(index int) string {
    return i.names[index]
}
",
        ),
        (
            "store/audit/audit.ore",
            "package audit

import \"app/store\"

type Guard struct {
    Name string
}

func (g mut Guard) drop() {
    println(\"released \" + g.Name)
}

func Report(items store.Items) {
    for var i = 0; i < items.Count(); i += 1 {
        println(items.At(i))
    }
}
",
        ),
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "2\nfirst\nsecond\nguard\nreleased guard\n");
}

#[test]
fn packages_with_equal_names_do_not_collide() {
    let output = run_project(&[
        (
            "main.ore",
            "package main

import \"app/a\"
import \"app/b\"

type T struct { N int }

func helper() int { return 100 }

func main() {
    let x = a.Make()
    let y = b.Make()
    let z = T{N: helper()}
    println(x.N + y.N + z.N)
    println(a.Name() + b.Name())
}
",
        ),
        (
            "a/a.ore",
            "package a\ntype T struct { N int }\nfunc Make() T { return T{N: 1} }\nfunc helper() string { return \"a\" }\nfunc Name() string { return helper() }\n",
        ),
        (
            "b/b.ore",
            "package b\ntype T struct { N int }\nfunc Make() T { return T{N: 20} }\nfunc helper() string { return \"b\" }\nfunc Name() string { return helper() }\n",
        ),
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "121\nab\n");
}

#[test]
fn a_panic_in_an_imported_package_names_its_file() {
    let output = run_project(&[
        (
            "main.ore",
            "package main\nimport \"app/lib\"\nfunc main() {\nprintln(1)\nlib.Fail(0)\n}\n",
        ),
        (
            "lib/lib.ore",
            "package lib\nfunc Fail(n int) int {\nreturn 1 / n\n}\n",
        ),
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "1\n");
    let err = stderr(&output);
    assert!(err.contains("division by zero"), "{err}");
    assert!(err.contains("lib.ore:3:"), "{err}");
}

#[test]
fn closures_and_collections_work_across_packages() {
    let output = run_project(&[
        (
            "main.ore",
            "package main

import \"app/tools\"

func main() {
    let next = tools.Counter()
    println(next())
    println(next())
    let parts = tools.Pairs(3)
    for index, part in parts {
        println(index + part)
    }
}
",
        ),
        (
            "tools/tools.ore",
            "package tools

func Counter() func() int {
    var count = 0
    return func() int {
        count += 1
        return count
    }
}

func Pairs(n int) Array<int> {
    var out = Array<int>{}
    for var i = 0; i < n; i += 1 {
        out.push(i * 10)
    }
    return out
}
",
        ),
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "1\n2\n0\n11\n22\n");
}

#[test]
fn semantic_target_prints_john() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/semantic-target/main.ore"
    ))
    .unwrap();
    prints(&source, "John\n");
}

#[test]
fn hello_prints() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/hello/main.ore"
    ))
    .unwrap();
    prints(&source, "Hello, Zore!\n");
}

#[test]
fn rust_runtime_preserves_empty_nul_and_long_strings() {
    let long = "x".repeat(8192);
    prints(
        &main_body(&format!(
            "var empty string = \"\"\nprintln(empty)\nprintln(empty == \"\")\n\
             println(\"a\\u0000b\")\nprintln(\"{long}\")"
        )),
        &format!("\ntrue\na\0b\n{long}\n"),
    );
}

#[test]
fn methods_call_with_receiver_first() {
    let source = "package main

type Counter struct {
    Label string
    Step int
}

func (c Counter) next(from int) int {
    return from + c.Step
}

func (c own Counter) describe() string {
    return c.Label
}

func main() {
    let counter = Counter{Label: \"steps\", Step: 5}
    println(counter.describe())
    println(counter.next(counter.next(1)))
}
";
    prints(source, "steps\n11\n");
}

#[test]
fn mut_parameters_and_receivers_mutate_the_callers_place() {
    let source = "package main

type Counter struct {
    Label string
    N int
}

type Pair struct {
    Left Counter
    Right Counter
}

func bump(c mut Counter, by int) {
    c.N += by
}

func set(n mut int, value int) {
    n = value
}

func twice(c mut Counter) {
    bump(c, 1)
    bump(c, 1)
}

func (c mut Counter) reset() {
    c.N = 0
}

func (p mut Pair) swapLabels() {
    p.Left.Label = \"right\"
    p.Right.Label = \"left\"
}

func total(p Pair) int {
    return p.Left.N + p.Right.N
}

func main() {
    var c = Counter{Label: \"c\", N: 1}
    bump(c, 4)
    println(c.N)
    twice(c)
    println(c.N)
    c.reset()
    println(c.N)
    var n = 7
    set(n, 42)
    println(n)
    var pair = Pair{Left: Counter{Label: \"l\", N: 1}, Right: Counter{Label: \"r\", N: 2}}
    bump(pair.Right, 10)
    twice(pair.Left)
    pair.swapLabels()
    pair.Left.reset()
    println(pair.Left.Label)
    println(pair.Right.Label)
    println(total(pair))
    println(pair.Left.N)
}
";
    prints(source, "5\n7\n0\n42\nright\nleft\n12\n0\n");
}

#[test]
fn rust_runtime_links_to_an_output_path_with_spaces() {
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("output with spaces");
    std::fs::create_dir(&folder).unwrap();
    let executable = folder.join("hello world");
    build_file(&main_body("println(42)"), &executable).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "42\n");
}

#[test]
fn println_formats_each_printable_type() {
    prints(
        &main_body(
            "println(\"text\")
            println(\"\")
            println(\"café 用户\")
            println(\"a\\tb\")
            println(42)
            println(-7)
            println(-9223372036854775807 - 1)
            println(uint64(18446744073709551615))
            println(int8(-128))
            println(uint8(255))
            println(0x1F)
            println(true)
            println(false)
            println('A')
            println('é')
            println('😀')",
        ),
        "text\n\ncafé 用户\na\tb\n42\n-7\n-9223372036854775808\n18446744073709551615\n-128\n255\n31\ntrue\nfalse\nA\né\n😀\n",
    );
}

#[test]
fn functions_structs_and_control_flow() {
    let source = "package main

type Point struct {
    X int
    Y int
}

type Line struct {
    From Point
    To Point
    Label string
}

func pair() (int, string) {
    return 42, \"answer\"
}

func forward() (int, string) {
    return pair()
}

func fib(n int) int {
    if n < 2 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

func length(l Line) int {
    return l.To.X - l.From.X + l.To.Y - l.From.Y
}

func classify(n int) string {
    if n < 0 {
        return \"negative\"
    } else if n == 0 {
        return \"zero\"
    } else {
        return \"positive\"
    }
}

func main() {
    println(fib(20))
    let n, s = forward()
    println(n)
    println(s)
    var total = 0
    for var i = 0; i < 10; i += 1 {
        if i == 3 { continue }
        if i == 8 { break }
        total += i
    }
    println(total)
    var line = Line{Label: \"diag\", To: Point{X: 4, Y: 6}, From: Point{X: 1, Y: 2}}
    line.To.X = 10
    println(length(line))
    println(line.Label)
    var left = 1
    var right = 2
    left, right = right, left
    println(left * 10 + right)
    var c = 0
    for c < 5 { c += 2 }
    println(c)
    for {
        c -= 1
        if c < 3 { break }
    }
    println(c)
    println(classify(-3))
    println(classify(0))
    println(classify(9))
}
";
    prints(
        source,
        "6765\n42\nanswer\n25\n13\ndiag\n21\n6\n2\nnegative\nzero\npositive\n",
    );
}

#[test]
fn integer_float_and_string_operations() {
    prints(
        &main_body(
            "var bits uint8 = 128
            bits <<= 1
            println(bits)
            var neg = int8(-2)
            println(neg >> 1)
            var u uint8 = 128
            println(u >> 7)
            var one int8 = 64
            println(one << 1)
            println(^uint8(5))
            var a = 17
            var b = 5
            println(a % b)
            println(-a / b)
            println(-a % b)
            println(a & 12 | 2 ^ 1)
            var f = 2.75
            f = f * 2
            println(int64(f))
            println(int64(-f))
            println(int32(uint8(200)))
            let x float32 = 1.5
            println(int(float64(x) * 4))
            var g float32 = 255.9
            println(uint8(g))
            println(\"apple\" < \"banana\")
            var s = \"same\"
            println(s == \"same\")
            println(s != \"same\")
            println(\"\" < s)
            println(\"ab\" < \"a\")
            println('a' < 'b')
            println(1.5 < f)
            println(!true)",
        ),
        "0\n-1\n1\n-128\n250\n2\n-3\n-2\n3\n5\n-5\n200\n6\n255\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue\nfalse\n",
    );
}

#[test]
fn evaluation_order_is_left_to_right() {
    let source = "package main

type Pair struct {
    A int
    B int
}

func tag(label string, value int) int {
    println(label)
    return value
}

func noisy(label string, value bool) bool {
    println(label)
    return value
}

func add(a int, b int) int {
    return a + b
}

func main() {
    let p = Pair{B: tag(\"b\", 2), A: tag(\"a\", 1)}
    println(p.A * 10 + p.B)
    println(add(tag(\"first\", 1), tag(\"second\", 2)))
    println(tag(\"left\", 3) * tag(\"right\", 4))
    println(noisy(\"and-left\", false) && noisy(\"and-right\", true))
    println(noisy(\"or-left\", true) || noisy(\"or-right\", true))
    println(noisy(\"x\", true) && noisy(\"y\", false))
}
";
    prints(
        source,
        "b\na\n12\nfirst\nsecond\n3\nleft\nright\n12\nand-left\nfalse\nor-left\ntrue\nx\ny\nfalse\n",
    );
}

#[test]
fn checked_operations_panic_with_locations() {
    panics(
        &main_body("println(1)\nvar x int8 = 127\nx += 1"),
        "integer overflow at test.ore:6:1",
        "1\n",
    );
    panics(
        &main_body("var x uint8 = 0\nx -= 1"),
        "integer overflow",
        "",
    );
    panics(
        &main_body("var x = 9223372036854775807\nprintln(x * 2)"),
        "integer overflow",
        "",
    );
    panics(
        &main_body("var x int8 = -128\nprintln(-x)"),
        "integer overflow",
        "",
    );
    panics(
        &main_body("var d = 0\nprintln(10 / d)"),
        "division by zero at test.ore:5:9",
        "",
    );
    panics(
        &main_body("var d = 0\nprintln(10 % d)"),
        "division by zero",
        "",
    );
    panics(
        &main_body("var m int8 = -128\nvar n int8 = -1\nprintln(m / n)"),
        "integer overflow",
        "",
    );
    panics(
        &main_body("var s = 64\nprintln(1 << s)"),
        "shift count out of range",
        "",
    );
    panics(
        &main_body("var s int8 = -1\nprintln(1 << s)"),
        "shift count out of range",
        "",
    );
    panics(
        &main_body("var b uint8 = 1\nvar s = 8\nb <<= s"),
        "shift count out of range",
        "",
    );
    panics(
        &main_body("var big = 300\nprintln(uint8(big))"),
        "integer conversion out of range",
        "",
    );
    panics(
        &main_body("var neg = -1\nprintln(uint64(neg))"),
        "integer conversion out of range",
        "",
    );
    panics(
        &main_body("var f = 1e300\nprintln(int64(f))"),
        "float to integer conversion out of range",
        "",
    );
    panics(
        &main_body("var f = -1.0\nprintln(uint8(f))"),
        "float to integer conversion out of range",
        "",
    );
    panics(
        &main_body("var f = 1e300\nlet g = float32(f)"),
        "float conversion out of range",
        "",
    );
}

#[test]
fn closed_standard_output_panics() {
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("spam");
    build_file(&main_body("for {\nprintln(\"spam\")\n}"), &executable).unwrap();
    let mut child = Command::new(&executable)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Closing the read end makes the program's next write fail.
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("failed to write to standard output"));
}

#[test]
fn unsupported_backend_features_are_diagnosed() {
    let mut sources = SourceMap::new();
    let id = sources.add("test.ore", main_body("println(1.5)")).unwrap();
    match emit_llvm(sources.file(id).unwrap()) {
        Err(BuildError::Diagnostics(diagnostics)) => {
            assert!(
                diagnostics.iter().any(|d| d
                    .message()
                    .contains("printing floating-point values is not supported")),
                "{diagnostics:?}"
            );
        }
        other => panic!("expected diagnostics, got {other:?}"),
    }
    prints(
        &main_body("var s = \"a\"\ns += \"b\"\nprintln(s + \"c\")\nprintln(\"con\" + \"cat\")"),
        "abc\nconcat\n",
    );
}

#[test]
fn shared_array_parameters_read_the_callers_storage() {
    prints(
        "package main
type Grid struct { cells [int; 3]\n label string }
func (g Grid) total() int { return g.cells[0] + g.cells[1] + g.cells[2] }
func sum(xs [int; 3]) int { return xs[0] + xs[1] + xs[2] }
func label(g Grid) string { return g.label }
func bump(xs mut [int; 3]) { xs[1] += 10 }
func main() {
    var xs = [int; 3]{1, 2, 3}
    println(sum(xs))
    bump(xs)
    println(sum(xs))
    println(sum([int; 3]{4, 5, 6}))
    let g = Grid{cells: xs, label: \"grid\"}
    println(g.total())
    println(label(g))
    println(xs[1])
}
",
        "6\n16\n15\n16\ngrid\n12\n",
    );
}

/// Prints the first `count` elements of a view, one per line.
const SHOW: &str = "func show(items []int, count int) {
    for var i = 0; i < count; i += 1 {
        println(items[i])
    }
}";

/// `SHOW` and other declarations, then `stmts` as `main`'s body.
fn slice_main(decls: &str, stmts: &str) -> String {
    format!("package main\n\n{SHOW}\n{decls}\n\nfunc main() {{\n{stmts}\n}}\n")
}

#[test]
fn slices_view_the_requested_range() {
    prints(
        &slice_main(
            "",
            "let data = [int; 4]{10, 20, 30, 40}
            show(data[1:3], 2)
            show(data[:2], 2)
            show(data[2:], 2)
            show(data[:], 4)
            show(data[4:4], 0)
            show(data[0:0], 0)
            println(data[1:][0])",
        ),
        "20\n30\n10\n20\n30\n40\n10\n20\n30\n40\n20\n",
    );
}

#[test]
fn writes_through_mutable_views_reach_the_backing_array() {
    prints(
        &slice_main(
            "func edit(items mut []int) { items[0] = items[0] * 10\nitems[1] += 1 }",
            "var data = [int; 3]{1, 2, 3}
            edit(data[:])
            var part mut []int = data[1:]
            edit(part)
            show(data[:], 3)",
        ),
        "10\n30\n4\n",
    );
}

#[test]
fn views_flow_through_calls_structs_and_subslices() {
    prints(
        &slice_main(
            "type View struct { Items []int\n Count int }
            func sum(items []int, count int) int {
                var total = 0
                for var i = 0; i < count; i += 1 { total += items[i] }
                return total
            }
            func rest(items []int) []int { return items[1:] }
            func window(a [int; 4]) []int { return a[1:3] }
            func middle(a mut [int; 4]) mut []int { return a[1:3] }
            func wrap(items []int, count int) View { return View{Items: items, Count: count} }",
            "var data = [int; 4]{1, 2, 3, 4}
            println(sum(data[:], 4))
            let r = rest(rest(data[:]))
            println(r[0])
            let w = window(data)
            println(w[1])
            let v = wrap(data[2:], 2)
            println(sum(v.Items, v.Count))
            var m mut []int = middle(data)
            m[0] = 7
            println(data[1])
            var s []int = data[:]
            s = s[3:]
            println(s[0])",
        ),
        "10\n3\n3\n7\n7\n4\n",
    );
}

#[test]
fn replacing_a_move_element_through_a_view_drops_the_old_one() {
    prints(
        "package main
type Res struct { id int }
func (r mut Res) drop() { println(r.id) }
func replace(rs mut []Res) { rs[0] = Res{id: 9} }
func id(r Res) int { return r.id }
func first(rs []Res) int { return id(rs[0]) }
func main() {
    var rs = [Res; 2]{Res{id: 1}, Res{id: 2}}
    replace(rs[:])
    println(first(rs[:]))
}
",
        "1\n9\n2\n9\n",
    );
}

#[test]
fn invalid_runtime_slice_bounds_panic() {
    for (setup, expr) in [
        ("var hi = 4", "data[1:hi]"),
        ("var lo = 2\nvar hi = 1", "data[lo:hi]"),
        ("var lo = -1", "data[lo:]"),
        ("var hi uint64 = 18446744073709551615", "data[:hi]"),
        ("var lo = 3", "data[1:][lo:]"),
    ] {
        panics(
            &slice_main(
                "",
                &format!("let data = [int; 3]{{1, 2, 3}}\n{setup}\nprintln(1)\nshow({expr}, 0)"),
            ),
            "slice bounds out of range",
            "1\n",
        );
    }
}

#[test]
fn indexing_past_a_view_panics() {
    panics(
        &slice_main("", "let data = [int; 3]{1, 2, 3}\nshow(data[1:], 3)"),
        "index out of range",
        "2\n3\n",
    );
    panics(
        &slice_main(
            "func fail() error { return error(\"failed\") }
            func view(items []int) ([]int, error) { fail()?\nreturn items, nil }",
            "let data = [int; 1]{1}\nlet s, _ = view(data[:])\nprintln(s[0])",
        ),
        "index out of range",
        "",
    );
}

#[test]
fn only_valid_executables_are_built() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("x");
    match build_file("package tools\nfunc helper() {}\n", &out) {
        Err(BuildError::NotExecutable(message)) => assert!(message.contains("not executable")),
        other => panic!("{other:?}"),
    }
    match build_file(&main_body("let x uint8 = 300"), &out) {
        Err(BuildError::Diagnostics(d)) => assert!(d[0].message().contains("does not fit")),
        other => panic!("{other:?}"),
    }
    assert!(!out.exists());
}

fn zore() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zore"));
    command.env("ZORE_CHECK_LEAKS", "1");
    command
}

#[test]
fn cli_build_and_run() {
    let dir = TempDir::new().unwrap();
    // A program is its whole folder, so each one gets its own.
    let program = |name: &str, text: String| {
        let folder = dir.path().join(name);
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join(format!("{name}.ore"));
        std::fs::write(&source, text).unwrap();
        (folder, source)
    };
    let (greet_dir, source) = program("greet", main_body("println(\"hi\")"));
    let output = zore()
        .arg("build")
        .arg(&source)
        .current_dir(&greet_dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    let built = Command::new(greet_dir.join("greet")).output().unwrap();
    assert_eq!(stdout(&built), "hi\n");

    let output = zore().arg("run").arg(&source).output().unwrap();
    assert_eq!(
        (output.status.code(), stdout(&output)),
        (Some(0), "hi\n".into())
    );

    let (_, panicking) = program(
        "boom",
        main_body("println(\"before\")\nvar d = 0\nprintln(1 / d)"),
    );
    let output = zore().arg("run").arg(&panicking).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "before\n");
    assert!(stderr(&output).contains("division by zero"));

    let (invalid_dir, invalid) = program("bad", main_body("println(missing)"));
    for command in ["build", "run"] {
        let output = zore()
            .arg(command)
            .arg(&invalid)
            .current_dir(&invalid_dir)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let err = stderr(&output);
        assert!(err.contains("cannot find `missing`"), "{err}");
        assert!(err.contains("zore: build failed with 1 error"), "{err}");
    }
    assert!(!invalid_dir.join("bad").exists());
}

#[test]
fn missing_toolchain_is_reported() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("main.ore");
    let mut file = std::fs::File::create(&source).unwrap();
    file.write_all(main_body("println(1)").as_bytes()).unwrap();
    let output = zore()
        .arg("build")
        .arg(&source)
        .current_dir(dir.path())
        .env("ZORE_CC", "/nonexistent/zore-test-cc")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("requires clang with LLVM 15 or newer"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn missing_rust_toolchain_is_reported_but_check_stays_independent() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("main.ore");
    std::fs::write(&source, main_body("println(1)")).unwrap();
    let output = zore()
        .arg("build")
        .arg(&source)
        .current_dir(dir.path())
        .env("ZORE_RUSTC", dir.path().join("missing-rustc"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("requires rustc 1.98 or newer"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("main").exists());

    let output = zore()
        .arg("check")
        .arg(&source)
        .env("ZORE_RUSTC", dir.path().join("missing-rustc"))
        .env("ZORE_CC", dir.path().join("missing-clang"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

#[test]
fn emitted_ir_is_deterministic_and_names_the_entry() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/semantic-target/main.ore"
    ))
    .unwrap();
    let emit = || {
        let mut sources = SourceMap::new();
        let id = sources.add("main.ore", source.clone()).unwrap();
        emit_llvm(sources.file(id).unwrap()).unwrap()
    };
    let ir = emit();
    assert_eq!(ir, emit());
    assert!(
        ir.contains("%\"main.User\" = type { { ptr, i64 } }"),
        "{ir}"
    );
    assert!(ir.contains("define void @\"main.greet\""), "{ir}");
    assert!(ir.contains("define void @zore_entry()"), "{ir}");
    assert!(ir.contains("c\"John\""), "{ir}");
}

#[test]
fn closures_read_and_write_their_captures() {
    prints(
        &main_body(
            "let name = \"zore\"
            let greet = func() { println(name) }
            greet()
            var count = 0
            let bump = func() { count += 1 }
            bump()
            bump()
            println(count)
            var total = 0
            let outer = func() {
                let inner = func(v int) { total += v }
                inner(2)
                inner(3)
            }
            outer()
            println(total)
            var data = [int; 3]{1, 2, 3}
            let set = func(i int, v int) { data[i] = v }
            set(1, 20)
            println(data[0] + data[1] + data[2])
            println(func(x int) int { return x * x }(7))",
        ),
        "zore\n2\n5\n24\n49\n",
    );
}

#[test]
fn closures_pass_through_function_typed_parameters() {
    prints(
        "package main

func each(n int, f func(int)) {
    for var i = 0; i < n; i += 1 {
        f(i)
    }
}

func compose(f func(int) int, g func(int) int, x int) int {
    let both = func(v int) int { return g(f(v)) }
    return both(x)
}

func edit(x mut int, f func(mut int)) {
    f(x)
}

func main() {
    var sum = 0
    each(5, func(i int) { sum += i })
    println(sum)
    println(compose(func(v int) int { return v + 1 }, func(v int) int { return v * 10 }, 4))
    var n = 3
    edit(n, func(x mut int) { x *= 7 })
    println(n)
}
",
        "10\n50\n21\n",
    );
}

#[test]
fn closures_return_results_and_propagate_errors() {
    prints(
        "package main

func parse(text string) (int, error) {
    if text == \"\" {
        return 0, error(\"empty\")
    }
    return 41, nil
}

func main() {
    let checked = func(text string) (int, error) {
        let v = parse(text)?
        return v + 1, nil
    }
    let a, e1 = checked(\"x\")
    println(a)
    println(e1 == nil)
    let b, e2 = checked(\"\")
    println(b)
    println(e2 == error(\"empty\"))
    let pair = func() (string, bool) { return \"pair\", true }
    let s, ok = pair()
    println(s)
    println(ok)
}
",
        "42\ntrue\n0\ntrue\npair\ntrue\n",
    );
}

#[test]
fn captured_move_values_keep_their_owner_cleanup() {
    prints(
        "package main

type Res struct {
    name string
}

func (r mut Res) drop() {
    println(r.name)
}

func show(r Res) {
    println(\"show \" + \"it\")
}

func outer(r Res, n mut int) {
    let f = func() {
        show(r)
        n += 100
    }
    f()
}

func main() {
    let kept = Res{name: \"kept\"}
    let use = func() { show(kept) }
    use()
    use()
    var slot = Res{name: \"first\"}
    let replace = func() { slot = Res{name: \"second\"} }
    replace()
    let consume = func(r own Res) { println(\"consumed\") }
    consume(Res{name: \"owned by closure\"})
    var n = 1
    outer(Res{name: \"temporary\"}, n)
    println(n)
}
",
        "show it\nshow it\nfirst\nconsumed\nowned by closure\nshow it\ntemporary\n101\nsecond\nkept\n",
    );
}

#[test]
fn a_panic_in_a_closure_unwinds_through_its_caller() {
    panics(
        "package main

type Guard struct {
    name string
}

func (g mut Guard) drop() {
    println(g.name)
}

func run(f func(int)) {
    let guard = Guard{name: \"run guard\"}
    f(5)
}

func main() {
    let outer = Guard{name: \"main guard\"}
    let data = [int; 2]{1, 2}
    run(func(i int) {
        let inner = Guard{name: \"closure guard\"}
        println(data[i])
    })
    println(\"unreachable\")
}
",
        "index out of range",
        "closure guard\nrun guard\nmain guard\n",
    );
}

#[test]
fn cloned_collections_are_independent_of_their_source() {
    prints(
        "package main

type Point struct {
    x int
    y int
}

func main() {
    var point = Point{x: 1, y: 2}
    let point2 = clone(point)
    point.x = 10
    println(point2.x + point2.y)

    var list = Array<int>{1, 2, 3}
    let list2 = clone(list)
    list[0] = 100
    println(list2[0] + list2[1] + list2[2])

    var grid = [int; 3]{4, 5, 6}
    let grid2 = clone(grid)
    grid[1] = 50
    println(grid2[1])

    var scores = map[string]int{\"a\": 1, \"b\": 2}
    let copy = clone(scores)
    scores[\"a\"] = 99
    let found, value = copy[\"a\"]
    println(value)
    let removed, old = scores.remove(\"b\")
    let still, kept = copy[\"b\"]
    println(kept)

    var nested = Array<Array<int>>{Array<int>{1, 2}, Array<int>{3}}
    let nested2 = clone(nested)
    nested[0][1] = 20
    println(nested2[0][1] + nested2[1][0])

    let empty = Array<int>{}
    let empty2 = clone(empty)
    let none = map[int]int{}
    let none2 = clone(none)
    let missing, zero = none2[1]
    println(missing)
}
",
        "3\n6\n5\n1\n2\n5\nfalse\n",
    );
}

#[test]
fn custom_clone_runs_instead_of_the_structural_default() {
    prints(
        "package main

type Counter struct {
    n int
}

func (c Counter) clone() Counter {
    println(\"custom\")
    return Counter{n: c.n + 1}
}

type Holder struct {
    c Counter
    label int
}

func main() {
    let counter = Counter{n: 1}
    println(clone(counter).n)
    println(counter.clone().n)
    println(counter.n)
    let holder = Holder{c: counter, label: 7}
    println(clone(holder).c.n)
}
",
        "custom\n2\ncustom\n2\n1\n1\n",
    );
}

#[test]
fn clones_of_resources_drop_independently() {
    prints(
        "package main

type Res struct {
    id int
}

func (r mut Res) drop() {
    println(r.id)
}

func (r Res) clone() Res {
    return Res{id: r.id + 10}
}

type Pair struct {
    a Res
    b Res
}

func main() {
    let pair = Pair{a: Res{id: 1}, b: Res{id: 2}}
    let pair2 = clone(pair)
    let list = Array<Res>{Res{id: 3}, Res{id: 4}}
    let list2 = clone(list)
    let fixed = [Res; 2]{Res{id: 5}, Res{id: 6}}
    let fixed2 = clone(fixed)
    let table = map[int]Res{1: Res{id: 7}}
    let table2 = clone(table)
    let single = clone(Res{id: 8})
    println(0)
}
",
        "8\n0\n18\n17\n7\n16\n15\n6\n5\n14\n13\n4\n3\n12\n11\n2\n1\n",
    );
}

#[test]
fn clone_is_usable_inside_closures_and_as_a_statement() {
    prints(
        "package main

func main() {
    let list = Array<int>{7, 8}
    let first = func() int {
        let copy = clone(list)
        return copy[0]
    }
    println(first())
    clone(list)
    println(list[1])
}
",
        "7\n8\n",
    );
}

const FAILING_CLONE: &str = "type Res struct {
    id int
}

func (r mut Res) drop() {
    println(r.id)
}

func (r Res) clone() Res {
    var zero = 0
    if r.id == 3 {
        println(1 / zero)
    }
    return Res{id: r.id + 10}
}

type Trio struct {
    a Res
    b Res
    c Res
}
";

fn failing_clone(body: &str, stdout_before: &str) {
    panics(
        &format!(
            "package main\n\n{FAILING_CLONE}\nfunc main() {{\n{body}\nprintln(\"unreachable\")\n}}\n"
        ),
        "division by zero",
        stdout_before,
    );
}

#[test]
fn a_panicking_clone_drops_the_parts_already_cloned() {
    failing_clone(
        "let items = Array<Res>{Res{id: 1}, Res{id: 2}, Res{id: 3}}\nlet copy = clone(items)",
        "12\n11\n3\n2\n1\n",
    );
    failing_clone(
        "let items = [Res; 3]{Res{id: 1}, Res{id: 2}, Res{id: 3}}\nlet copy = clone(items)",
        "12\n11\n3\n2\n1\n",
    );
    failing_clone(
        "let trio = Trio{a: Res{id: 1}, b: Res{id: 2}, c: Res{id: 3}}\nlet copy = clone(trio)",
        "12\n11\n3\n2\n1\n",
    );
    failing_clone(
        "let items = map[int]Res{1: Res{id: 1}, 2: Res{id: 2}, 3: Res{id: 3}}\nlet copy = clone(items)",
        "11\n12\n1\n2\n3\n",
    );
}

#[test]
fn a_panicking_clone_in_a_loop_cleans_up_every_iteration() {
    failing_clone(
        "for var i = 0; i < 2; i += 1 {\nlet items = Array<Res>{Res{id: 1}, Res{id: 2}}\nlet copy = clone(items)\n}\nlet last = Array<Res>{Res{id: 3}}\nlet bad = clone(last)",
        "12\n11\n2\n1\n12\n11\n2\n1\n3\n",
    );
}

#[test]
fn mutable_views_inside_structs_and_arrays_write_through() {
    prints(
        "package main

type Window struct {
    items mut []int
    label int
}

func fill(w mut Window, value int) {
    w.items[0] = value
}

func first(w own Window) int {
    return w.items[0]
}

func narrow(data mut []int) Window {
    return Window{items: data[1:], label: 2}
}

func main() {
    var data = [int; 3]{1, 2, 3}
    var w = Window{items: data[:], label: 1}
    w.items[1] = 20
    fill(w, 10)
    println(first(w))
    let n = narrow(data[:])
    n.items[0] = 7
    println(data[0] + data[1] + data[2])

    var a = [int; 2]{1, 2}
    var b = [int; 2]{3, 4}
    var rows = [mut []int; 2]{a[:], b[:]}
    rows[0][1] = 20
    rows[1][0] = 30
    println(a[0] + a[1] + b[0])
}
",
        "10\n20\n51\n",
    );
}

#[test]
fn views_in_collections_and_slice_elements_write_through() {
    prints(
        "package main

func put(rows mut [][]int, view []int) {
    rows[1] = view
}

func main() {
    var a = [int; 2]{1, 2}
    var b = [int; 2]{3, 4}
    var views = Array<mut []int>{a[:], b[:]}
    var first = views[0]
    first[0] = 10
    var all mut []mut []int = views[:]
    all[1][1] = 40
    var m = map[string]mut []int{\"a\": a[:]}
    var found, taken = m.remove(\"a\")
    if found {
        taken[1] = 20
    }
    println(a[0] + a[1] + b[1])

    var rows = [[]int; 2]{a[:], a[:]}
    var slots mut [][]int = rows[:]
    slots[0] = b[:]
    put(rows[:], b[1:])
    println(rows[0][0] + rows[1][0])
}
",
        "70\n43\n",
    );
}

#[test]
fn a_drop_reads_the_views_it_holds_on_every_exit() {
    prints(
        "package main

type Watch struct {
    items []int
    id int
}

func (w mut Watch) drop() {
    println(w.items[0])
}

func keep(w own Watch) {
    println(w.id)
}

func main() {
    var data = [int; 2]{1, 2}
    let w = Watch{items: data[:], id: 7}
    println(w.id)
    var a = [int; 1]{10}
    var b = [int; 1]{20}
    var replaced = Watch{items: a[:], id: 1}
    replaced = Watch{items: b[:], id: 2}
    a[0] = 11
    let moved = Watch{items: a[:], id: 3}
    keep(moved)
    a[0] = 12
    println(a[0])
}
",
        "7\n10\n3\n11\n12\n20\n1\n",
    );
    panics(
        "package main

type Watch struct {
    items []int
}

func (w mut Watch) drop() {
    println(w.items[0] + w.items[1])
}

func boom(n int) int {
    var zero = 0
    return n / zero
}

func main() {
    var list = Array<int>{1, 2}
    var other = Array<int>{30, 40}
    let w = Watch{items: list[:]}
    var heap = Array<Watch>{Watch{items: other[:]}}
    println(boom(1))
}
",
        "division by zero",
        "70\n3\n",
    );
}

#[test]
fn views_flow_through_mut_parameters_captures_and_closure_results() {
    prints(
        "package main

type View struct {
    Items []int
}

func fill(out mut View, items []int) {
    out.Items = items
}

func apply(f func([]int) []int, s []int) []int {
    return f(s)
}

func main() {
    var data = [int; 3]{1, 2, 3}
    var other = [int; 1]{0}
    var view = View{Items: other[:]}
    fill(view, data[:])
    println(view.Items[2])

    let tail = func(items []int) []int { return items[1:] }
    println(apply(tail, data[:])[0])

    var arr = [int; 2]{7, 8}
    let whole = func() []int { return arr[:] }
    println(whole()[1])

    var pointed = other[:]
    let point = func() { pointed = arr[:] }
    point()
    println(pointed[0])
}
",
        "3\n2\n8\n7\n",
    );
}

#[test]
fn collections_grow_shrink_and_report_their_length() {
    prints(
        "package main

type Res struct { id int }

func (r mut Res) drop() { println(r.id) }

func main() {
    var names = Array<string>{}
    for var i = 0; i < 20; i += 1 {
        names.push(\"x\")
    }
    names.push(\"last\")
    println(names.len())
    let found, last = names.pop()
    println(found)
    println(last)
    println(names.len())
    var empty = Array<int>{}
    let none, zero = empty.pop()
    println(none)
    println(zero)
    let fixed = [int; 3]{1, 2, 3}
    println(fixed.len() + fixed[1:].len())
    var scores = map[string]int{\"a\": 1, \"b\": 2}
    println(scores.len())
    var held = Array<Res>{Res{id: 1}}
    held.push(Res{id: 2})
    held.push(Res{id: 3})
    let ok, popped = held.pop()
    println(ok)
    println(popped.id + 10)
}
",
        "21\ntrue\nlast\n20\nfalse\n0\n5\n2\ntrue\n13\n3\n2\n1\n",
    );
}

#[test]
fn collection_loops_visit_every_element_and_clean_up() {
    prints(
        "package main

type Res struct { id int }

func (r mut Res) drop() { println(r.id) }

func make() Array<Res> {
    var out = Array<Res>{}
    for var i = 1; i <= 3; i += 1 {
        out.push(Res{id: i})
    }
    return out
}

func find(xs Array<Res>, id int) int {
    for i, r in xs {
        if r.id == id {
            return i
        }
    }
    return -1
}

func main() {
    for r in make() {
        if r.id == 2 {
            break
        }
        println(r.id + 10)
    }
    var xs = make()
    println(find(xs, 3))
    var sum = 0
    for i, r in xs {
        if i == 0 {
            continue
        }
        sum += r.id
    }
    println(sum)
    for x in [int; 3]{4, 5, 6}[1:] {
        println(x)
    }
    var m = map[string]int{\"a\": 1, \"b\": 2, \"c\": 3}
    var total = 0
    for key, value in m {
        if key != \"b\" {
            total += value
        }
    }
    println(total)
    var empty = map[bool]int{}
    for _, _ in empty {
        println(99)
    }
    var flags = map[bool]int{true: 1}
    for flag, _ in flags {
        println(flag)
    }
}
",
        "11\n3\n2\n1\n2\n5\n5\n6\n4\ntrue\n3\n2\n1\n",
    );
}

#[test]
fn a_panic_inside_a_loop_destroys_the_held_collection() {
    panics(
        "package main

type Res struct { id int }

func (r mut Res) drop() { println(r.id) }

func make() Array<Res> {
    return Array<Res>{Res{id: 1}, Res{id: 2}}
}

func main() {
    var zero = 0
    for r in make() {
        println(r.id / zero)
    }
}
",
        "division by zero",
        "2\n1\n",
    );
}

#[test]
fn owning_closures_keep_their_state_and_free_it() {
    prints(
        "package main

type Job struct { Id int }

func (j mut Job) drop() { println(j.Id) }

func consume(j own Job) { println(j.Id + 1000) }

func counter() func() int {
    var count = 0
    return func() int {
        count += 1
        return count
    }
}

type Handler struct {
    run func(int) int
}

func scaled(scale int) Handler {
    return Handler{run: func(x int) int { return x * scale }}
}

func holding(job own Job) func() int {
    return func() int { return job.Id }
}

func main() {
    let next = counter()
    println(next())
    println(next())
    let other = counter()
    println(other())
    var h = scaled(3)
    println((h.run)(5))
    var hold = holding(Job{Id: 1})
    println(hold())
    hold = holding(Job{Id: 2})
    println(hold())
    var seen = 10
    var later = Array<func() int>{}
    later.push(func() int { return seen })
    seen = 20
    println((later[0])() + seen)
    let job = Job{Id: 3}
    let finish = func() { consume(job) }
    finish()
    let kept = Job{Id: 4}
    let never = func() { consume(kept) }
    println(0)
}
",
        "1\n2\n1\n15\n1\n1\n2\n30\n1003\n3\n0\n4\n2\n",
    );
}

#[test]
fn strings_measure_index_slice_loop_and_concatenate_by_bytes() {
    prints(
        "package main

func repeat(piece string, count int) string {
    var out = \"\"
    for var i = 0; i < count; i += 1 {
        out += piece
    }
    return out
}

func main() {
    let word = \"héllo\"
    println(word.len())
    println(word[0])
    println(word[1:3])
    println(word[:1] + word[3:])
    println(word[1:1] == \"\")
    for i, ch in word {
        println(i)
        println(ch)
    }
    var letters = 0
    for _ in word {
        letters += 1
    }
    println(letters)
    println(string('é') + string('x'))
    let built = repeat(\"ab\", 3) + \"!\"
    println(built)
    println(built == \"ababab!\")
    println(built < \"b\")
    println(repeat(\"é\", 4)[2:6])
    var counts = map[string]int{}
    counts[repeat(\"k\", 2)] = 5
    let found, value = counts[\"kk\"]
    println(found)
    println(value)
    println(repeat(\"\", 5).len())
}
",
        "6\n104\né\nhllo\ntrue\n0\nh\n1\né\n3\nl\n4\nl\n5\no\n5\néx\nababab!\ntrue\ntrue\néé\ntrue\n5\n0\n",
    );
}

#[test]
fn string_operations_panic_on_bad_indexes_and_bounds() {
    for (body, message) in [
        (
            "let s = \"ab\"\n    let n = 2\n    println(s[n])",
            "index out of range",
        ),
        (
            "let s = \"ab\"\n    var n = 3\n    println(s[n:])",
            "slice bounds out of range",
        ),
        (
            "let s = \"ab\"\n    var n = 1\n    println(s[n:0])",
            "slice bounds out of range",
        ),
        (
            "let s = \"é\"\n    var n = 1\n    println(s[n:])",
            "string slice not on a character boundary",
        ),
        (
            "let s = \"aé\"\n    var n = 2\n    println(s[:n])",
            "string slice not on a character boundary",
        ),
    ] {
        panics(
            &format!("package main\n\nfunc main() {{\n    println(1)\n    {body}\n}}\n"),
            message,
            "1\n",
        );
    }
}

#[test]
fn every_standard_function_computes_its_documented_result() {
    prints(
        "package main

import \"zore/strconv\"
import \"zore/strings\"

func show(parts Array<string>) {
    var joined = \"\"
    for index, part in parts {
        if index > 0 {
            joined += \"|\"
        }
        joined += \"<\" + part + \">\"
    }
    println(joined)
}

func main() {
    println(strings.Contains(\"hello\", \"ell\"))
    println(strings.Contains(\"hello\", \"\"))
    println(strings.Contains(\"hello\", \"xyz\"))
    println(strings.HasPrefix(\"hello\", \"he\"))
    println(strings.HasPrefix(\"hello\", \"lo\"))
    println(strings.HasSuffix(\"hello\", \"lo\"))
    println(strings.HasSuffix(\"hello\", \"he\"))
    println(strings.Index(\"héllo\", \"l\"))
    println(strings.Index(\"hello\", \"\"))
    println(strings.Index(\"hello\", \"z\"))
    println(strings.Upper(\"héllo ß\"))
    println(strings.Lower(\"HÉLLO\"))
    println(\"[\" + strings.TrimSpace(\"  \\t a b \\n\") + \"]\")
    println(\"[\" + strings.TrimSpace(\"   \") + \"]\")
    println(strings.Repeat(\"ab\", 3))
    println(strings.Repeat(\"ab\", 0).len())
    println(strings.Replace(\"banana\", \"an\", \"AN\"))
    println(strings.Replace(\"ab\", \"\", \"-\"))
    show(strings.Split(\"a,b,c\", \",\"))
    show(strings.Split(\",a,\", \",\"))
    show(strings.Split(\"\", \",\"))
    show(strings.Split(\"héy\", \"\"))
    show(strings.Split(\"\", \"\"))
    show(strings.Split(\"a--b\", \"--\"))
    let words = [string; 3]{\"x\", \"y\", \"z\"}
    println(strings.Join(words[:], \", \"))
    println(strings.Join(words[:1], \", \"))
    println(strings.Join(words[:0], \", \").len())
    println(strconv.Itoa(0))
    println(strconv.Itoa(-9223372036854775807 - 1))
    for text in Array<string>{\"42\", \"-7\", \"+5\", \"\", \"-\", \"4x\", \"9223372036854775808\"} {
        let value, err = strconv.Atoi(text)
        println(value)
        println(err == nil)
    }
    println(strconv.FormatBool(true) + strconv.FormatBool(false))
    for text in Array<string>{\"true\", \"false\", \"True\"} {
        let flag, err = strconv.ParseBool(text)
        println(flag)
        println(err == nil)
    }
    let _, failed = strconv.Atoi(\"nope\")
    println(failed == error(\"strconv.Atoi: invalid syntax\"))
    let _, range = strconv.Atoi(\"99999999999999999999\")
    println(range == error(\"strconv.Atoi: value out of range\"))
}
",
        "true\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\n3\n0\n-1\nHÉLLO SS\nhéllo\n[a b]\n[]\nababab\n0\nbANANa\n-a-b-\n\
<a>|<b>|<c>\n<>|<a>|<>\n<>\n<h>|<é>|<y>\n\n<a>|<b>\nx, y, z\nx\n0\n0\n-9223372036854775808\n\
42\ntrue\n-7\ntrue\n5\ntrue\n0\nfalse\n0\nfalse\n0\nfalse\n0\nfalse\n\
truefalse\ntrue\ntrue\nfalse\ntrue\nfalse\nfalse\ntrue\ntrue\n",
    );
}

#[test]
fn standard_functions_that_panic_clean_up_and_report() {
    for (call, message) in [
        (
            "strings.Repeat(\"x\", -1)",
            "strings.Repeat: negative count",
        ),
        (
            "strings.Repeat(\"xx\", 9223372036854775807)",
            "strings.Repeat: result too large",
        ),
    ] {
        panics(
            &format!(
                "package main

import \"zore/strings\"

type Guard struct {{ Name string }}

func (g mut Guard) drop() {{ println(g.Name) }}

func main() {{
    let guard = Guard{{Name: \"guard\"}}
    println(\"before\")
    println({call})
}}
"
            ),
            message,
            "before\nguard\n",
        );
    }
}

#[test]
fn building_text_in_a_loop_does_not_copy_it_every_round() {
    prints(
        "package main

func main() {
    var text = \"\"
    for var i = 0; i < 200000; i += 1 {
        text += \"ab\"
    }
    println(text.len())
    let early = text[:4]
    text += \"!\"
    println(text[text.len() - 1])
    println(early)
    var other = early + \"zz\"
    other += \"yy\"
    println(other)
    println(text.len())
}
",
        "400000\n33\nabab\nababzzyy\n400001\n",
    );
}

#[test]
fn discarded_text_is_freed_when_it_is_overwritten_or_goes_out_of_scope() {
    prints(
        "package main

import \"zore/strconv\"

func main() {
    var kept = \"\"
    for var i = 0; i < 2000; i += 1 {
        let scratch = \"line \" + strconv.Itoa(i)
        kept = scratch + \"!\"
        kept = kept[1:]
    }
    println(kept)
}
",
        "ine 1999!\n",
    );
}

#[test]
fn text_inside_structs_and_fixed_arrays_is_shared_by_copies() {
    prints(
        "package main

type Person struct {
    Name string
    Tags [string; 2]
}

func describe(p Person) string {
    return p.Name + \":\" + p.Tags[0] + p.Tags[1]
}

func rename(p mut Person, name string) {
    p.Name = name + \"!\"
}

func main() {
    var first = Person{Name: \"a\" + \"b\", Tags: [string; 2]{\"x\" + \"1\", \"y\" + \"2\"}}
    let second = first
    rename(first, \"z\" + \"z\")
    println(describe(first))
    println(describe(second))
    first.Tags[1] = second.Tags[0] + second.Tags[1]
    println(describe(first))
}
",
        "zz!:x1y2\nab:x1y2\nzz!:x1x1y2\n",
    );
}

#[test]
fn text_in_dynamic_arrays_and_maps_is_released_with_its_owner() {
    prints(
        "package main

import \"zore/strconv\"

func main() {
    var names = Array<string>{}
    for var i = 0; i < 20; i += 1 {
        names.push(\"n\" + strconv.Itoa(i))
    }
    let found, last = names.pop()
    println(found)
    println(last)
    let grid = Array<Array<string>>{Array<string>{\"a\" + \"b\", \"c\"}, Array<string>{\"d\" + \"e\"}}
    println(grid[0][0] + grid[1][0])
    let twin = clone(grid)

    var scores = map[string]string{}
    for var i = 0; i < 30; i += 1 {
        scores[\"k\" + strconv.Itoa(i % 4)] = \"v\" + strconv.Itoa(i)
    }
    let seen, value = scores[\"k1\"]
    println(seen)
    println(value)
    let removed, old = scores.remove(\"k2\")
    println(removed)
    println(old)
    let copy = clone(scores)
    scores[\"k0\"] = \"changed\"
    var total = 0
    for key, text in copy {
        total += key.len() + text.len()
    }
    println(total)
    println(twin[0][1])
}
",
        "true\nn19\nabde\ntrue\nv29\ntrue\nv26\n15\nc\n",
    );
}

#[test]
fn closures_keep_the_text_they_capture_until_they_are_dropped() {
    prints(
        "package main

import \"zore/strconv\"

func counter(prefix string) func() string {
    var n = 0
    return func() string {
        n += 1
        return prefix + strconv.Itoa(n)
    }
}

func main() {
    let next = counter(\"c\" + \"-\")
    println(next())
    println(next())
    var message = \"hel\" + \"lo\"
    let show = func() string { return message + \"!\" }
    println(show())
}
",
        "c-1\nc-2\nhello!\n",
    );
}

#[test]
fn error_messages_built_from_text_are_released() {
    prints(
        "package main

import \"zore/strconv\"

func parse(text string) (int, error) {
    let value, err = strconv.Atoi(text)
    if err != nil {
        return 0, err
    }
    return value, nil
}

func check(n int) (int, error) {
    if n > 2 {
        return 0, error(\"too big: \" + strconv.Itoa(n))
    }
    return n, nil
}

func chain(n int) (int, error) {
    let value = check(n)?
    return value + 1, nil
}

func main() {
    for var i = 0; i < 5; i += 1 {
        let value, err = chain(i)
        if err != nil {
            println(err == error(\"too big: 3\"))
        } else {
            println(value)
        }
    }
    let _, ignored = parse(\"x\" + \"y\")
    _ = ignored
    let _, other = check(9)
    _ = other
}
",
        "1\n2\n3\ntrue\nfalse\n",
    );
}

#[test]
fn text_from_the_standard_packages_outlives_the_text_it_came_from() {
    prints(
        "package main

import \"zore/strings\"

func pieces() Array<string> {
    let line = \"one\" + \" \" + \"two\" + \" \" + \"three\"
    return strings.Split(line, \" \")
}

func main() {
    let words = pieces()
    println(words[1])
    println(strings.Join(words[:], \"-\"))
    var trimmed = strings.TrimSpace(\"  \" + \"padded\" + \"  \")
    trimmed = strings.Upper(trimmed)
    println(trimmed)
}
",
        "two\none-two-three\nPADDED\n",
    );
}

#[test]
fn text_is_released_when_a_panic_unwinds_the_stack() {
    panics(
        "package main

import \"zore/strconv\"

type Guard struct { Label string }

func (g mut Guard) drop() { println(\"drop \" + g.Label) }

func fail(items Array<int>, text string) int {
    let more = text + strconv.Itoa(1)
    println(more)
    return items[9]
}

func main() {
    let guard = Guard{Label: \"g\" + \"1\"}
    var kept = Array<string>{\"a\" + \"b\"}
    kept.push(\"c\" + \"d\")
    println(fail(Array<int>{1}, \"t\" + \"x\"))
}
",
        "index out of range",
        "tx1\ndrop g1\n",
    );
}

#[test]
fn spawned_calls_return_their_results_and_errors_through_wait() {
    let source = "package main
func double(n int) int { return n * 2 }
func split(text string) (string, int, error) {
    if text.len() == 0 { return \"\", 0, error(\"empty\") }
    return text + \"!\", text.len(), nil
}
func note(text string) { println(text) }
func main() {
    let a = go double(21)
    let b = go double(4)
    println(a.wait() + b.wait())
    let t = go split(\"abc\")
    let text, count, err = t.wait()
    println(text)
    println(count)
    println(err == nil)
    let bad = go split(\"\")
    let _, _, failure = bad.wait()
    println(failure != nil)
    let n = go note(\"noted\")
    n.wait()
}";
    prints(source, "50\nabc!\n3\ntrue\ntrue\nnoted\n");
}

#[test]
fn spawned_inputs_move_in_and_results_move_out() {
    let source = "package main
type Resource struct { Name string }
func (r mut Resource) drop() { println(\"drop \" + r.Name) }
func rename(r own Resource, suffix string) Resource {
    println(\"rename \" + r.Name)
    return Resource{Name: r.Name + suffix}
}
func main() {
    let first = Resource{Name: \"a\"}
    let t = go rename(first, \"-b\")
    let second = t.wait()
    println(second.Name)
    let u = go rename(second, \"-c\")
    let third = u.wait()
    println(third.Name)
}";
    prints(
        source,
        "rename a\ndrop a\na-b\nrename a-b\ndrop a-b\na-b-c\ndrop a-b-c\n",
    );
}

#[test]
fn a_task_panic_unwinds_that_task_and_is_raised_again_at_the_wait() {
    let source = "package main
type Resource struct { Name string }
func (r mut Resource) drop() { println(\"drop \" + r.Name) }
func fail(r own Resource, zero int) int {
    println(\"working\")
    return 1 / zero
}
func main() {
    let t = go fail(Resource{Name: \"held\"}, 0)
    println(\"before\")
    let v = t.wait()
    println(\"unreachable\")
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "before\nworking\ndrop held\n");
    let report = stderr(&output);
    assert!(
        report.contains("panic in task 1: division by zero"),
        "{report}"
    );
    assert!(
        report.contains("panic in the main task: division by zero"),
        "{report}"
    );
}

#[test]
fn a_detached_task_panic_does_not_stop_the_program() {
    let source = "package main
func fail(zero int) int { return 1 / zero }
func ready() int { return 5 }
func main() {
    let t = go fail(0)
    drop(t)
    let ok = go ready()
    println(ok.wait())
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "5\n");
}

#[test]
fn an_endless_detached_task_does_not_keep_the_process_alive() {
    let source = "package main
func spin() {
    for var i = 0; true; i += 1 { }
}
func main() {
    go spin()
    println(\"done\")
}";
    prints(source, "done\n");
}

#[test]
fn waiting_on_a_nil_task_panics() {
    let output = run(&main_body("var t Task<int> = nil\nprintln(t.wait())"));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("wait on a nil task"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn async_functions_await_calls_and_tasks() {
    let source = "package main
func parse(text string) (int, error) {
    if text.len() == 0 { return 0, error(\"empty\") }
    return text.len(), nil
}
async func measure(text string) (int, error) {
    let task = go parse(text)
    let n = await task?
    return n * 2, nil
}
async func twice(n int) int { return n * 2 }
async func total(first string, second string) (int, error) {
    let a = go measure(first)
    let b = go measure(second)
    let x = await a?
    let y = await b?
    return await twice(x + y), nil
}
func main() {
    let t = go total(\"ab\", \"cde\")
    let n, err = t.wait()
    println(n)
    println(err == nil)
    let u = go total(\"ab\", \"\")
    let _, failure = u.wait()
    println(failure != nil)
}";
    prints(source, "20\ntrue\ntrue\n");
}

#[test]
fn many_tasks_share_and_release_text() {
    let source = "package main
func build(prefix string, count int) string {
    var text = prefix
    for var i = 0; i < count; i += 1 { text = text + \"x\" }
    return text
}
func main() {
    let shared = build(\"shared\", 20)
    var tasks = Array<Task<string>>{}
    for var i = 0; i < 32; i += 1 {
        tasks.push(go build(shared, 100 + i))
    }
    var total = 0
    for tasks.len() > 0 {
        let found, task = tasks.pop()
        if found {
            let text = task.wait()
            total += text.len()
        }
    }
    println(total)
    println(shared)
}";
    prints(source, "4528\nsharedxxxxxxxxxxxxxxxxxxxx\n");
}

#[test]
fn a_task_value_can_be_stored_and_moved_between_functions() {
    let source = "package main
type Job struct { Work Task<int> }
func compute() int { return 11 }
func start() Job { return Job{Work: go compute()} }
func finish(job own Job) int { return job.Work.wait() }
func main() {
    let job = start()
    println(finish(job))
}";
    prints(source, "11\n");
}

#[test]
fn tens_of_thousands_of_tasks_run_and_wait_on_each_other() {
    let source = "package main
func square(n int) int { return n * n }
func chain(depth int) int {
    if depth == 0 { return 0 }
    let next = go chain(depth - 1)
    return next.wait() + 1
}
func main() {
    var tasks = Array<Task<int>>{}
    for var i = 0; i < 50000; i += 1 {
        tasks.push(go square(i % 10))
    }
    var total = 0
    for tasks.len() > 0 {
        let found, task = tasks.pop()
        if found { total += task.wait() }
    }
    println(total)
    let deep = go chain(5000)
    println(deep.wait())
}";
    prints(source, "1425000\n5000\n");
}

#[test]
fn a_panic_in_one_of_many_tasks_stays_in_that_task() {
    let source = "package main
func work(n int) int { return 100 / (n % 7) }
func main() {
    var good = 0
    for var i = 1; i < 7; i += 1 {
        let t = go work(i)
        good += t.wait()
    }
    println(good)
    let bad = go work(7)
    let again = go work(2)
    println(again.wait())
    println(bad.wait())
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "244\n50\n");
    assert!(
        stderr(&output).contains("division by zero"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_unbuffered_channel_hands_values_between_tasks() {
    let source = "package main
func produce(ch channel<int>, count int) {
    for var i = 1; i <= count; i += 1 { ch.send(i) }
    ch.close()
}
func main() {
    let ch = channel<int>()
    go produce(ch, 100)
    var sum = 0
    var received = 0
    for {
        let value, ok = ch.receive()
        if !ok { break }
        sum += value
        received += 1
    }
    println(sum)
    println(received)
}";
    prints(source, "5050\n100\n");
}

#[test]
fn a_buffered_channel_keeps_order_and_waits_only_when_full() {
    let source = "package main
func main() {
    let ch = channel<string>(3)
    ch.send(\"a\")
    ch.send(\"b\")
    ch.send(\"c\")
    ch.close()
    for {
        let text, ok = ch.receive()
        if !ok { break }
        println(text)
    }
    let empty, ok = ch.receive()
    println(empty.len())
    println(ok)
}";
    prints(source, "a\nb\nc\n0\nfalse\n");
}

#[test]
fn values_sent_over_a_channel_change_owner_and_are_dropped_once() {
    let source = "package main
type Job struct { Name string }
func (j mut Job) drop() { println(\"drop \" + j.Name) }
func main() {
    let ch = channel<Job>(2)
    ch.send(Job{Name: \"a\"})
    ch.send(Job{Name: \"b\"})
    let first, ok = ch.receive()
    println(\"received \" + first.Name)
    println(ok)
}";
    prints(source, "received a\ntrue\ndrop a\ndrop b\n");
}

#[test]
fn buffered_values_are_dropped_when_the_last_handle_goes() {
    let source = "package main
type Job struct { Name string }
func (j mut Job) drop() { println(\"drop \" + j.Name) }
func hold(ch channel<Job>) { println(\"holding\") }
func fill() {
    let ch = channel<Job>(3)
    let other = ch
    ch.send(Job{Name: \"x\"})
    other.send(Job{Name: \"y\"})
    hold(other)
    println(\"leaving\")
}
func main() {
    fill()
    println(\"done\")
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(&lines[..2], ["holding", "leaving"]);
    assert_eq!(lines[4], "done");
    let mut dropped = [lines[2], lines[3]];
    dropped.sort();
    assert_eq!(dropped, ["drop x", "drop y"]);
}

#[test]
fn closing_lets_buffered_values_drain_then_reports_zero_and_false() {
    let source = "package main
type Point struct { X int
    Y int }
func main() {
    let ch = channel<Point>(2)
    ch.send(Point{X: 1, Y: 2})
    ch.close()
    let a, aok = ch.receive()
    let b, bok = ch.receive()
    println(a.X + a.Y)
    println(aok)
    println(b.X + b.Y)
    println(bok)
}";
    prints(source, "3\ntrue\n0\nfalse\n");
}

#[test]
fn sending_on_a_closed_channel_panics_and_drops_the_value() {
    let source = "package main
type Job struct { Name string }
func (j mut Job) drop() { println(\"drop \" + j.Name) }
func main() {
    let ch = channel<Job>(1)
    ch.close()
    println(\"sending\")
    ch.send(Job{Name: \"lost\"})
    println(\"unreachable\")
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "sending\ndrop lost\n");
    assert!(
        stderr(&output).contains("send on a closed channel"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn closing_twice_panics() {
    let output = run(&main_body(
        "let ch = channel<int>()\nch.close()\nch.close()",
    ));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("close of a closed channel"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_zero_value_channel_is_closed_and_empty() {
    let source = "package main
func main() {
    let holder = channel<channel<int>>(1)
    holder.close()
    let zero, ok = holder.receive()
    println(ok)
    let value, got = zero.receive()
    println(value)
    println(got)
    let again, more = zero.receive()
    println(again)
    println(more)
    zero.send(1)
    println(\"unreachable\")
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "false\n0\nfalse\n0\nfalse\n");
    assert!(
        stderr(&output).contains("send on a closed channel"),
        "{}",
        stderr(&output)
    );
    let close = run(&main_body(
        "let holder = channel<channel<int>>()\nholder.close()\nlet zero, _ = holder.receive()\nzero.close()",
    ));
    assert_eq!(close.status.code(), Some(2));
}

#[test]
fn closing_wakes_a_blocked_receiver_with_false() {
    let source = "package main
func wait(ch channel<int>) bool {
    let value, ok = ch.receive()
    return ok
}
func main() {
    let ch = channel<int>()
    let t = go wait(ch)
    ch.close()
    println(t.wait())
}";
    prints(source, "false\n");
}

#[test]
fn closing_wakes_a_blocked_sender_with_a_panic() {
    let source = "package main
type Job struct { Name string }
func (j mut Job) drop() { println(\"drop \" + j.Name) }
func push(ch channel<Job>) {
    ch.send(Job{Name: \"stuck\"})
}
func main() {
    let ch = channel<Job>()
    let t = go push(ch)
    ch.close()
    t.wait()
    println(\"unreachable\")
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "drop stuck\n");
    assert!(
        stderr(&output).contains("send on a closed channel"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_request_carries_its_own_reply_channel() {
    let source = "package main
type Request struct {
    Text string
    Reply channel<string>
}
func serve(requests channel<Request>) {
    for {
        let request, ok = requests.receive()
        if !ok { return }
        request.Reply.send(\"echo \" + request.Text)
    }
}
func main() {
    let requests = channel<Request>()
    go serve(requests)
    let letters = \"abc\"
    for var i = 0; i < 3; i += 1 {
        let reply = channel<string>()
        requests.send(Request{Text: \"hi\" + letters[i:i + 1], Reply: reply})
        let answer, _ = reply.receive()
        println(answer)
    }
    requests.close()
}";
    prints(source, "echo hia\necho hib\necho hic\n");
}

#[test]
fn a_pool_of_workers_shares_one_channel() {
    let source = "package main
func worker(jobs channel<int>, results channel<int>) {
    for {
        let n, ok = jobs.receive()
        if !ok { return }
        results.send(n * n)
    }
}
func main() {
    let jobs = channel<int>(8)
    let results = channel<int>(50)
    for var w = 0; w < 4; w += 1 {
        go worker(jobs, results)
    }
    for var n = 1; n <= 50; n += 1 {
        jobs.send(n)
    }
    jobs.close()
    var total = 0
    for var n = 0; n < 50; n += 1 {
        let square, _ = results.receive()
        total += square
    }
    println(total)
}";
    prints(source, "42925\n");
}

#[test]
fn a_thousand_tasks_pass_a_value_along_a_chain_of_channels() {
    let source = "package main
func link(from channel<int>, to channel<int>) {
    let value, _ = from.receive()
    to.send(value + 1)
}
func main() {
    let first = channel<int>()
    var last = first
    for var i = 0; i < 1000; i += 1 {
        let next = channel<int>()
        go link(last, next)
        last = next
    }
    first.send(0)
    let result, _ = last.receive()
    println(result)
}";
    prints(source, "1000\n");
}

#[test]
fn async_functions_send_and_receive_on_channels() {
    let source = "package main
async func produce(ch channel<string>) {
    ch.send(\"one\")
    ch.send(\"two\")
    ch.close()
}
async func gather(ch channel<string>) int {
    var total = 0
    for {
        let text, ok = ch.receive()
        if !ok { return total }
        total += text.len()
    }
}
func main() {
    let ch = channel<string>()
    go produce(ch)
    let t = go gather(ch)
    println(t.wait())
}";
    prints(source, "6\n");
}

#[test]
fn channel_handles_in_structs_and_collections_are_shared_and_released() {
    let source = "package main
type Pair struct {
    Left channel<int>
    Right channel<int>
}
func main() {
    let pair = Pair{Left: channel<int>(1), Right: channel<int>(1)}
    let copy = pair
    pair.Left.send(7)
    let seen, _ = copy.Left.receive()
    println(seen)
    var all = Array<channel<int>>{pair.Left, pair.Right}
    all.push(copy.Right)
    let found, last = all.pop()
    last.send(9)
    let again, _ = pair.Right.receive()
    println(again)
    println(found)
}";
    prints(source, "7\n9\ntrue\n");
}
