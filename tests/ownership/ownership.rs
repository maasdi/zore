//! Ownership-analysis tests: moves, borrows, partial moves, reinitialization,
//! and array-element restrictions, checked through the full frontend.

use zore::check::{Checked, check_file};
use zore::source::SourceMap;

struct Case {
    sources: SourceMap,
    checked: Checked,
}

impl Case {
    fn new(text: &str) -> Self {
        let mut sources = SourceMap::new();
        let id = sources.add("test.ore", text.into()).unwrap();
        let checked = check_file(sources.file(id).unwrap());
        Self { sources, checked }
    }

    fn errors(&self) -> Vec<(&str, &str)> {
        self.checked
            .diagnostics
            .iter()
            .map(|d| (d.message(), self.sources.slice(d.span()).unwrap()))
            .collect()
    }
}

/// Wrap declarations in a `main` package with a trivial entry point.
fn program(decls: &str) -> String {
    format!("package main\n\n{decls}\n\nfunc main() {{}}\n")
}

/// Wrap statements as the body of `main`.
fn body(stmts: &str) -> String {
    format!("package main\n\nfunc main() {{\n{stmts}\n}}\n")
}

fn accepts(text: &str) -> Case {
    let case = Case::new(text);
    assert!(
        case.checked.diagnostics.is_empty() && case.checked.package.is_some(),
        "{text}\n{:#?}",
        case.errors()
    );
    case
}

fn rejects(text: &str, message: &str) -> Case {
    let case = Case::new(text);
    assert!(case.checked.package.is_none(), "{text} produced HIR");
    let errors = case.errors();
    assert!(
        errors.iter().any(|(m, _)| m.contains(message)),
        "{text}\nexpected {message:?}, got {errors:#?}"
    );
    case
}

#[test]
fn move_values_transfer_and_ordinary_calls_borrow() {
    let declarations = "type Resource struct { id int }
        func (r mut Resource) drop() { println(r.id) }
        func inspect(r Resource) { println(r.id) }
        func consume(r own Resource) { println(r.id) }
        func inspect_then_consume(left Resource, right own Resource) {}";
    accepts(&program(&format!(
        "{declarations}\nfunc use() {{ let a = Resource{{id: 1}}\ninspect(a)\ninspect(a)\nconsume(a) }}"
    )));
    let moved = rejects(
        &program(&format!(
            "{declarations}\nfunc use() {{ let a = Resource{{id: 1}}\nconsume(a)\ninspect(a) }}"
        )),
        "use of moved value `a`",
    );
    let rendered = moved
        .checked
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message().contains("use of moved value `a`"))
        .unwrap()
        .render(&moved.sources)
        .unwrap();
    assert!(rendered.contains("value moved here"), "{rendered}");
    rejects(
        &program(&format!(
            "{declarations}\nfunc use() {{ let a = Resource{{id: 1}}\ndrop(a)\ndrop(a) }}"
        )),
        "use of moved value `a`",
    );
    rejects(
        &program(&format!(
            "{declarations}\nfunc invalid(r Resource) {{ consume(r) }}"
        )),
        "cannot move borrowed value `r`",
    );
    rejects(
        &program(&format!(
            "{declarations}\nfunc use() {{ let a = Resource{{id: 1}}\ninspect_then_consume(a, a) }}"
        )),
        "cannot move `a` while it is borrowed by this call",
    );
    accepts(&body("drop(5)"));
    accepts(&body("drop(5)"));
    rejects(&body("drop()"), "`drop` takes exactly 1 argument");
    accepts(&program(&format!(
        "{declarations}\nfunc use() {{ drop(Resource{{id: 1}}) }}"
    )));
}

#[test]
fn move_state_flows_through_branches_loops_and_reinitialization() {
    let resource = "type Resource struct { id int }
        func (r mut Resource) drop() {}";
    accepts(&program(&format!(
        "{resource}\nfunc use() {{ var a = Resource{{id: 1}}\nlet b = a\na = Resource{{id: 2}}\ndrop(a)\ndrop(b) }}"
    )));
    accepts(&program(&format!(
        "{resource}\nfunc use(flag bool) {{ let a = Resource{{id: 1}}\nif flag {{ drop(a) }} else {{ drop(a) }} }}"
    )));
    rejects(
        &program(&format!(
            "{resource}\nfunc use(flag bool) {{ let a = Resource{{id: 1}}\nif flag {{ drop(a) }}\ndrop(a) }}"
        )),
        "use of moved value `a`",
    );
    rejects(
        &program(&format!(
            "{resource}\nfunc use() {{ let a = Resource{{id: 1}}\nfor {{ drop(a) }} }}"
        )),
        "use of moved value `a`",
    );
    accepts(&program(&format!(
        "{resource}\ntype Wrapper struct {{ resource Resource }}\nfunc use(w own Wrapper) {{ let r = w.resource\ndrop(r) }}"
    )));
}

