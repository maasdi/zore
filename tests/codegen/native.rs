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

fn run_with_input(source: &str, input: &[u8]) -> Output {
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("program");
    build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let mut child = Command::new(&executable)
        .env("ZORE_CHECK_LEAKS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start built program");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input)
        .expect("write stdin");
    child.wait_with_output().expect("run built program")
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

const RECURSIVE_NODE: &str = r#"
type Tag struct { Id int }
func (t mut Tag) drop() { if t.Id != 0 { println(t.Id) } }
func (t Tag) clone() Tag { return Tag{Id: t.Id + 100} }
type Node struct { Tag Tag; Children Array<Node>; Named map[int]Node }
func leaf(id int) Node {
    return Node{Tag: Tag{Id: id}, Children: Array<Node>{}, Named: map[int]Node{}}
}
func tree() Node {
    return Node{Tag: Tag{Id: 1}, Children: Array<Node>{leaf(2), leaf(3)}, Named: map[int]Node{4: leaf(4)}}
}
"#;

#[test]
fn recursive_owned_values_move_replace_and_drop_only_live_fields() {
    prints(
        &format!(
            "package main\n{RECURSIVE_NODE}
func main() {{
    var root = tree()
    root.Children[0] = leaf(5)
    root.Named[4] = leaf(6)
    let children = root.Children
    let tag = root.Tag
    root.Children = Array<Node>{{leaf(7)}}
    root.Tag = Tag{{Id: 8}}
    let moved = root
    println(99)
}}"
        ),
        "2\n4\n99\n6\n7\n8\n1\n3\n5\n",
    );
    prints(
        &format!(
            "package main\n{RECURSIVE_NODE}
func main() {{ let root = tree(); let tag = root.Tag; let children = root.Children }}"
        ),
        "3\n2\n1\n4\n",
    );
}

#[test]
fn recursive_clone_is_independent_and_preserves_custom_leaf_clone() {
    prints(
        &format!(
            "package main\n{RECURSIVE_NODE}
func main() {{
    let original = tree()
    var copy = clone(original)
    copy.Children[0].Tag.Id = 202
    println(original.Children[0].Tag.Id)
    drop(copy)
    println(99)
}}"
        ),
        "2\n104\n103\n202\n101\n99\n4\n3\n2\n1\n",
    );
}

#[test]
fn recursive_mutual_array_map_and_by_value_fields_have_finite_helpers() {
    let source = r#"package main
type A struct { Id int; B B }
type B struct { Array Array<C> }
type C struct { Map map[int]A }
func (a mut A) drop() { if a.Id != 0 { println(a.Id) } }
func (a A) clone() A { return A{Id: a.Id + 10, B: clone(a.B)} }
func main() {
    let a = A{Id: 1, B: B{Array: Array<C>{C{Map: map[int]A{
        2: A{Id: 2, B: B{Array: Array<C>{}}},
    }}}}}
    let b = clone(a)
}
"#;
    prints(source, "11\n12\n1\n2\n");
    let mut sources = SourceMap::new();
    let id = sources.add("recursive.ore", source.into()).unwrap();
    let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
    assert_eq!(ir.matches("define private void @zore_drop.").count(), 5);
    assert_eq!(ir.matches("define private void @zore_clone.").count(), 5);
    assert!(
        ir.len() < 100_000,
        "recursive types must not expand indefinitely"
    );
}

#[test]
fn recursive_zero_values_are_empty_and_destructible() {
    prints(
        &format!(
            "package main\n{RECURSIVE_NODE}
func failed() (Node, error) {{ return leaf(0), error(\"failed\") }}
func propagate() (Node, error) {{ let n = failed()?; return n, nil }}
func main() {{
    var nodes = Array<Node>{{}}
    let present, popped = nodes.pop()
    println(present)
    println(popped.Children.len())
    var named = map[int]Node{{}}
    let found, removed = named.remove(0)
    println(found)
    println(removed.Named.len())
    let ch = channel<Node>()
    ch.close()
    let received, ok = ch.receive()
    println(ok)
    println(received.Tag.Id)
    let result, err = propagate()
    println(err != nil)
    println(result.Children.len())
}}"
        ),
        "false\n0\nfalse\n0\nfalse\n0\ntrue\n0\n",
    );
}

#[test]
fn recursive_values_clean_up_on_error_and_partial_construction() {
    prints(
        &format!(
            "package main\n{RECURSIVE_NODE}
func failed() (Node, error) {{ return leaf(0), error(\"failed\") }}
func work() error {{
    let root = tree()
    let partial = Array<Node>{{leaf(8), failed()?}}
    return nil
}}
func main() {{ println(work() != nil) }}"
        ),
        "8\n4\n3\n2\n1\ntrue\n",
    );
    panics(
        &format!(
            "package main\n{RECURSIVE_NODE}
func fail() Node {{ var zero = 0; println(1 / zero); return leaf(0) }}
func main() {{ let root = tree(); let partial = Array<Node>{{leaf(8), fail()}} }}"
        ),
        "division by zero",
        "8\n4\n3\n2\n1\n",
    );
}

#[test]
fn recursive_custom_drop_precedes_children_and_cleans_up_after_panic() {
    let declarations = r#"
type Node struct { Id int; Children Array<Node> }
func (n mut Node) drop() {
    if n.Id != 0 { println(n.Id) }
    if n.Id == 2 { var zero = 0; println(1 / zero) }
}
func leaf(id int) Node { return Node{Id: id, Children: Array<Node>{}} }
"#;
    panics(
        &format!(
            "package main\n{declarations}
func main() {{ let n = Node{{Id: 1, Children: Array<Node>{{leaf(3), leaf(2)}}}} }}"
        ),
        "division by zero",
        "1\n2\n3\n",
    );
    let output = run_poll_bounded(
        &format!(
            "package main\n{declarations}
func main() {{ let n = leaf(2); var zero = 0; println(1 / zero) }}"
        ),
        None,
    );
    assert!(!output.status.success());
    assert_ne!(
        output.status.code(),
        Some(2),
        "must abort on a second panic"
    );
    assert_eq!(stdout(&output), "2\n");
    let output = run_poll_bounded(
        &format!(
            "package main\n{declarations}
func main() {{
    var n = Node{{Id: 1, Children: Array<Node>{{leaf(2)}}}}
    n.Children[0] = leaf(3)
}}"
        ),
        None,
    );
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "2\n");
}

#[test]
fn recursive_panicking_clone_destroys_each_completed_prefix_once() {
    let declarations = RECURSIVE_NODE.replace(
        "return Tag{Id: t.Id + 100}",
        "if t.Id == 3 { var zero = 0; println(1 / zero) }; return Tag{Id: t.Id + 100}",
    );
    panics(
        &format!(
            "package main\n{declarations}
func main() {{ let root = tree(); let copy = clone(root) }}"
        ),
        "division by zero",
        "102\n101\n4\n3\n2\n1\n",
    );
    panics(
        &format!(
            "package main\n{declarations}
func main() {{
    let root = Node{{Tag: Tag{{Id: 1}}, Children: Array<Node>{{}}, Named: map[int]Node{{2: leaf(2), 3: leaf(3)}}}}
    let copy = clone(root)
}}"
        ),
        "division by zero",
        "102\n101\n2\n3\n1\n",
    );
}

#[test]
fn recursive_async_frames_and_channel_buffers_destroy_owned_trees() {
    let source = format!(
        "package main\n{RECURSIVE_NODE}
async func send(ch channel<Node>, ready channel<int>, resume channel<int>, n own Node) {{
    ready.send(1)
    let _, _ = resume.receive()
    ch.send(n)
}}
func main() {{
    let ch = channel<Node>(2)
    let ready = channel<int>()
    let resume = channel<int>()
    let task = go send(ch, ready, resume, tree())
    let _, _ = ready.receive()
    println(99)
    resume.send(1)
    task.wait()
    ch.send(leaf(5))
}}"
    );
    let output = run_poll_bounded(&source, None);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "99\n4\n3\n2\n1\n5\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn recursive_deep_and_wide_trees_clone_and_drop_with_bounded_execution() {
    let source = r#"package main
type Node struct { Text string; Children Array<Node> }
func leaf() Node { return Node{Text: "node" + " text", Children: Array<Node>{}} }
func main() {
    var root = leaf()
    for var depth = 0; depth < 256; depth += 1 {
        let parent = Node{Text: "branch", Children: Array<Node>{root}}
        root = parent
    }
    for var width = 0; width < 1024; width += 1 { root.Children.push(leaf()) }
    let copy = clone(root)
    println(copy.Children.len())
    println(root.Children.len())
}
"#;
    let output = run_poll_bounded(source, None);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "1025\n1025\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

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
    println(\"before\")
    let t = go fail(Resource{Name: \"held\"}, 0)
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
    let (count, depth) = (50_000, 5000);
    let source = format!(
        "package main
func square(n int) int {{ return n * n }}
async func chain(depth int) int {{
    if depth == 0 {{ return 0 }}
    let next = go chain(depth - 1)
    return await next + 1
}}
func main() {{
    var tasks = Array<Task<int>>{{}}
    for var i = 0; i < {count}; i += 1 {{
        tasks.push(go square(i % 10))
    }}
    var total = 0
    for tasks.len() > 0 {{
        let found, task = tasks.pop()
        if found {{ total += task.wait() }}
    }}
    println(total)
    let deep = go chain({depth})
    println(deep.wait())
}}"
    );
    prints(&source, &format!("{}\n{depth}\n", count / 10 * 285));
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
async func link(from channel<int>, to channel<int>) {
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

#[test]
fn many_sleeping_tasks_wait_together() {
    let source = "package main
import \"zore/time\"
async func nap(ms int) int {
    time.Sleep(ms)
    return ms
}
func main() {
    let start = time.Millis()
    var tasks = Array<Task<int>>{}
    for var i = 0; i < 300; i += 1 { tasks.push(go nap(250)) }
    var total = 0
    for tasks.len() > 0 {
        let found, task = tasks.pop()
        if found { total += task.wait() }
    }
    let elapsed = time.Millis() - start
    println(total)
    println(elapsed >= 250)
    println(elapsed < 2000)
    time.Sleep(0)
    time.Sleep(-5)
    println(time.Millis() >= start)
}";
    prints(source, "75000\ntrue\ntrue\ntrue\n");
}

#[test]
fn a_sleeping_task_does_not_hold_up_a_busy_one() {
    let source = "package main
import \"zore/time\"
func sleeper(done channel<string>) {
    time.Sleep(150)
    done.send(\"slept\")
}
func counter(done channel<string>) {
    var n = 0
    for var i = 0; i < 100000; i += 1 { n += 1 }
    done.send(\"counted\")
}
func main() {
    let done = channel<string>(2)
    go sleeper(done)
    go counter(done)
    let first, _ = done.receive()
    let second, _ = done.receive()
    println(first)
    println(second)
}";
    prints(source, "counted\nslept\n");
}

#[test]
fn files_are_written_and_read_whole() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("note.txt");
    let missing = dir.path().join("absent.txt");
    let source = format!(
        "package main
import \"zore/os\"
func main() {{
    let wrote = os.WriteFile(\"{path}\", \"héllo\\nfile\")
    println(wrote == nil)
    let text, err = os.ReadFile(\"{path}\")
    println(err == nil)
    println(text)
    let rewrote = os.WriteFile(\"{path}\", \"short\")
    println(rewrote == nil)
    let again, err2 = os.ReadFile(\"{path}\")
    println(again)
    println(err2 == nil)
    let _, failure = os.ReadFile(\"{missing}\")
    println(failure != nil)
}}",
        path = path.display(),
        missing = missing.display()
    );
    prints(
        &source,
        "true\ntrue\nhéllo\nfile\ntrue\nshort\ntrue\ntrue\n",
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "short");
}

#[test]
fn file_errors_name_the_operation_and_reject_bad_text() {
    let dir = TempDir::new().unwrap();
    let binary = dir.path().join("binary.dat");
    std::fs::write(&binary, [0x66, 0xff, 0xfe]).unwrap();
    let nowhere = dir.path().join("no-folder").join("file.txt");
    let source = format!(
        "package main
import \"zore/os\"
func main() {{
    let _, bad = os.ReadFile(\"{binary}\")
    println(bad == error(\"os.ReadFile: invalid UTF-8\"))
    let write = os.WriteFile(\"{nowhere}\", \"x\")
    println(write != nil)
}}",
        binary = binary.display(),
        nowhere = nowhere.display()
    );
    prints(&source, "true\ntrue\n");
}

#[test]
fn standard_input_is_read_line_by_line() {
    let source = "package main
import \"zore/io\"
func main() {
    var lines = 0
    var length = 0
    for {
        let line, err = io.ReadLine()
        if err != nil {
            println(err == error(\"EOF\"))
            break
        }
        lines += 1
        length += line.len()
        println(\"[\" + line + \"]\")
    }
    println(lines)
    println(length)
}";
    let output = run_with_input(source, b"one\r\n\ntwo words\nlast");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "[one]\n[]\n[two words]\n[last]\ntrue\n4\n16\n"
    );
    let bad = run_with_input(
        "package main
import \"zore/io\"
func main() {
    let _, err = io.ReadLine()
    println(err == error(\"io.ReadLine: invalid UTF-8\"))
    let line, _ = io.ReadLine()
    println(line)
}",
        b"\xff\xfe\nok\n",
    );
    assert_eq!(bad.status.code(), Some(0), "{}", stderr(&bad));
    assert_eq!(stdout(&bad), "true\nok\n");
}

#[test]
fn a_task_waiting_for_input_does_not_stop_the_others() {
    let source = "package main
import \"zore/io\"
func reader(done channel<string>) {
    let line, _ = io.ReadLine()
    done.send(line)
}
func ticker(done channel<string>) {
    var n = 0
    for var i = 0; i < 1000; i += 1 { n += i }
    done.send(\"ticked\")
}
func main() {
    let typed = channel<string>(1)
    let ticks = channel<string>(1)
    go reader(typed)
    go ticker(ticks)
    let first, _ = ticks.receive()
    let second, _ = typed.receive()
    println(first)
    println(second)
}";
    let output = run_with_input(source, b"typed\n");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ticked\ntyped\n");
}

const ECHO_PRELUDE: &str = "
import \"zore/net\"
func digits(n int) string {
    if n == 0 { return \"0\" }
    var text = \"\"
    var rest = n
    for rest > 0 {
        let d = rest % 10
        text = \"0123456789\"[d:d + 1] + text
        rest = rest / 10
    }
    return text
}
func echo(conn own net.Conn) int {
    var total = 0
    for {
        let text, err = conn.Read(64)
        if err != nil { return total }
        total += text.len()
        if conn.Write(text) != nil { return total }
    }
}
func serve(listener own net.Listener, clients int) int {
    var tasks = Array<Task<int>>{}
    for var i = 0; i < clients; i += 1 {
        let conn, err = listener.Accept()
        if err != nil { return -1 }
        tasks.push(go echo(conn))
    }
    var total = 0
    for tasks.len() > 0 {
        let found, task = tasks.pop()
        if found { total += task.wait() }
    }
    return total
}
func client(port int, message string, chunk int) string {
    let conn, err = net.Dial(\"127.0.0.1:\" + digits(port))
    if err != nil { return \"dial failed\" }
    if conn.Write(message) != nil { return \"write failed\" }
    _ = conn.CloseWrite()
    var reply = \"\"
    for {
        let text, readErr = conn.Read(chunk)
        if readErr != nil { return reply }
        reply += text
    }
}
";

#[test]
fn a_server_echoes_text_to_several_clients_without_splitting_characters() {
    let source = format!(
        "package main
{ECHO_PRELUDE}
func main() {{
    let listener, err = net.Listen(\"127.0.0.1:0\")
    if err != nil {{ println(\"listen failed\"); return }}
    let port = listener.Port()
    let server = go serve(listener, 3)
    let a = go client(port, \"hello\", 64)
    let b = go client(port, \"héllo wörld ✓\", 1)
    let c = go client(port, \"x\", 5)
    println(a.wait())
    println(b.wait())
    println(c.wait())
    println(server.wait())
}}"
    );
    prints(&source, "hello\nhéllo wörld ✓\nx\n23\n");
}

#[test]
fn hundreds_of_connections_share_the_event_loop() {
    let source = format!(
        "package main
{ECHO_PRELUDE}
func main() {{
    let listener, err = net.Listen(\"127.0.0.1:0\")
    if err != nil {{ println(\"listen failed\"); return }}
    let port = listener.Port()
    let server = go serve(listener, 200)
    var clients = Array<Task<string>>{{}}
    for var i = 0; i < 200; i += 1 {{
        clients.push(go client(port, \"message \" + digits(i), 7))
    }}
    var bytes = 0
    for clients.len() > 0 {{
        let found, task = clients.pop()
        if found {{ bytes += task.wait().len() }}
    }}
    println(bytes)
    println(server.wait())
}}"
    );
    let expected: usize = (0..200)
        .map(|i| "message ".len() + i.to_string().len())
        .sum();
    prints(&source, &format!("{expected}\n{expected}\n"));
}

#[test]
fn dropping_a_connection_ends_the_peers_stream() {
    let source = "package main
import \"zore/net\"
func greet(listener own net.Listener) {
    let conn, err = listener.Accept()
    if err != nil { return }
    _ = conn.Write(\"hi\")
    drop(conn)
}
func digits(n int) string {
    var text = \"\"
    var rest = n
    for rest > 0 {
        let d = rest % 10
        text = \"0123456789\"[d:d + 1] + text
        rest = rest / 10
    }
    return text
}
func main() {
    let listener, err = net.Listen(\"127.0.0.1:0\")
    if err != nil { return }
    let port = listener.Port()
    go greet(listener)
    let conn, derr = net.Dial(\"127.0.0.1:\" + digits(port))
    if derr != nil { println(\"dial failed\"); return }
    let first, _ = conn.Read(10)
    println(first)
    let _, end = conn.Read(10)
    println(end == error(\"EOF\"))
}";
    prints(source, "hi\ntrue\n");
}

#[test]
fn network_errors_are_reported_and_closed_handles_fail() {
    let source = "package main
import \"zore/net\"
func main() {
    let conns = channel<net.Conn>(1)
    conns.close()
    let zero, _ = conns.receive()
    let text, err = zero.Read(10)
    println(text.len())
    println(err != nil)
    println(zero.Write(\"x\") != nil)
    println(zero.CloseWrite() != nil)
    let listeners = channel<net.Listener>(1)
    listeners.close()
    let closed, _ = listeners.receive()
    let _, accept = closed.Accept()
    println(accept != nil)
    println(closed.Port())
    let _, refused = net.Dial(\"127.0.0.1:1\")
    println(refused != nil)
    let _, malformed = net.Listen(\"not an address\")
    println(malformed != nil)
    let open, _ = net.Listen(\"127.0.0.1:0\")
    println(open.Port() > 0)
}";
    prints(source, "0\ntrue\ntrue\ntrue\ntrue\n-1\ntrue\ntrue\ntrue\n");
}

#[test]
fn a_peer_that_sends_bytes_that_are_not_text_gives_an_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = std::thread::spawn(move || {
        for bytes in [&[b'o', b'k', 0xC3][..], &[0xFF, b'x'][..]] {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(bytes).unwrap();
        }
    });
    let source = format!(
        "package main
import \"zore/net\"
func main() {{
    let first, _ = net.Dial(\"127.0.0.1:{port}\")
    let a, errA = first.Read(16)
    println(a)
    println(errA == nil)
    let b, errB = first.Read(16)
    println(errB == error(\"net.Read: invalid UTF-8\"))
    let second, _ = net.Dial(\"127.0.0.1:{port}\")
    let c, errC = second.Read(16)
    println(errC == error(\"net.Read: invalid UTF-8\"))
}}"
    );
    prints(&source, "ok\ntrue\ntrue\ntrue\n");
    peer.join().unwrap();
}

#[test]
fn a_listener_waiting_in_accept_does_not_stop_other_tasks() {
    let source = "package main
import \"zore/net\"
func wait(listener own net.Listener) string {
    let conn, err = listener.Accept()
    if err != nil { return \"failed\" }
    return \"accepted\"
}
func work(done channel<int>) {
    var n = 0
    for var i = 0; i < 1000; i += 1 { n += i }
    done.send(n)
}
func main() {
    let listener, err = net.Listen(\"127.0.0.1:0\")
    if err != nil { return }
    let pending = go wait(listener)
    let done = channel<int>()
    go work(done)
    let n, _ = done.receive()
    println(n)
    drop(pending)
}";
    prints(source, "499500\n");
}

fn assert_deadlock(output: &Output) {
    assert_eq!(output.status.code(), Some(2), "{}", stderr(output));
    assert!(
        stderr(output).contains("all tasks are asleep"),
        "{}",
        stderr(output)
    );
}

#[test]
fn a_receive_nobody_will_answer_is_reported_as_a_deadlock() {
    let output = run(&main_body(
        "let ch = channel<int>()\nprintln(\"waiting\")\nlet v, _ = ch.receive()\nprintln(v)",
    ));
    assert_deadlock(&output);
    assert_eq!(stdout(&output), "waiting\n");
}

#[test]
fn a_send_with_no_receiver_is_reported_as_a_deadlock() {
    assert_deadlock(&run(&main_body("let ch = channel<int>()\nch.send(1)")));
}

#[test]
fn tasks_waiting_on_each_other_are_reported_as_a_deadlock() {
    let source = "package main
func relay(a channel<int>, b channel<int>) {
    let v, _ = a.receive()
    b.send(v)
}
func main() {
    let x = channel<int>()
    let y = channel<int>()
    let t = go relay(x, y)
    go relay(y, x)
    t.wait()
    println(\"unreachable\")
}";
    assert_deadlock(&run(source));
}

#[test]
fn waiting_for_a_task_that_waits_forever_is_reported_as_a_deadlock() {
    let source = "package main
func stuck(ch channel<int>) int {
    let v, _ = ch.receive()
    return v
}
func main() {
    let ch = channel<int>()
    let t = go stuck(ch)
    println(t.wait())
}";
    assert_deadlock(&run(source));
}

#[test]
fn the_last_running_task_finishing_leaves_a_deadlock() {
    let source = "package main
func finish(done channel<int>) {
    var n = 0
    for var i = 0; i < 100000; i += 1 { n += i }
}
func main() {
    let never = channel<int>()
    go finish(never)
    let v, _ = never.receive()
    println(v)
}";
    assert_deadlock(&run(source));
}

#[test]
fn waiting_on_a_timer_a_helper_thread_or_a_busy_task_is_not_a_deadlock() {
    let source = "package main
import \"zore/time\"
import \"zore/os\"
func late(ch channel<int>) {
    time.Sleep(200)
    ch.send(5)
}
func reader(ch channel<string>) {
    let text, _ = os.ReadFile(\"/dev/null\")
    ch.send(\"read\" + text)
}
func busy(ch channel<int>) {
    var n = 0
    for var i = 0; i < 20000000; i += 1 { n += 1 }
    ch.send(n)
}
func main() {
    let timed = channel<int>()
    go late(timed)
    let a, _ = timed.receive()
    println(a)
    let files = channel<string>()
    go reader(files)
    let b, _ = files.receive()
    println(b)
    let work = channel<int>()
    go busy(work)
    let c, _ = work.receive()
    println(c)
}";
    prints(source, "5\nread\n20000000\n");
}

#[test]
fn a_waiting_accept_keeps_the_program_from_being_called_dead() {
    let source = "package main
import \"zore/net\"
import \"zore/time\"
func accept(listener own net.Listener, done channel<string>) {
    let conn, err = listener.Accept()
    if err != nil {
        done.send(\"failed\")
        return
    }
    done.send(\"accepted\")
}
func digits(n int) string {
    var text = \"\"
    var rest = n
    for rest > 0 {
        let d = rest % 10
        text = \"0123456789\"[d:d + 1] + text
        rest = rest / 10
    }
    return text
}
func main() {
    let listener, _ = net.Listen(\"127.0.0.1:0\")
    let port = listener.Port()
    let done = channel<string>()
    go accept(listener, done)
    time.Sleep(150)
    let conn, _ = net.Dial(\"127.0.0.1:\" + digits(port))
    let result, _ = done.receive()
    println(result)
}";
    prints(source, "accepted\n");
}

#[test]
fn many_tasks_update_one_mutex_without_losing_a_write() {
    let source = "package main
func worker(counter Mutex<int>, done channel<bool>) {
    for var i = 0; i < 1000; i += 1 {
        counter.withLock(func(value mut int) { value += 1 })
    }
    done.send(true)
}
func main() {
    let counter = mutex(0)
    let done = channel<bool>(100)
    for var i = 0; i < 100; i += 1 { go worker(counter, done) }
    for var i = 0; i < 100; i += 1 {
        let _, _ = done.receive()
    }
    println(counter.withLock(func(value mut int) int { return value }))
    println(counter.isPoisoned())
}";
    prints(source, "100000\nfalse\n");
}

#[test]
fn a_mutex_guards_a_move_value_and_drops_it_once_with_the_last_handle() {
    let source = "package main
type Ledger struct {
    Name string
    Log Array<string>
}
func (l mut Ledger) drop() { println(\"drop \" + l.Name) }
func record(shared Mutex<Ledger>, entry string) {
    shared.withLock(func(l mut Ledger) { l.Log.push(entry) })
}
func main() {
    let ledger = mutex(Ledger{Name: \"books\", Log: Array<string>{}})
    let other = ledger
    record(ledger, \"a\")
    record(other, \"b\")
    let count, name = ledger.withLock(func(l mut Ledger) (int, string) {
        return l.Log.len(), l.Name
    })
    println(count)
    println(name)
    println(\"end\")
}";
    prints(source, "2\nbooks\nend\ndrop books\n");
}

#[test]
fn results_of_the_function_come_back_from_with_lock() {
    let source = "package main
func parse(m Mutex<string>) (int, error) {
    return m.withLock(func(text mut string) (int, error) {
        if text.len() == 0 { return 0, error(\"empty\") }
        return text.len(), nil
    })
}
func main() {
    let good = mutex(\"abc\")
    let n, err = parse(good)
    println(n)
    println(err == nil)
    let empty = mutex(\"\")
    let _, bad = parse(empty)
    println(bad == error(\"empty\"))
}";
    prints(source, "3\ntrue\ntrue\n");
}

#[test]
fn a_task_waiting_for_the_lock_does_not_block_the_others() {
    let source = "package main
import \"zore/time\"
func holder(m Mutex<int>, events channel<string>) {
    m.withLock(func(value mut int) {
        events.send(\"holder locked\")
        time.Sleep(400)
        value = 7
        events.send(\"holder done\")
    })
}
func waiter(m Mutex<int>, events channel<string>) {
    let seen = m.withLock(func(value mut int) int { return value })
    if seen == 7 {
        events.send(\"waiter saw seven\")
    }
}
func other(events channel<string>) {
    events.send(\"other ran\")
}
func main() {
    let m = mutex(0)
    let events = channel<string>(8)
    go holder(m, events)
    let first, _ = events.receive()
    println(first)
    go waiter(m, events)
    go other(events)
    let second, _ = events.receive()
    println(second)
    let third, _ = events.receive()
    println(third)
    let fourth, _ = events.receive()
    println(fourth)
}";
    prints(
        source,
        "holder locked\nother ran\nholder done\nwaiter saw seven\n",
    );
}

#[test]
fn waiters_get_the_lock_in_the_order_they_arrived() {
    let source = "package main
import \"zore/time\"
func take(m Mutex<Array<int>>, id int, ready channel<bool>) {
    ready.send(true)
    m.withLock(func(order mut Array<int>) { order.push(id) })
}
func main() {
    let m = mutex(Array<int>{})
    let ready = channel<bool>(8)
    m.withLock(func(order mut Array<int>) {
        for var id = 1; id <= 4; id += 1 {
            go take(m, id, ready)
            let _, _ = ready.receive()
            time.Sleep(200)
        }
    })
    time.Sleep(500)
    let result = m.withLock(func(order mut Array<int>) int {
        var code = 0
        for var i = 0; i < order.len(); i += 1 {
            code = code * 10 + order[i]
        }
        return code
    })
    println(result)
}";
    prints(source, "1234\n");
}

#[test]
fn a_panic_while_holding_the_lock_poisons_the_mutex() {
    let source = "package main
import \"zore/time\"
type Marker struct { Name string }
func (m mut Marker) drop() { println(\"drop \" + m.Name) }
func breaker(m Mutex<int>) {
    let marker = Marker{Name: \"inside\"}
    m.withLock(func(value mut int) {
        value += 1
        let boom = 1 / (value - 1)
        println(boom)
    })
}
func main() {
    let m = mutex(0)
    println(m.isPoisoned())
    let t = go breaker(m)
    time.Sleep(200)
    println(m.isPoisoned())
    m.withLock(func(value mut int) { println(\"unreachable\") })
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "false\ndrop inside\ntrue\n");
    let report = stderr(&output);
    assert!(report.contains("division by zero"), "{report}");
    assert!(report.contains("withLock on a poisoned mutex"), "{report}");
}

#[test]
fn the_zero_value_mutex_has_no_lock() {
    let source = "package main
func main() {
    let holder = channel<Mutex<int>>(1)
    holder.close()
    let zero, _ = holder.receive()
    println(zero.isPoisoned())
    zero.withLock(func(value mut int) { println(\"unreachable\") })
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "false\n");
    assert!(
        stderr(&output).contains("withLock on a zero-value mutex"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn locking_a_mutex_inside_its_own_function_is_a_deadlock() {
    let source = "package main
func main() {
    let m = mutex(0)
    m.withLock(func(value mut int) {
        m.withLock(func(inner mut int) { inner += 1 })
    })
}";
    assert_deadlock(&run(source));
}

#[test]
fn two_tasks_locking_in_opposite_order_are_a_deadlock() {
    let source = "package main
import \"zore/time\"
func lockBoth(first Mutex<int>, second Mutex<int>, done channel<bool>) {
    first.withLock(func(a mut int) {
        time.Sleep(100)
        second.withLock(func(b mut int) { b += a })
    })
    done.send(true)
}
func main() {
    let a = mutex(1)
    let b = mutex(2)
    let done = channel<bool>(2)
    go lockBoth(a, b, done)
    go lockBoth(b, a, done)
    let _, _ = done.receive()
    let _, _ = done.receive()
}";
    assert_deadlock(&run(source));
}

#[test]
fn a_mutex_works_from_async_functions_and_hundreds_of_waiters() {
    let source = "package main
import \"zore/time\"
async func add(m Mutex<int>, amount int) {
    m.withLock(func(value mut int) {
        time.Sleep(1)
        value += amount
    })
}
func run(m Mutex<int>, amount int) {
    let t = go add(m, amount)
    t.wait()
}
func main() {
    let m = mutex(0)
    var tasks = Array<Task>{}
    for var i = 1; i <= 300; i += 1 {
        tasks.push(go add(m, i))
    }
    for tasks.len() > 0 {
        let found, task = tasks.pop()
        if found { task.wait() }
    }
    println(m.withLock(func(value mut int) int { return value }))
}";
    prints(source, "45150\n");
}

#[test]
fn select_takes_whichever_channel_has_a_value() {
    prints(
        &main_body(
            "let a = channel<int>(1)
let b = channel<string>(1)
b.send(\"hi\")
select {
    case let n, ok = a.receive() { println(n) }
    case let s, ok = b.receive() { println(s) }
}
a.send(7)
select {
    case let n, ok = a.receive() { println(n)\nprintln(ok) }
    default { println(\"none\") }
}
select {
    case let n, ok = a.receive() { println(n) }
    default { println(\"none\") }
}",
        ),
        "hi\n7\ntrue\nnone\n",
    );
}

#[test]
fn select_sends_to_a_free_buffer_and_skips_a_full_one() {
    prints(
        &main_body(
            "let full = channel<string>(1)
let free = channel<string>(1)
full.send(\"old\")
select {
    case full.send(\"a\") { println(\"full\") }
    case free.send(\"b\") { println(\"free\") }
}
let s, _ = free.receive()
println(s)
let t, _ = full.receive()
println(t)",
        ),
        "free\nb\nold\n",
    );
}

#[test]
fn select_waits_for_a_task_and_merges_two_producers() {
    prints(
        "package main
func produce(out channel<int>, base int) {
    for var i = 1; i <= 100; i += 1 { out.send(base + i) }
    out.close()
}
func main() {
    var a = channel<int>()
    var b = channel<int>()
    go produce(a, 0)
    go produce(b, 1000)
    var total = 0
    var open = 2
    for open > 0 {
        select {
            case let n, ok = a.receive() {
                if ok { total += n } else { open -= 1\na = channel<int>() }
            }
            case let n, ok = b.receive() {
                if ok { total += n } else { open -= 1\nb = channel<int>() }
            }
        }
    }
    println(total)
}",
        "110100\n",
    );
}

#[test]
fn select_wakes_on_close_and_on_a_zero_value_channel() {
    prints(
        "package main
func closer(gate channel<int>) { gate.close() }
func main() {
    let holder = channel<channel<int>>(1)
    holder.close()
    let none, _ = holder.receive()
    let gate = channel<int>()
    go closer(gate)
    select {
        case let n, ok = gate.receive() { println(ok) }
    }
    select {
        case let n, ok = none.receive() { println(ok) }
    }
}",
        "false\nfalse\n",
    );
}

#[test]
fn select_send_to_a_closed_channel_panics_and_drops_the_value() {
    panics(
        &main_body(
            "let ch = channel<string>(1)
ch.close()
select {
    case ch.send(\"x\") { println(\"sent\") }
}",
        ),
        "send on a closed channel",
        "",
    );
}

#[test]
fn select_with_nothing_ready_and_no_other_task_is_a_deadlock() {
    assert_deadlock(&run(&main_body(
        "let a = channel<int>()
let b = channel<int>()
select {
    case let n, ok = a.receive() { println(n) }
    case b.send(1) { }
}",
    )));
}

#[test]
fn strings_convert_to_bytes_and_back_with_a_utf8_check() {
    prints(
        "package main
import \"zore/strings\"
func main() {
    let data = strings.Bytes(\"héllo\")
    println(data.len())
    println(data[1])
    println(data[2])
    let text, err = strings.FromBytes(data[:])
    println(text)
    println(err == nil)
    let bad = Array<byte>{104, 255}
    let none, failure = strings.FromBytes(bad[:])
    println(none.len())
    println(failure == error(\"strings.FromBytes: invalid UTF-8\"))
    let cut = Array<byte>{195}
    let _, cutErr = strings.FromBytes(cut[:])
    println(cutErr != nil)
    let empty = strings.Bytes(\"\")
    println(empty.len())
    let whole, emptyErr = strings.FromBytes(empty[:])
    println(whole.len())
    println(emptyErr == nil)
    let part, partErr = strings.FromBytes(data[0:3])
    println(part)
    println(partErr == nil)
}",
        "6\n195\n169\nhéllo\ntrue\n0\ntrue\ntrue\n0\n0\ntrue\nhé\ntrue\n",
    );
}

#[test]
fn files_hold_any_bytes_and_text_reads_still_check_utf8() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("data.bin").display().to_string();
    let source = format!(
        "package main
import \"zore/os\"
func main() {{
    let data = Array<byte>{{0, 255, 10, 0, 128}}
    println(os.WriteBytes(\"{path}\", data[:]) == nil)
    let back, err = os.ReadBytes(\"{path}\")
    println(err == nil)
    println(back.len())
    for i, b in back {{ println(b) }}
    let _, textErr = os.ReadFile(\"{path}\")
    println(textErr == error(\"os.ReadFile: invalid UTF-8\"))
    let _, missing = os.ReadBytes(\"{path}.missing\")
    println(missing != nil)
    println(os.WriteBytes(\"{path}.dir/none\", data[0:0]) != nil)
}}"
    );
    prints(
        &source,
        "true\ntrue\n5\n0\n255\n10\n0\n128\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn a_connection_moves_raw_bytes_in_both_directions() {
    let source = format!(
        "package main
{ECHO_PRELUDE}
func bounce(conn own net.Conn) int {{
    var total = 0
    for {{
        let data, err = conn.ReadBytes(3)
        if err != nil {{ return total }}
        total += data.len()
        if conn.WriteBytes(data[:]) != nil {{ return total }}
    }}
}}
func accept(listener own net.Listener) int {{
    let conn, err = listener.Accept()
    if err != nil {{ return -1 }}
    return bounce(conn)
}}
func main() {{
    let listener, err = net.Listen(\"127.0.0.1:0\")
    if err != nil {{ println(\"listen failed\"); return }}
    let port = listener.Port()
    let server = go accept(listener)
    let conn, dialErr = net.Dial(\"127.0.0.1:\" + digits(port))
    if dialErr != nil {{ println(\"dial failed\"); return }}
    let sent = Array<byte>{{0, 255, 254, 1, 2, 3, 128, 0}}
    println(conn.WriteBytes(sent[:]) == nil)
    _ = conn.CloseWrite()
    var seen = Array<byte>{{}}
    for {{
        let data, readErr = conn.ReadBytes(64)
        if readErr != nil {{ break }}
        for i, b in data {{ seen.push(b) }}
    }}
    println(seen.len())
    var same = true
    for i, b in sent {{
        if seen[i] != b {{ same = false }}
    }}
    println(same)
    let _, zeroErr = conn.ReadBytes(0)
    println(zeroErr != nil)
    println(server.wait())
}}"
    );
    prints(&source, "true\n8\ntrue\ntrue\n8\n");
}

#[test]
fn after_gives_a_channel_that_fires_once_and_closes() {
    prints(
        "package main
import \"zore/time\"
func main() {
    let started = time.Millis()
    let timer = time.After(60)
    let first, ok = timer.receive()
    println(first)
    println(ok)
    println(time.Millis() - started >= 60)
    let again, more = timer.receive()
    println(again)
    println(more)
    let instant = time.After(0)
    let _, fired = instant.receive()
    println(fired)
    let slow = time.After(5000)
    let quick = time.After(20)
    select {
        case slow.receive() { println(\"slow\") }
        case quick.receive() { println(\"quick\") }
    }
    println(time.Millis() - started < 3000)
}",
        "true\ntrue\ntrue\nfalse\nfalse\ntrue\nquick\ntrue\n",
    );
}

#[test]
fn a_task_waiting_on_a_timer_channel_is_not_a_deadlock() {
    prints(
        "package main
import \"zore/time\"
func main() {
    let never = channel<int>()
    select {
        case let v, ok = never.receive() { println(v) }
        case time.After(40).receive() { println(\"timed out\") }
    }
}",
        "timed out\n",
    );
}

#[test]
fn a_token_cancels_once_and_wakes_sleepers_and_children() {
    prints(
        "package main
import \"zore/cancel\"
import \"zore/time\"
func worker(token cancel.Token, results channel<int>) {
    var steps = 0
    for {
        if !token.Sleep(10) { break }
        steps += 1
    }
    results.send(steps)
}
func main() {
    let token = cancel.New()
    println(token.Cancelled())
    let results = channel<int>(1)
    go worker(token, results)
    time.Sleep(60)
    token.Cancel()
    token.Cancel()
    println(token.Cancelled())
    let steps, _ = results.receive()
    println(steps >= 2)
    println(token.Sleep(5000))

    let limit = cancel.WithTimeout(30)
    let child = limit.Child()
    let grandchild = child.Child()
    let other = cancel.New()
    let _, _ = grandchild.Done().receive()
    println(limit.Cancelled())
    println(child.Cancelled())
    println(other.Cancelled())
    println(other.Sleep(20))

    let quiet = cancel.New()
    let kid = quiet.Child()
    kid.Cancel()
    println(quiet.Cancelled())
    println(kid.Cancelled())
}",
        "false\ntrue\ntrue\nfalse\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\n",
    );
}

#[test]
fn reads_and_accepts_give_up_after_their_time_limit() {
    let source = format!(
        "package main
import \"zore/time\"
{ECHO_PRELUDE}
func quiet(listener own net.Listener, release channel<bool>) {{
    let conn, err = listener.Accept()
    if err != nil {{ return }}
    let _, _ = release.receive()
    _ = conn.Write(\"late\")
}}
func main() {{
    let listener, err = net.Listen(\"127.0.0.1:0\")
    if err != nil {{ println(\"listen failed\"); return }}
    let port = listener.Port()
    println(listener.SetTimeout(40) == nil)
    let started = time.Millis()
    let _, acceptErr = listener.Accept()
    println(acceptErr == error(\"net.Accept: timed out\"))
    println(time.Millis() - started >= 40)
    println(listener.SetTimeout(0) == nil)

    let release = channel<bool>(1)
    let server = go quiet(listener, release)
    let conn, dialErr = net.DialTimeout(\"127.0.0.1:\" + digits(port), 2000)
    if dialErr != nil {{ println(\"dial failed\"); return }}
    println(conn.SetTimeout(50) == nil)
    let first = time.Millis()
    let _, readErr = conn.Read(16)
    println(readErr == error(\"net.Read: timed out\"))
    println(time.Millis() - first >= 50)
    let _, bytesErr = conn.ReadBytes(16)
    println(bytesErr == error(\"net.ReadBytes: timed out\"))
    release.send(true)
    println(conn.SetTimeout(0) == nil)
    let text, laterErr = conn.Read(16)
    println(text)
    println(laterErr == nil)
    server.wait()
}}"
    );
    prints(
        &source,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\nlate\ntrue\n",
    );
}

#[test]
fn a_write_to_a_peer_that_never_reads_gives_up() {
    let source = format!(
        "package main
{ECHO_PRELUDE}
func hold(listener own net.Listener, release channel<bool>) {{
    let conn, err = listener.Accept()
    if err != nil {{ return }}
    let _, _ = release.receive()
}}
func flood(conn net.Conn) error {{
    var chunk = Array<byte>{{}}
    for var i = 0; i < 65536; i += 1 {{ chunk.push(7) }}
    for var i = 0; i < 4000; i += 1 {{
        let writeErr = conn.WriteBytes(chunk[:])
        if writeErr != nil {{ return writeErr }}
    }}
    return nil
}}
func main() {{
    let listener, err = net.Listen(\"127.0.0.1:0\")
    if err != nil {{ println(\"listen failed\"); return }}
    let port = listener.Port()
    let release = channel<bool>(1)
    let server = go hold(listener, release)
    let conn, dialErr = net.Dial(\"127.0.0.1:\" + digits(port))
    if dialErr != nil {{ println(\"dial failed\"); return }}
    println(conn.SetTimeout(100) == nil)
    println(flood(conn) == error(\"net.WriteBytes: timed out\"))
    release.send(true)
    server.wait()
}}"
    );
    prints(&source, "true\ntrue\n");
}

#[test]
fn time_limits_reject_closed_handles_and_connects_that_fail() {
    prints(
        "package main
import \"zore/net\"
func main() {
    let holder = channel<net.Conn>(1)
    holder.close()
    let zero, _ = holder.receive()
    println(zero.SetTimeout(10) == error(\"net.SetTimeout: not an open connection or listener\"))
    let _, refused = net.DialTimeout(\"127.0.0.1:1\", 500)
    println(refused != nil)
    let _, bad = net.DialTimeout(\"not an address\", 500)
    println(bad != nil)
}",
        "true\ntrue\ntrue\n",
    );
}

fn build_in(dir: &TempDir, name: &str, body: &str, cache: &std::path::Path) -> Output {
    let folder = dir.path().join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let source = folder.join("main.ore");
    std::fs::write(&source, main_body(body)).unwrap();
    zore()
        .arg("build")
        .arg(&source)
        .current_dir(&folder)
        .env("ZORE_CACHE_DIR", cache)
        .output()
        .unwrap()
}

#[test]
fn the_compiled_runtime_is_cached_and_shared_between_builds() {
    let dir = TempDir::new().unwrap();
    let cache = dir.path().join("cache");
    for (name, body) in [("first", "println(1)"), ("second", "println(2)")] {
        let output = build_in(&dir, name, body, &cache);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    }
    let entries: Vec<_> = std::fs::read_dir(cache.join("zore"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    let library = entries[0].join("libzore_runtime.rlib");
    assert!(library.is_file(), "{library:?}");
    let staged: Vec<_> = std::fs::read_dir(&entries[0]).unwrap().collect();
    assert_eq!(staged.len(), 1, "{staged:?}");
    for name in ["first", "second"] {
        let run = Command::new(dir.path().join(name).join("main"))
            .output()
            .unwrap();
        assert_eq!(stdout(&run), if name == "first" { "1\n" } else { "2\n" });
    }
}

#[test]
fn a_cache_folder_that_cannot_be_used_does_not_stop_a_build() {
    let dir = TempDir::new().unwrap();
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "a file, not a folder").unwrap();
    let output = build_in(&dir, "plain", "println(3)", &blocked);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let run = Command::new(dir.path().join("plain").join("main"))
        .output()
        .unwrap();
    assert_eq!(stdout(&run), "3\n");
}

#[test]
fn async_state_machine_ir_uses_heap_storage_and_explicit_resume_states() {
    let source = "package main
async func leaf(t own Task<int>) int { let view = [int; 2]{3, 4}; return view[0] + await t }
async func parent(t own Task<int>) int { return await leaf(t) }
async func fallback(m Mutex<int>) int { return m.withLock(func(n mut int) int { return n }) }
func main() {}";
    let mut sources = SourceMap::new();
    let id = sources.add("async.ore", source.into()).unwrap();
    let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
    assert!(ir.contains("AsyncFrame"));
    assert!(ir.contains("leaf$async$new"));
    assert!(ir.contains("fallback$async$poll"));
    for function in ir.split("define private i8 ").skip(1) {
        let function = function.split("\n}\n").next().unwrap();
        if !function.contains("$async$poll") {
            continue;
        }
        assert!(function.contains("switch i32 %state"));
        assert!(function.contains("ret i8 0"));
        assert!(function.contains("ret i8 1"));
        if function.starts_with("@\"main.leaf$async$poll\"") {
            assert!(function.contains("alloca [2 x i64]"));
        }
        assert!(!function.contains("call ptr @zore_task_wait"));
    }
    assert!(ir.contains("call i8 @zore_task_poll"));
}

#[test]
fn poll_local_drop_flags_are_initialized_and_cleanup_survives_pending() {
    let source = r#"package main
type Job struct { Name string }
func (j mut Job) drop() { println("drop " + j.Name) }
async func leaf() {
    let local = Job{Name: "leaf"}
    println(local.Name)
}

async func run(ch channel<int>) int {
    await leaf()
    {
        let early = Job{Name: "early"}
        println(early.Name)
    }
    ch.send(1)
    let late = Job{Name: "late"}
    println(late.Name)
    return 2
}
func main() {
    let ch = channel<int>()
    let task = go run(ch)
    let _, _ = ch.receive()
    println(task.wait())
}"#;
    let mut sources = SourceMap::new();
    let id = sources.add("async.ore", source.into()).unwrap();
    let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
    let leaf = ir
        .split("define private i8 @\"main.leaf$async$poll\"")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    assert!(leaf.contains("alloca i1"));
    assert!(leaf.contains("store i1 zeroinitializer"));
    prints(
        source,
        "leaf\ndrop leaf\nearly\ndrop early\nlate\ndrop late\n2\n",
    );
}

#[test]
fn reused_async_frame_arrays_survive_child_and_implicit_waits_with_error_cleanup() {
    prints(
        r#"package main
type Guard struct { Name string }
func (g mut Guard) drop() { println("drop " + g.Name) }
async func pause(ch channel<int>) { ch.send(1) }
func fail() (int, error) { return 0, error("expected") }
async func run(ch channel<int>) (int, error) {
    var total = 0
    {
        let guard = Guard{Name: "first"}
        let first = [int; 4]{1, 2, 3, 4}
        await pause(ch)
        total += first[0]
    }
    {
        let guard = Guard{Name: "second"}
        let second = [int; 4]{5, 6, 7, 8}
        ch.send(2)
        total += second[0]
        let ignored = fail()?
        total += ignored
    }
    return total, nil
}
func main() {
    let ch = channel<int>()
    let task = go run(ch)
    let a, _ = ch.receive()
    let b, _ = ch.receive()
    let result, err = task.wait()
    println(a + b)
    println(result)
    println(err == error("expected"))
}"#,
        "drop first\ndrop second\n3\n0\ntrue\n",
    );
}

#[test]
fn reused_async_frame_arrays_preserve_panic_cleanup() {
    let output = run(r#"package main
type Guard struct { Name string }
func (g mut Guard) drop() { println("drop " + g.Name) }
async func pause(ch channel<int>) { ch.send(1) }
async func run(ch channel<int>) int {
    {
        let guard = Guard{Name: "first"}
        let first = [int; 4]{1, 2, 3, 4}
        await pause(ch)
        println(first[0])
    }
    {
        let guard = Guard{Name: "second"}
        let second = [int; 4]{5, 6, 7, 8}
        await pause(ch)
        println(second[0])
        let zero = second[0] - 5
        return 1 / zero
    }
}
func main() {
    let ch = channel<int>()
    let task = go run(ch)
    let _, _ = ch.receive()
    let _, _ = ch.receive()
    println(task.wait())
}"#);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "1\ndrop first\n5\ndrop second\n");
    assert!(stderr(&output).contains("panic in the main task: division by zero"));
}

#[test]
fn borrowed_async_frame_arrays_keep_views_valid_across_resumes() {
    prints(
        r#"package main
async func pause(ch channel<int>) { ch.send(1) }
async func run(ch channel<int>) int {
    var total = 0
    {
        let first = [int; 4]{1, 2, 3, 4}
        let view = first[:]
        await pause(ch)
        total += view[0]
    }
    {
        let second = [int; 4]{5, 6, 7, 8}
        let view = second[:]
        await pause(ch)
        total += view[0]
    }
    return total
}

func main() {
    let ch = channel<int>()
    let task = go run(ch)
    let _, _ = ch.receive()
    let _, _ = ch.receive()
    println(task.wait())
}"#,
        "6\n",
    );
}

#[test]
fn reused_async_frame_arrays_survive_budget_resumes() {
    prints(
        r#"package main
async func run() int {
    var total = 0
    {
        let first = [int; 4]{1, 2, 3, 4}
        for var i = 0; i < 300; i += 1 { total += first[0] }
    }
    {
        let second = [int; 4]{5, 6, 7, 8}
        for var i = 0; i < 300; i += 1 { total += second[0] }
    }
    return total
}
func main() { let task = go run(); println(task.wait()) }"#,
        "1800\n",
    );
}

#[test]
fn loop_poll_locals_are_rebuilt_after_budget_yields() {
    prints(
        r#"package main
async func count() int {
    var sum = 0
    for var i = 0; i < 1024; i += 1 {
        let scratch = [int; 4]{i, i + 1, i + 2, i + 3}
        sum += scratch[0]
    }
    return sum
}
func main() { let task = go count(); println(task.wait()) }
"#,
        "523776\n",
    );
    prints(
        r#"package main
type Guard struct { Value int }
func (g mut Guard) drop() { println("drop") }
async func run() {
    for var i = 0; i < 260; i += 1 { let guard = Guard{Value: i} }
}
func main() { let task = go run(); task.wait() }
"#,
        &"drop\n".repeat(260),
    );
}

#[test]
fn async_budget_loops_with_ready_channels_stop_and_nested_calls_complete() {
    prints(
        r#"package main
async func churn(stop channel<bool>) int {
    let pulse = channel<int>(1)
    var iterations = 0
    for iterations < 1000000 {
        select { case let _, _ = stop.receive() { return iterations }; default {} }
        pulse.send(1)
        let _, _ = pulse.receive()
        iterations += 1
    }
    return iterations
}
async func signal(stop channel<bool>) { stop.send(true) }
async func leaf(value int) int { return value + 1 }
async func nested() int {
    var value = 0
    for value < 2048 { value = await leaf(value) }
    return value
}
func main() {
    let stop = channel<bool>(1)
    let worker = go churn(stop)
    let controller = go signal(stop)
    println(worker.wait() < 1000000)
    controller.wait()
    let recursive = go nested()
    println(recursive.wait())
}"#,
        "true\n2048\n",
    );
}

#[test]
fn async_state_machines_preserve_mutable_borrows_and_views_across_pending_children() {
    prints(
        r#"package main
import "zore/time"
func delayed(n int) int { time.Sleep(20); return n }
async func change(data mut [int; 2], t own Task<int>) {
    let n = await t
    data[1] += n
}
async func read(data []int, t own Task<int>) int {
    let n = await t
    return data[0] + n
}
async func process() int {
    var values = [int; 2]{10, 2}
    await change(values, go delayed(3))
    let view = values[:]
    let answer = await read(view, go delayed(6))
    return answer + values[1]
}
func main() { let t = go process(); println(t.wait()) }
"#,
        "21\n",
    );
}

#[test]
fn async_state_machines_evaluate_arguments_once_and_keep_loop_closures_alive() {
    prints(
        r#"package main
import "zore/time"
func delayed(n int) int { time.Sleep(2); return n }
func next(counter Mutex<int>) int {
    return counter.withLock(func(n mut int) int { n += 1; return n })
}
async func leaf(n int, t own Task<int>) int { return n + await t }
async func run(counter Mutex<int>) (int, string) {
    var total = 0
    var text = ""
    let append = func() { text = text + "x" }
    for var i = 0; i < 20; i += 1 {
        total += await leaf(next(counter), go delayed(1))
        append()
    }
    return total, text
}
func main() {
    let counter = mutex(0)
    let t = go run(counter)
    let total, text = t.wait()
    println(total)
    println(text.len())
    println(counter.withLock(func(n mut int) int { return n }))
}
"#,
        "230\n20\n20\n",
    );
}

#[test]
fn async_state_machines_drop_moved_and_partially_moved_resources_once() {
    prints(
        r#"package main
import "zore/time"
type Resource struct { Name string }
func (r mut Resource) drop() { println("drop " + r.Name) }
type Pair struct { First Resource; Second Resource }
func delayed() int { time.Sleep(20); return 7 }
async func consume(r own Resource, t own Task<int>) int { return await t }
async func run() int {
    let pair = Pair{First: Resource{Name: "first"}, Second: Resource{Name: "second"}}
    let value = await consume(pair.First, go delayed())
    println(pair.Second.Name)
    return value
}
func main() { let t = go run(); println(t.wait()) }
"#,
        "drop first\nsecond\ndrop second\n7\n",
    );
}

#[test]
fn async_state_machine_panic_runs_nested_cleanup_and_propagates_through_join() {
    let output = run(r#"package main
import "zore/time"
type Resource struct { Name string }
func (r mut Resource) drop() { println("drop " + r.Name) }
func delayed() int { time.Sleep(20); return 0 }
async func inner(r own Resource) int {
    let t = go delayed()
    let zero = await t
    return 1 / zero
}
async func outer() int {
    let guard = Resource{Name: "outer"}
    return await inner(Resource{Name: "inner"})
}
func main() {
    let guard = Resource{Name: "main"}
    let t = go outer()
    println(t.wait())
}
"#);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "drop inner\ndrop outer\ndrop main\n");
    assert!(stderr(&output).contains("panic in task 1: division by zero"));
    assert!(stderr(&output).contains("panic in the main task: division by zero"));
}

#[test]
fn async_state_machines_propagate_errors_and_return_move_values() {
    prints(
        r#"package main
import "zore/time"
func delayed(ok bool) (int, error) {
    time.Sleep(10)
    if !ok { return 0, error("failed") }
    return 5, nil
}
async func values(ok bool) (Array<string>, error) {
    let guard = "kept" + " across await"
    let t = go delayed(ok)
    let n = await t?
    return Array<string>{guard, "result"}, nil
}
func main() {
    let a = go values(true)
    let result, err = a.wait()
    println(err == nil)
    println(result[0])
    let b = go values(false)
    let empty, failure = b.wait()
    println(empty.len())
    println(failure != nil)
}
"#,
        "true\nkept across await\n0\ntrue\n",
    );
}

#[test]
fn async_state_machines_handle_recursion_and_nil_task_panics() {
    prints(
        r#"package main
async func recurse(n int) int {
    if n == 0 { return 1 }
    return n + await recurse(n - 1)
}
func main() { let t = go recurse(100); println(t.wait()) }
"#,
        "5051\n",
    );
    prints(
        r#"package main
async func recurse(n int) int {
    if n == 0 { return 0 }
    return 1 + await recurse(n - 1)
}
func main() { let t = go recurse(300); println(t.wait()) }
"#,
        "300\n",
    );
    let output = run(r#"package main
async func fail() int { var t Task<int> = nil; return await t }
func main() { let t = go fail(); println(t.wait()) }
"#);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("wait on a nil task"));
}

#[test]
fn async_state_machines_can_await_plain_tasks_after_channel_suspension() {
    prints(
        r#"package main
import "zore/time"
func delayed() string { time.Sleep(10); return "ready" }
async func polled() string { let t = go delayed(); return await t }
async func fallback(ch channel<bool>) string {
    let _, _ = ch.receive()
    return await polled()
}
func main() {
    let ch = channel<bool>(1)
    ch.send(true)
    let t = go fallback(ch)
    println(t.wait())
}
"#,
        "ready\n",
    );
}

#[test]
fn plain_tasks_join_polled_children_and_mix_with_async_channel_waiters() {
    let output = run_channel_poll_bounded(
        r#"package main
import "zore/time"
async func child(value int) int { time.Sleep(1); return value }
func parent(value int) int { let task = go child(value); return task.wait() }
func plain(ch channel<int>, ready channel<bool>) int {
    ready.send(true)
    let n, _ = ch.receive()
    return parent(n)
}
async func polled(ch channel<int>, ready channel<bool>) int {
    ready.send(true)
    let n, _ = ch.receive()
    let task = go parent(n)
    return await task
}
func main() {
    let ch = channel<int>()
    let ready = channel<bool>()
    var tasks = Array<Task<int>>{}
    for var i = 0; i < 32; i += 1 {
        if i % 2 == 0 { tasks.push(go plain(ch, ready)) } else { tasks.push(go polled(ch, ready)) }
    }
    for var i = 0; i < 32; i += 1 { let _, _ = ready.receive() }
    for var i = 1; i <= 32; i += 1 { ch.send(i) }
    var total = 0
    for tasks.len() > 0 {
        let found, task = tasks.pop()
        if found { total += task.wait() }
    }
    println(total)
}
"#,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "528\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_state_machine_task_panics_are_reported_at_both_task_boundaries() {
    let output = run(r#"package main
import "zore/time"
type Resource struct { Name string }
func (r mut Resource) drop() { println("drop " + r.Name) }
func delayed() int { time.Sleep(20); return 0 }
async func fail(r own Resource) int {
    let t = go delayed()
    let zero = await t
    return 1 / zero
}
async func outer() int {
    let guard = Resource{Name: "outer"}
    let t = go fail(Resource{Name: "inner"})
    return await t
}
func main() {
    let guard = Resource{Name: "main"}
    let t = go outer()
    println(t.wait())
}
"#);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "drop inner\ndrop outer\ndrop main\n");
    let report = stderr(&output);
    assert!(
        report.contains("panic in task 2: division by zero"),
        "{report}"
    );
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
fn async_state_machines_drop_detached_results_and_keep_work_running() {
    prints(
        r#"package main
import "zore/time"
type Resource struct { Name string; Done channel<bool> }
func (r mut Resource) drop() { println("drop " + r.Name); r.Done.send(true) }
func delayed() int { time.Sleep(10); return 7 }
async func produce(done channel<bool>) Resource {
    let t = go delayed()
    let value = await t
    return Resource{Name: "detached", Done: done}
}
func main() {
    let done = channel<bool>()
    go produce(done)
    let _, _ = done.receive()
    println("finished")
}
"#,
        "drop detached\nfinished\n",
    );
}

#[test]
fn thousands_of_polled_tasks_can_suspend_on_each_other_without_private_stacks() {
    prints(
        r#"package main
async func seed() int { return 1 }
async func add(t own Task<int>) int { return 1 + await t }
func main() {
    var t = go seed()
    for var i = 0; i < 5000; i += 1 { t = go add(t) }
    println(t.wait())
}
"#,
        "5001\n",
    );
}

#[test]
fn async_state_machine_join_deadlocks_are_still_reported() {
    let output = run(r#"package main
func blocked(ch channel<int>) int { let n, _ = ch.receive(); return n }
async func wait() int { let t = go blocked(channel<int>()); return await t }
func main() { let t = go wait(); println(t.wait()) }
"#);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("all tasks are asleep"));
}

fn channel_poll_parity(source: &str, expected: &str) {
    for mode in ["", "async "] {
        let source = source
            .replace("MODE ", mode)
            .replace("AWAIT ", if mode.is_empty() { "" } else { "await " });
        let mut source = source;
        for name in ["writer", "task", "t"] {
            source = source.replace(
                &format!("JOIN {name}"),
                &if mode.is_empty() {
                    format!("{name}.wait()")
                } else {
                    format!("await {name}")
                },
            );
        }
        let mut sources = SourceMap::new();
        let id = sources.add("channels.ore", source.clone()).unwrap();
        let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
        if !mode.is_empty() {
            assert!(ir.contains("run$async$poll"));
            assert!(ir.contains("call ptr @zore_channel_start"));
            assert!(ir.contains("call i8 @zore_channel_poll"));
            for function in ir.split("define private i8 ").skip(1) {
                let function = function.split("\n}\n").next().unwrap();
                if function.contains("$async$poll") {
                    assert!(!function.contains("call void @zore_channel_send("));
                    assert!(!function.contains("call zeroext i1 @zore_channel_receive("));
                    assert!(!function.contains("call i64 @zore_select("));
                }
            }
        }
        let output = run_channel_poll_bounded(&source);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        assert_eq!(stdout(&output), expected);
        assert!(output.stderr.is_empty(), "{}", stderr(&output));
    }
}

fn run_channel_poll_bounded(source: &str) -> Output {
    run_poll_bounded(source, None)
}

fn run_poll_bounded(source: &str, input: Option<&[u8]>) -> Output {
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("program");
    build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let mut child = Command::new(&executable)
        .env("ZORE_CHECK_LEAKS", "1")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("poll program timed out: {}", stderr(&output));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn channel_poll_unbuffered_and_buffered_loops_match_plain_execution() {
    channel_poll_parity(
        r#"package main
MODE func produce(ch channel<int>, count int) int {
    for var i = 1; i <= count; i += 1 { ch.send(i) }
    ch.close()
    return 0
}
MODE func total(ch channel<int>) int {
    var sum = 0
    for { let value, ok = ch.receive(); if !ok { return sum }; sum += value }
}
MODE func run() int {
    var answer = 0
    for var capacity = 0; capacity < 4; capacity += 1 {
        let ch = channel<int>(capacity)
        let reader = go total(ch)
        let writer = go produce(ch, 500)
        _ = JOIN writer
        answer += AWAIT totalTask(reader)
    }
    return answer
}
MODE func totalTask(t own Task<int>) int { return JOIN t }
func main() { let t = go run(); println(t.wait()) }
"#,
        "501000\n",
    );
}

#[test]
fn channel_poll_select_ready_default_zero_and_move_cleanup_match_plain_execution() {
    channel_poll_parity(
        r#"package main
type Job struct { Name string }
func (j mut Job) drop() { println("drop " + j.Name) }
MODE func run() {
    let full = channel<Job>(1)
    let free = channel<Job>(1)
    full.send(Job{Name: "old"})
    select {
        case full.send(Job{Name: "skip"}) { println("wrong") }
        case free.send(Job{Name: "chosen"}) { println("sent") }
    }
    { let job, ok = free.receive(); println(job.Name); println(ok) }
    { let job, _ = full.receive(); println(job.Name) }
    select { case let job, ok = free.receive() {}; default { println("default") } }
    free.close()
    select { case let job, ok = free.receive() { println(ok) } }
    let holder = channel<channel<int>>(1)
    holder.close()
    let zero, _ = holder.receive()
    select { case let n, ok = zero.receive() { println(n); println(ok) } }
}
func main() { let t = go run(); t.wait() }
"#,
        "drop skip\nsent\nchosen\ntrue\ndrop chosen\nold\ndrop old\ndefault\nfalse\ndrop \n0\nfalse\n",
    );
}

#[test]
fn channel_poll_select_pending_duplicate_channels_and_single_evaluation_match_plain() {
    channel_poll_parity(
        r#"package main
type Job struct { Name string; Count Mutex<int> }
func (j mut Job) drop() { j.Count.withLock(func(n mut int) { n += 1 }) }
func make(counter Mutex<int>, name string) Job {
    counter.withLock(func(n mut int) { n += 1 })
    return Job{Name: name, Count: counter}
}
func count(counter Mutex<int>) int { return counter.withLock(func(n mut int) int { return n }) }
MODE func receive(ch channel<Job>) int {
    let job, ok = ch.receive()
    println((job.Name == "first") || (job.Name == "second"))
    println(ok)
    return 0
}
MODE func run() {
    let counter = mutex(0)
    let ch = channel<Job>()
    let task = go receive(ch)
    select {
        case ch.send(make(counter, "first")) {}
        case ch.send(make(counter, "second")) {}
    }
    _ = JOIN task
    println(count(counter))
    select { case let job, ok = ch.receive() {}; default { println("clear") } }
}
func main() { let t = go run(); t.wait() }
"#,
        "true\ntrue\n4\nclear\n",
    );
}
#[test]
fn channel_poll_nested_calls_keep_mutable_borrows_and_partial_moves_alive() {
    channel_poll_parity(
        r#"package main
type Parts struct { A Array<int>; B Array<int> }
MODE func change(ch channel<int>, data mut [int; 2]) {
    let n, _ = ch.receive()
    data[1] += n
}
MODE func push(ch channel<int>) int { ch.send(9); return 0 }
MODE func run() int {
    var parts = Parts{A: Array<int>{2}, B: Array<int>{3}}
    let moved = parts.A
    let ch = channel<int>()
    let task = go push(ch)
    var values = [int; 2]{moved[0], parts.B[0]}
    AWAIT change(ch, values)
    _ = JOIN task
    return values[0] + values[1]
}
func main() { let t = go run(); println(t.wait()) }
"#,
        "14\n",
    );
}

#[test]
fn channel_poll_internal_waits_preserve_deadlock_detection() {
    for body in [
        "ch.send(1)",
        "let _, _ = ch.receive()",
        "select { case let n, ok = ch.receive() {}; case ch.send(1) {} }",
    ] {
        let source = format!(
            "package main\nasync func run() {{ let ch = channel<int>(); {body} }}\nfunc main() {{ let t = go run(); t.wait() }}"
        );
        assert_deadlock(&run_channel_poll_bounded(&source));
    }
}

#[test]
fn channel_poll_send_panic_drops_unsent_values_and_unwinds_parent_frames() {
    for send in [
        "ch.send(Job{Name: \"lost\"})",
        "select { case ch.send(Job{Name: \"lost\"}) {}; case other.send(Job{Name: \"skip\"}) {} }",
    ] {
        let source = format!(
            "package main\ntype Job struct {{ Name string }}\nfunc (j mut Job) drop() {{ println(\"drop \" + j.Name) }}\nasync func run(ch channel<Job>) {{ let local = Job{{Name: \"local\"}}; let other = channel<Job>(); {send} }}\nfunc main() {{ let ch = channel<Job>(); let t = go run(ch); ch.close(); t.wait() }}"
        );
        let output = run_channel_poll_bounded(&source);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("send on a closed channel"));
        let text = stdout(&output);
        assert_eq!(text.matches("drop lost\n").count(), 1);
        assert_eq!(text.matches("drop local\n").count(), 1);
        assert_eq!(
            text.matches("drop skip\n").count(),
            usize::from(send.starts_with("select"))
        );
    }
}

#[test]
fn channel_poll_select_drops_unchosen_values_in_reverse_source_order() {
    channel_poll_parity(
        r#"package main
type Job struct { Name string }
func (j mut Job) drop() { println("drop " + j.Name) }
MODE func run() {
    let ready = channel<Job>(1)
    let blocked = channel<Job>()
    select {
        case ready.send(Job{Name: "chosen"}) { println("body") }
        case blocked.send(Job{Name: "second"}) {}
        case blocked.send(Job{Name: "third"}) {}
    }
}
func main() { let t = go run(); t.wait() }
"#,
        "drop third\ndrop second\nbody\ndrop chosen\n",
    );
}

#[test]
fn channel_poll_mixes_plain_and_async_waiters_in_both_directions() {
    channel_poll_parity(
        r#"package main
func plain(ch channel<int>) { for var i = 1; i <= 200; i += 1 { ch.send(i) }; ch.close() }
MODE func produce(ch channel<int>) { for var i = 1; i <= 200; i += 1 { ch.send(i) }; ch.close() }
MODE func run(ch channel<int>) int {
    var sum = 0
    for {
        select { case let n, ok = ch.receive() { if !ok { return sum }; sum += n } }
    }
}
func main() {
    let a = channel<int>()
    let reader = go run(a)
    let writer = go plain(a)
    println(reader.wait())
    writer.wait()
    let b = channel<int>()
    let producer = go produce(b)
    var sum = 0
    for { let n, ok = b.receive(); if !ok { break }; sum += n }
    producer.wait()
    println(sum)
}
"#,
        "20100\n20100\n",
    );
}

#[test]
fn channel_poll_thousands_of_tasks_wait_without_private_stacks() {
    let output = run_channel_poll_bounded(
        r#"package main
async func run(ch channel<int>) int { let n, _ = ch.receive(); return n }
func main() {
    let ch = channel<int>()
    var tasks = Array<Task<int>>{}
    for var i = 0; i < 5000; i += 1 { tasks.push(go run(ch)) }
    for var i = 0; i < 5000; i += 1 { ch.send(1) }
    var total = 0
    for tasks.len() > 0 { let ok, t = tasks.pop(); if ok { total += t.wait() } }
    println(total)
}
"#,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "5000\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

fn timer_poll_parity(source: &str, expected: &str) {
    for mode in ["", "async "] {
        let source = source
            .replace("MODE ", mode)
            .replace("AWAIT ", if mode.is_empty() { "" } else { "await " });
        let mut sources = SourceMap::new();
        let id = sources.add("timers.ore", source.clone()).unwrap();
        let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
        if source.contains("time.After") {
            assert!(ir.contains("zore/time.fire$async$poll"));
        }
        if !mode.is_empty() {
            assert!(ir.contains("run$async$poll"));
            assert!(ir.contains("call ptr @zore_native_time_sleep_start"));
            assert!(ir.contains("call i8 @zore_reactor_poll"));
            for function in ir.split("define private i8 ").skip(1) {
                let function = function.split("\n}\n").next().unwrap();
                if function.contains("$async$poll") {
                    assert!(!function.contains("call void @\"main.zore/time.Sleep\""));
                    assert!(!function.contains("call void @zore_native_time_sleep("));
                }
            }
        }
        let output = run_channel_poll_bounded(&source);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        assert_eq!(stdout(&output), expected);
        assert!(output.stderr.is_empty(), "{}", stderr(&output));
    }
}

#[test]
fn timer_poll_positive_zero_and_negative_durations_match_plain_execution() {
    timer_poll_parity(
        r#"package main
import "zore/time"
MODE func run() {
    let start = time.Millis()
    time.Sleep(25)
    println(time.Millis() - start >= 25)
    time.Sleep(0)
    time.Sleep(-5)
    time.Sleep(-9223372036854775808)
    println(time.Millis() >= start)
}
func main() { let t = go run(); t.wait() }
"#,
        "true\ntrue\n",
    );
}

#[test]
fn timer_poll_nested_calls_preserve_views_mutable_borrows_and_move_cleanup() {
    timer_poll_parity(
        r#"package main
import "zore/time"
type Job struct { Name string }
func (j mut Job) drop() { println("drop " + j.Name) }
MODE func change(data mut [int; 2], job own Job) {
    let view = data[:]
    time.Sleep(5)
    println(view[0])
    data[1] += 3
}
MODE func run() {
    var data = [int; 2]{4, 2}
    let job = Job{Name: "child"}
    AWAIT change(data, job)
    println(data[1])
}
func main() { let t = go run(); t.wait() }
"#,
        "4\ndrop child\n5\n",
    );
}

#[test]
fn timer_poll_evaluates_sleep_arguments_once_across_loop_resumptions() {
    timer_poll_parity(
        r#"package main
import "zore/time"
func next(counter Mutex<int>) int { return counter.withLock(func(n mut int) int { n += 1; return 1 }) }
func count(counter Mutex<int>) int { return counter.withLock(func(n mut int) int { return n }) }
MODE func run(counter Mutex<int>) int {
    var total = 0
    for var i = 0; i < 30; i += 1 { time.Sleep(next(counter)); total += i }
    return total
}
func main() { let counter = mutex(0); let t = go run(counter); println(t.wait()); println(count(counter)) }
"#,
        "435\n30\n",
    );
}

#[test]
fn timer_poll_after_select_and_internal_waits_are_not_false_deadlocks() {
    timer_poll_parity(
        r#"package main
import "zore/time"
MODE func produce(ch channel<int>, ms int) { time.Sleep(ms); ch.send(7); ch.close() }
MODE func run() {
    let ch = channel<int>()
    let t = go produce(ch, 10)
    let n, ok = ch.receive()
    println(n)
    println(ok)
    let _, more = ch.receive()
    println(more)
    let timer = time.After(10)
    select { case let fired, open = timer.receive() { println(fired); println(open) } }
    let _, stillOpen = timer.receive()
    println(stillOpen)
    select { case let fired, open = time.After(0).receive() { println(fired) } }
}
func main() { let t = go run(); t.wait() }
"#,
        "7\ntrue\nfalse\ntrue\ntrue\nfalse\ntrue\n",
    );
}

#[test]
fn timer_poll_panic_after_resumption_unwinds_owned_values_once() {
    for mode in ["", "async "] {
        let source = format!(
            r#"package main
import "zore/time"
type Job struct {{ Name string }}
func (j mut Job) drop() {{ println("drop " + j.Name) }}
{mode}func run() int {{ let job = Job{{Name: "live"}}; time.Sleep(5); var zero = 0; return 1 / zero }}
func main() {{ let t = go run(); println(t.wait()) }}
"#
        );
        let output = run_channel_poll_bounded(&source);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(stdout(&output), "drop live\n");
        assert!(stderr(&output).contains("division by zero"));
    }
}

#[test]
fn timer_poll_many_sleeping_tasks_and_plain_helpers_preserve_progress() {
    timer_poll_parity(
        r#"package main
import "zore/time"
func helper() { time.Sleep(1) }
MODE func run() int { time.Sleep(10); helper(); return 1 }
func main() {
    var tasks = Array<Task<int>>{}
    for var i = 0; i < 2000; i += 1 { tasks.push(go run()) }
    var total = 0
    for tasks.len() > 0 { let ok, t = tasks.pop(); if ok { total += t.wait() } }
    println(total)
}
"#,
        "2000\n",
    );
}

#[test]
fn timer_poll_then_internal_wait_still_detects_deadlock() {
    let output = run_channel_poll_bounded(
        r#"package main
import "zore/time"
async func run() { time.Sleep(1); let ch = channel<int>(); ch.send(1) }
func main() { let t = go run(); t.wait() }
"#,
    );
    assert_deadlock(&output);
}

#[test]
fn timer_poll_detached_result_is_destroyed_after_sleep_completion() {
    let output = run_channel_poll_bounded(
        r#"package main
import "zore/time"
type Job struct { Done channel<bool> }
func (j mut Job) drop() { j.Done.send(true) }
async func run(done channel<bool>) Job { time.Sleep(1); return Job{Done: done} }
func main() {
    let done = channel<bool>()
    go run(done)
    let dropped, _ = done.receive()
    println(dropped)
}
"#,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "true\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

fn mutex_poll_parity(source: &str, expected: &str) {
    for mode in ["", "async "] {
        let source = source.replace("MODE ", mode);
        let mut sources = SourceMap::new();
        let id = sources.add("mutex.ore", source.clone()).unwrap();
        let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
        if !mode.is_empty() {
            assert!(ir.contains("run$async$poll"));
            let mut found = false;
            for function in ir.split("define private i8 ").skip(1) {
                let function = function.split("\n}\n").next().unwrap();
                if function.lines().next().unwrap().contains("run$async$poll") {
                    found = true;
                    assert!(function.contains("call ptr @zore_mutex_start"));
                    assert!(function.contains("call i8 @zore_mutex_poll"));
                    assert!(!function.contains("call ptr @zore_mutex_lock("));
                }
            }
            assert!(found);
        }
        let output = run_channel_poll_bounded(&source);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(stdout(&output), expected);
    }
}

#[test]
fn mutex_poll_callbacks_block_with_compensation_and_return_move_results() {
    mutex_poll_parity(
        r#"package main
MODE func run(m Mutex<Array<int>>, started channel<bool>, values channel<int>) Array<int> {
    return m.withLock(func(items mut Array<int>) Array<int> {
        started.send(true)
        let n, _ = values.receive()
        items.push(n)
        return clone(items)
    })
}
func produce(started channel<bool>, values channel<int>) { let _, _ = started.receive(); values.send(9) }
func main() {
    let m = mutex(Array<int>{1})
    let started = channel<bool>()
    let values = channel<int>()
    let producer = go produce(started, values)
    let t = go run(m, started, values)
    let result = t.wait()
    producer.wait()
    println(result.len())
    println(result[1])
}
"#,
        "2\n9\n",
    );
}

#[test]
fn mutex_poll_repeated_contention_preserves_callback_results_and_cleanup() {
    mutex_poll_parity(
        r#"package main
import "zore/time"
MODE func run(m Mutex<int>) int {
    var result = 0
    for var i = 0; i < 20; i += 1 {
        result = m.withLock(func(n mut int) int { time.Sleep(1); n += 1; return n })
    }
    return result
}
func main() {
    let m = mutex(0)
    var tasks = Array<Task<int>>{}
    for var i = 0; i < 8; i += 1 { tasks.push(go run(m)) }
    for tasks.len() > 0 { let ok, t = tasks.pop(); if ok { let _ = t.wait() } }
    println(m.withLock(func(n mut int) int { return n }))
}
"#,
        "160\n",
    );
}

#[test]
fn mutex_poll_zero_and_callback_panic_preserve_unwind_cleanup() {
    for source in [
        r#"package main
async func run() int { let holder = channel<Mutex<int>>(); holder.close(); let m, _ = holder.receive(); return m.withLock(func(n mut int) int { println("unreachable"); return n }) }
func main() { let t = go run(); println(t.wait()) }"#,
        r#"package main
async func run(m Mutex<int>) int { return m.withLock(func(n mut int) int { return 1 / n }) }
func main() { let m = mutex(0); let t = go run(m); println(t.wait()) }"#,
    ] {
        let output = run_channel_poll_bounded(source);
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert!(!stdout(&output).contains("unreachable"));
        assert!(stderr(&output).contains("panic"));
    }
}

#[test]
fn mutex_poll_nonreentrant_callback_still_detects_deadlock() {
    let output = run_channel_poll_bounded(
        r#"package main
async func run(m Mutex<int>) {
    m.withLock(func(n mut int) { m.withLock(func(inner mut int) { inner += n }) })
}
func main() { let t = go run(mutex(1)); t.wait() }
"#,
    );
    assert_deadlock(&output);
}

#[test]
fn mutex_poll_callback_error_results_preserve_captures_and_drop_once() {
    mutex_poll_parity(
        r#"package main
type Marker struct { Name string }
func (m mut Marker) drop() { println("drop " + m.Name) }
MODE func run(m Mutex<string>) (int, error) {
    let marker = Marker{Name: "frame"}
    let offset = 5
    return m.withLock(func(text mut string) (int, error) {
        if text.len() == 0 { return 0, error("empty") }
        return text.len() + offset, nil
    })
}
func main() {
    let t = go run(mutex("1234567"))
    let number, err = t.wait()
    println(number)
    println(err == nil)
}
"#,
        "drop frame\n12\ntrue\n",
    );
}

fn io_poll_parity(source: &str, expected: &str) {
    for mode in ["", "async "] {
        let source = source
            .replace("MODE ", mode)
            .replace("JOIN ", if mode.is_empty() { "" } else { "await " });
        let source = if mode.is_empty() {
            source.replace("taskJoin", "t.wait()")
        } else {
            source.replace("taskJoin", "t")
        };
        let mut sources = SourceMap::new();
        let id = sources.add("io.ore", source.clone()).unwrap();
        let ir = emit_llvm(sources.file(id).unwrap()).unwrap();
        if !mode.is_empty() {
            assert!(ir.contains("run$async$poll"));
            let mut found_run = false;
            for function in ir.split("define private i8 ").skip(1) {
                let function = function.split("\n}\n").next().unwrap();
                if function.lines().next().unwrap().contains("run$async$poll") {
                    found_run = true;
                    assert!(function.contains("ret i8 0"));
                }
            }
            assert!(found_run);
        }
        let output = run_channel_poll_bounded(&source);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(stdout(&output), expected);
    }
}

#[test]
fn io_poll_files_and_byte_views_preserve_results_and_cleanup() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("poll.dat").display().to_string();
    let source = r#"package main
import "zore/os"
import "zore/strings"
MODE func run(path string) Array<byte> {
    let first = os.WriteFile(path, "héllo")
    println(first == nil)
    let text, err = os.ReadFile(path)
    println(err == nil)
    println(text)
    let data = strings.Bytes(text)
    println(os.WriteBytes(path, data[:]) == nil)
    let read, readErr = os.ReadBytes(path)
    println(readErr == nil)
    return read
}
func main() { let t = go run("PATH"); let bytes = t.wait(); println(bytes.len()) }
"#
    .replace("PATH", &path);
    io_poll_parity(&source, "true\ntrue\nhéllo\ntrue\ntrue\n6\n");
}

#[test]
fn io_poll_socket_wrappers_suspend_and_echo_split_characters() {
    io_poll_parity(
        r#"package main
import "zore/net"
import "zore/strconv"
MODE func run(conn own net.Conn) int {
    var count = 0
    for {
        let text, err = conn.Read(1)
        if err != nil { return count }
        if conn.Write(text) != nil { return -1 }
        count += text.len()
    }
}
MODE func serve(listener own net.Listener) int {
    let conn, err = listener.Accept()
    if err != nil { return -1 }
    let t = go run(conn)
    return JOIN taskJoin
}
MODE func client(port int) string {
    let conn, err = net.Dial("127.0.0.1:" + strconv.Itoa(port))
    if err != nil { return "dial failed" }
    if conn.Write("héllo ✓") != nil { return "write failed" }
    _ = conn.CloseWrite()
    var result = ""
    for { let text, failure = conn.Read(1); if failure != nil { return result }; result += text }
}
func main() {
    let listener, err = net.Listen("127.0.0.1:0")
    if err != nil { return }
    let port = listener.Port()
    let server = go serve(listener)
    let t = go client(port)
    println(t.wait())
    println(server.wait())
}
"#,
        "héllo ✓\n10\n",
    );
}

#[test]
fn io_poll_stdin_preserves_line_errors_and_eof() {
    for mode in ["", "async "] {
        let source = r#"package main
import "zore/io"
MODE func run() int {
    let _, bad = io.ReadLine()
    println(bad == error("io.ReadLine: invalid UTF-8"))
    let text, ok = io.ReadLine()
    println(text)
    println(ok == nil)
    let last, _ = io.ReadLine()
    println(last)
    let _, end = io.ReadLine()
    println(end == error("EOF"))
    return 1
}
func main() { let t = go run(); println(t.wait()) }
"#
        .replace("MODE ", mode);
        let output = run_poll_bounded(&source, Some(b"\xff\nok\r\nlast"));
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(stdout(&output), "true\nok\ntrue\nlast\ntrue\n1\n");
    }
}

#[test]
fn io_poll_file_errors_and_question_mark_run_frame_cleanup_once() {
    let dir = TempDir::new().unwrap();
    let good = dir.path().join("good.txt");
    let bad = dir.path().join("bad.txt");
    std::fs::write(&good, "abc").unwrap();
    std::fs::write(&bad, [0xff]).unwrap();
    let source = r#"package main
import "zore/os"
type Marker struct { Name string }
func (m mut Marker) drop() { println("drop " + m.Name) }
MODE func run(path string) (int, error) {
    let marker = Marker{Name: "frame"}
    let text = os.ReadFile(path)?
    return text.len(), nil
}
func main() {
    let good = go run("GOOD")
    let n, ok = good.wait()
    println(n)
    println(ok == nil)
    let bad = go run("BAD")
    let zero, err = bad.wait()
    println(zero)
    println(err == error("os.ReadFile: invalid UTF-8"))
}
"#
    .replace("GOOD", &good.display().to_string())
    .replace("BAD", &bad.display().to_string());
    io_poll_parity(&source, "drop frame\n3\ntrue\ndrop frame\n0\ntrue\n");
}

#[test]
fn io_poll_socket_timeouts_and_byte_results_match_plain_execution() {
    io_poll_parity(
        r#"package main
import "zore/net"
import "zore/strconv"
MODE func run(listener own net.Listener) int {
    _ = listener.SetTimeout(10)
    let _, timed = listener.Accept()
    println(timed == error("net.Accept: timed out"))
    return 1
}
MODE func echo(conn own net.Conn) int {
    let data, err = conn.ReadBytes(1)
    if err == nil { _ = conn.WriteBytes(data[:]) }
    return 0
}
MODE func serve(listener own net.Listener) int {
    let conn, err = listener.Accept()
    if err == nil { let t = go echo(conn); return JOIN taskJoin }
    return -1
}
MODE func client(port int) int {
    let conn, err = net.DialTimeout("127.0.0.1:" + strconv.Itoa(port), 1000)
    if err != nil { return -1 }
    let data = Array<byte>{255}
    println(conn.WriteBytes(data[:]) == nil)
    let reply, ok = conn.ReadBytes(1)
    println(ok == nil)
    return int(reply[0])
}
func main() {
    let timer, _ = net.Listen("127.0.0.1:0")
    let wait = go run(timer)
    println(wait.wait())
    let listener, err = net.Listen("127.0.0.1:0")
    if err != nil { return }
    let port = listener.Port()
    let server = go serve(listener)
    let t = go client(port)
    println(t.wait())
    server.wait()
}
"#,
        "true\n1\ntrue\ntrue\n255\n",
    );
}

#[test]
fn io_poll_cancel_library_wrappers_suspend_without_changing_their_api() {
    io_poll_parity(
        r#"package main
import "zore/cancel"
MODE func run() bool {
    let token = cancel.WithTimeout(10)
    let completed = token.Sleep(1000)
    println(completed)
    return token.Cancelled()
}
func main() { let t = go run(); println(t.wait()) }
"#,
        "false\ntrue\n",
    );
}

#[test]
fn io_poll_panic_after_helper_completion_drops_live_frame_values_once() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("panic.txt");
    std::fs::write(&path, "owned text").unwrap();
    let source = r#"package main
import "zore/os"
type Marker struct { Name string }
func (m mut Marker) drop() { println("drop " + m.Name) }
async func run(path string) int {
    let marker = Marker{Name: "frame"}
    let text, _ = os.ReadFile(path)
    let zero = text.len() - text.len()
    return 1 / zero
}
func main() { let t = go run("PATH"); println(t.wait()) }
"#
    .replace("PATH", &path.display().to_string());
    let output = run_channel_poll_bounded(&source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "drop frame\n");
    assert!(stderr(&output).contains("division by zero"));
    assert!(!stderr(&output).contains("leak:"));
}

#[test]
fn cancellation_background_tasks_scale_as_async_frames() {
    let output = run_channel_poll_bounded(
        r#"package main
import "zore/cancel"
func main() {
    let parent = cancel.WithTimeout(200)
    var children = Array<cancel.Token>{}
    for var i = 0; i < 1000; i += 1 { children.push(parent.Child()) }
    var total = 0
    for children.len() > 0 {
        let found, child = children.pop()
        if found {
            let _, _ = child.Done().receive()
            if child.Cancelled() { total += 1 }
        }
    }
    println(total)
    println(parent.Cancelled())
}
"#,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "1000\ntrue\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn declared_functions_run_as_function_values() {
    prints(
        "package main

import \"zore/strings\"

func add(a int, b int) int { return a + b }

func double(values mut []int) {
    for var i = 0; i < values.len(); i += 1 {
        values[i] = values[i] * 2
    }
}

func apply(op func(int, int) int, x int, y int) int {
    return op(x, y)
}

func pick() func(int, int) int {
    println(\"pick\")
    return add
}

func left() int {
    println(\"left\")
    return 1
}

func right() int {
    println(\"right\")
    return 2
}

type Table struct {
    op func(int, int) int
}

func main() {
    let f = add
    println(f(1, 2))
    println(apply(add, 2, 3))
    println(pick()(left(), right()))
    var data = Array<int>{1, 2, 3}
    let d = double
    d(data[:])
    println(data[0] + data[1] + data[2])
    let up = strings.Upper
    println(up(\"ok\"))
    let table = Table{op: add}
    let stored = table.op
    println(stored(10, 20))
    var ops = Array<func(int, int) int>{}
    ops.push(add)
    ops.push(add)
    println(ops.len())
}
",
        "3\n5\npick\nleft\nright\n3\n12\nOK\n30\n2\n",
    );
}

#[test]
fn spawned_closures_own_their_captures_and_destroy_them_once() {
    prints(
        "package main

type Res struct { Name string }

func (r mut Res) drop() { println(\"drop \" + r.Name) }

func use(r own Res) { println(\"use \" + r.Name) }

func left() int {
    println(\"left\")
    return 1
}

func right() int {
    println(\"right\")
    return 2
}

func main() {
    let kept = Res{Name: \"kept\"}
    let a = go func() int {
        println(\"run \" + kept.Name)
        return 1
    }()
    println(a.wait())

    let moved = Res{Name: \"moved\"}
    let b = go func() { use(moved) }()
    b.wait()

    let failing = Res{Name: \"failing\"}
    let c = go func() error {
        println(\"run \" + failing.Name)
        return error(\"failed\")
    }()
    println(c.wait() != nil)

    let add = func(x int, y int) int { return x + y }
    let d = go add(left(), right())
    println(d.wait())

    let idle = Res{Name: \"idle\"}
    let never = func() { println(idle.Name) }
    println(\"scope end\")
}
",
        "run kept\ndrop kept\n1\nuse moved\ndrop moved\nrun failing\ndrop failing\ntrue\nleft\nright\n3\nscope end\ndrop idle\n",
    );
}

#[test]
fn a_panic_in_a_spawned_closure_destroys_its_environment_once() {
    let source = "package main

type Res struct { Name string }

func (r mut Res) drop() { println(\"drop \" + r.Name) }

func main() {
    let held = Res{Name: \"held\"}
    let zero = 0
    println(\"before\")
    let t = go func() int {
        println(\"working \" + held.Name)
        return 1 / zero
    }()
    let v = t.wait()
    println(\"unreachable\")
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "before\nworking held\ndrop held\n");
    assert!(
        stderr(&output).contains("panic in task 1: division by zero"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_panic_in_an_argument_destroys_the_evaluated_callable_and_spawns_nothing() {
    let source = "package main

type Res struct { Name string }

func (r mut Res) drop() { println(\"drop \" + r.Name) }

func boom() int {
    let zero = 0
    return 1 / zero
}

func main() {
    let held = Res{Name: \"callee\"}
    let job = func(n int) int {
        println(\"never runs \" + held.Name)
        return n
    }
    println(\"before\")
    let t = go job(boom())
    println(\"unreachable\")
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "before\ndrop callee\n");
}

#[test]
fn go_runs_function_values_call_once_closures_and_owning_arguments() {
    prints(
        "package main

type Job struct { Id int }

func (j mut Job) drop() { println(\"drop \" + str(j.Id)) }

func str(n int) string {
    if n == 1 { return \"1\" }
    if n == 2 { return \"2\" }
    return \"3\"
}

func consume(j own Job) { println(\"consume \" + str(j.Id)) }

func twice(n int) int { return n * 2 }

func runWorker(handler own func() int) int { return handler() + 1 }

func main() {
    let f = twice
    let first = go f(21)
    println(first.wait())

    let job = Job{Id: 1}
    let finish = func() { consume(job) }
    let t = go finish()
    t.wait()

    let owned = Job{Id: 2}
    let handler = func() int { return owned.Id * 10 }
    let u = go runWorker(handler)
    println(u.wait())
}
",
        "42\nconsume 1\ndrop 1\ndrop 2\n21\n",
    );
}

#[test]
fn spawned_closures_run_as_plain_tasks_even_inside_async_functions() {
    prints(
        "package main

async func parent(id int, ch channel<int>) int {
    let doubled = go func() int { return id * 2 }()
    let blocking = go func() int {
        let v, ok = ch.receive()
        return v
    }()
    ch.send(4)
    return await doubled + await blocking
}

func main() {
    let ch = channel<int>(1)
    let p = go parent(5, ch)
    println(p.wait())
}
",
        "14\n",
    );
}

#[test]
fn a_detached_spawned_closure_runs_without_a_handle() {
    prints(
        "package main

func main() {
    let done = channel<int>(1)
    go func() { done.send(5) }()
    let v, ok = done.receive()
    println(v)
}
",
        "5\n",
    );
}

#[test]
fn method_values_call_the_method_on_the_captured_receiver() {
    prints(
        "package main

type Counter struct { N int }

func (c Counter) read() int { return c.N }
func (c mut Counter) bump() { c.N += 1 }
func (c own Counter) finish() int { return c.N * 100 }
func (c Counter) add(k int) int { return c.N + k }

type Holder struct { Inner Counter }

func apply(f func() int) int { return f() }

func main() {
    var counter = Counter{N: 1}
    let read = counter.read
    println(read())
    let bump = counter.bump
    bump()
    bump()
    println(counter.read())
    let add = counter.add
    println(add(10))
    println(apply(counter.read))
    let holder = Holder{Inner: Counter{N: 5}}
    let inner = holder.Inner.read
    println(inner())
    let done = counter.finish
    println(done())
    let again = Counter{N: 9}
    let read9 = again.read
    let task = go read9()
    println(task.wait())
}
",
        "1\n3\n13\n3\n5\n300\n9\n",
    );
}

#[test]
fn method_values_destroy_an_owned_receiver_exactly_once() {
    prints(
        "package main

type Res struct { Name string }

func (r mut Res) drop() { println(\"drop \" + r.Name) }

func (r Res) name() string { return r.Name }

func (r own Res) finish() string {
    println(\"finish \" + r.Name)
    return r.Name
}

func made() func() string {
    let r = Res{Name: \"made\"}
    return r.name
}

func main() {
    let one = Res{Name: \"one\"}
    let done = one.finish
    println(done())

    let two = Res{Name: \"two\"}
    let unused = two.finish

    let f = made()
    println(f())
    println(f())

    let three = Res{Name: \"three\"}
    let n = three.name
    let t = go n()
    println(t.wait())
    println(\"end\")
}
",
        "finish one\ndrop one\none\nmade\nmade\ndrop three\nthree\nend\ndrop made\ndrop two\n",
    );
}

#[test]
fn async_function_values_are_awaited_passed_and_spawned() {
    prints(
        "package main

async func double(n int) int { return n * 2 }

async func slow(n int) int {
    let ch = channel<int>(1)
    ch.send(n)
    let v, ok = ch.receive()
    return v + 1
}

async func twice(op async func(int) int, x int) int {
    let a = await op(x)
    let b = await op(a)
    return b
}

async func run() int {
    let h = double
    let direct = await h(5)
    let viaHelper = await twice(double, 3)
    let viaSlow = await twice(slow, 10)
    let spawned = go h(21)
    let fromTask = await spawned
    return direct + viaHelper + viaSlow + fromTask
}

func main() {
    let t = go run()
    println(t.wait())
}
",
        "76\n",
    );
}

#[test]
fn async_function_values_pass_borrowed_and_owned_arguments_and_destroy_them_once() {
    prints(
        "package main

type Res struct { Name string }

func (r mut Res) drop() { println(\"drop \" + r.Name) }

async func take(r own Res) int {
    println(\"take \" + r.Name)
    return r.Name.len()
}

async func peek(r Res) int {
    return r.Name.len() * 10
}

async func run() int {
    let t = take
    let p = peek
    let first = Res{Name: \"a\"}
    let a = await t(first)
    let second = Res{Name: \"bb\"}
    let b = await p(second)
    println(\"after peek\")
    return a + b
}

func main() {
    let task = go run()
    println(task.wait())
}
",
        "take a\ndrop a\nafter peek\ndrop bb\n21\n",
    );
}

#[test]
fn a_panic_in_an_awaited_async_function_value_unwinds_its_callers_once() {
    let source = "package main

type Res struct { Name string }

func (r mut Res) drop() { println(\"drop \" + r.Name) }

async func fail(r own Res, zero int) int {
    println(\"failing\")
    return 1 / zero
}

async func run() int {
    let f = fail
    let r = Res{Name: \"held\"}
    return await f(r, 0)
}

func main() {
    let task = go run()
    println(task.wait())
}";
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "failing\ndrop held\n");
    assert!(
        stderr(&output).contains("panic in task 1: division by zero"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn async_function_values_live_in_struct_fields_and_arrays() {
    prints(
        "package main

async func add1(n int) int { return n + 1 }
async func add2(n int) int { return n + 2 }

type Route struct {
    Path string
    handler async func(int) int
}

async func dispatch(r mut Route, n int) int {
    return await (r.handler)(n)
}

async func run() int {
    var routes = Array<Route>{}
    routes.push(Route{Path: \"a\", handler: add1})
    routes.push(Route{Path: \"b\", handler: add2})
    var total = 0
    for var i = 0; i < routes.len(); i += 1 {
        total += await dispatch(routes[i], 10)
    }
    return total
}

func main() {
    let task = go run()
    println(task.wait())
}
",
        "23\n",
    );
}

#[test]
fn many_tasks_awaiting_through_an_async_function_value_do_not_hold_threads() {
    prints(
        "package main

async func waitFor(gate channel<int>) int {
    let v, ok = gate.receive()
    return v
}

async func through(op async func(channel<int>) int, gate channel<int>) int {
    return await op(gate)
}

async func worker(gate channel<int>, done channel<int>) {
    let op = waitFor
    let v = await through(op, gate)
    done.send(v)
}

func main() {
    let count = 20000
    let gate = channel<int>(count)
    let done = channel<int>(count)
    for var i = 0; i < count; i += 1 {
        go worker(gate, done)
    }
    for var i = 0; i < count; i += 1 {
        gate.send(1)
    }
    var total = 0
    for var i = 0; i < count; i += 1 {
        let v, ok = done.receive()
        total += v
    }
    println(total)
}
",
        "20000\n",
    );
}

#[test]
fn an_awaited_async_function_value_evaluates_its_callee_then_its_arguments_once() {
    prints(
        "package main

async func add(a int, b int) int { return a + b }

func pick() async func(int, int) int {
    println(\"pick\")
    return add
}

func left() int {
    println(\"left\")
    return 1
}

func right() int {
    println(\"right\")
    return 2
}

async func run() int {
    return await pick()(left(), right())
}

func main() {
    let t = go run()
    println(t.wait())
}
",
        "pick\nleft\nright\n3\n",
    );
}