#[test]
fn partial_moves_track_fields_and_reinitialization() {
    let resource = "type Resource struct { id int }
        func (r mut Resource) drop() {}";
    let guard = format!(
        "{resource}\ntype Guard struct {{ resource Resource }}\nfunc (g mut Guard) drop() {{}}"
    );

    accepts(&program(&format!(
        "{resource}\ntype Wrapper struct {{ a Resource; b Resource }}\nfunc use(w own Wrapper) {{ let taken = w.a\ndrop(w.b)\ndrop(taken) }}"
    )));
    accepts(&program(&format!(
        "{resource}\ntype Wrapper struct {{ a Resource }}\nfunc consume(w own Wrapper) {{}}\nfunc use() {{ var w = Wrapper{{a: Resource{{id: 1}}}}\nlet taken = w.a\nw.a = Resource{{id: 2}}\ndrop(taken)\nconsume(w) }}"
    )));
    accepts(&program(&format!(
        "{resource}\ntype Wrapper struct {{ a Resource }}\nfunc take(r own Resource) {{}}\nfunc use() {{ var w = Wrapper{{a: Resource{{id: 1}}}}\nlet first = w.a\nw.a = Resource{{id: 2}}\ntake(first)\ntake(w.a) }}"
    )));
    accepts(&program(&format!(
        "{guard}\ntype Box struct {{ guard Guard }}\nfunc take_guard(g own Guard) {{}}\nfunc use(b own Box) {{ take_guard(b.guard) }}"
    )));

    rejects(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ a Resource; b Resource }}\nfunc inspect(w Wrapper) {{}}\nfunc use(w own Wrapper) {{ let taken = w.a\ninspect(w)\ndrop(taken)\ndrop(w.b) }}"
        )),
        "cannot use `w` as a whole value while a field is moved out",
    );
    rejects(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ a Resource; b Resource }}\nfunc consume(w own Wrapper) {{}}\nfunc use(w own Wrapper) {{ let taken = w.a\nconsume(w)\ndrop(taken) }}"
        )),
        "cannot use `w` as a whole value while a field is moved out",
    );
    rejects(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ a Resource }}\nfunc use(w own Wrapper) {{ let first = w.a\nlet second = w.a\ndrop(first)\ndrop(second) }}"
        )),
        "use of moved value `w.a`",
    );
    rejects(
        &program(&format!(
            "{guard}\nfunc use(g own Guard) {{ let r = g.resource\ndrop(r) }}"
        )),
        "cannot move `g.resource` out of a value with a custom `drop` method",
    );
    rejects(
        &program(&format!(
            "{guard}\ntype Box struct {{ guard Guard }}\nfunc use(b own Box) {{ let r = b.guard.resource\ndrop(r) }}"
        )),
        "cannot move `b.guard.resource` out of a value with a custom `drop` method",
    );
    rejects(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ a Resource }}\nfunc use(w Wrapper) {{ let taken = w.a\ndrop(taken) }}"
        )),
        "cannot move borrowed value `w.a`",
    );
}

#[test]
fn moving_an_array_element_out_through_an_index_is_rejected() {
    let resource = "type Resource struct { id int }
        func (r mut Resource) drop() {}
        func inspect(r Resource) {}
        func consume(r own Resource) {}";
    accepts(&program(&format!(
        "{resource}\nfunc use() {{ let xs = [Resource; 1]{{Resource{{id: 1}}}}\ninspect(xs[0]) }}"
    )));
    rejects(
        &program(&format!(
            "{resource}\nfunc use() {{ var xs = [Resource; 1]{{Resource{{id: 1}}}}\nconsume(xs[0]) }}"
        )),
        "moving `xs[_]` out through an array index is not supported yet",
    );
    rejects(
        &program(&format!(
            "{resource}\nfunc use() {{ var xs = [Resource; 1]{{Resource{{id: 1}}}}\nlet taken = xs[0] }}"
        )),
        "moving `xs[_]` out through an array index is not supported yet",
    );
}

#[test]
fn assigning_through_an_index_of_a_moved_array_is_rejected() {
    let resource = "type Resource struct { id int }
        func (r mut Resource) drop() {}";
    rejects(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ arr [Resource; 1] }}\nfunc use() {{ var w = Wrapper{{arr: [Resource; 1]{{Resource{{id: 1}}}}}}\nlet taken = w.arr\nw.arr[0] = Resource{{id: 2}} }}"
        )),
        "cannot assign through moved value `w.arr[_]`",
    );
}
