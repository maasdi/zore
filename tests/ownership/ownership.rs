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

fn program(decls: &str) -> String {
    format!("package main\n\n{decls}\n\nfunc main() {{}}\n")
}

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

const SLICE_FUNCS: &str = "func inspect(items []int) int { return items[0] }
func edit(items mut []int) { items[0] = 9 }
func firstHalf(s mut []int) mut []int { return s[:1] }";

fn slice_body(stmts: &str) -> String {
    format!("package main\n\n{SLICE_FUNCS}\n\nfunc main() {{\n{stmts}\n}}\n")
}

fn slice_program(decls: &str) -> String {
    program(&format!("{SLICE_FUNCS}\n{decls}"))
}

#[test]
fn mutable_views_of_one_owner_conflict_regardless_of_ranges() {
    let case = rejects(
        &slice_body(
            "var data = [int; 4]{1, 2, 3, 4}
            var low mut []int = data[0:2]
            var high mut []int = data[2:4]
            edit(low)
            edit(high)",
        ),
        "cannot borrow `data` as mutable because it is already borrowed",
    );
    let rendered = case.checked.diagnostics[0].render(&case.sources).unwrap();
    assert!(
        rendered.contains("mutable borrow of `data` created here"),
        "{rendered}"
    );
    assert!(
        rendered.contains("the view `low` is used later"),
        "{rendered}"
    );
    assert!(rendered.contains("whole originating place"), "{rendered}");
    accepts(&slice_body(
        "var data = [int; 4]{1, 2, 3, 4}
        var low mut []int = data[0:2]
        edit(low)
        var high mut []int = data[2:4]
        edit(high)",
    ));
}

#[test]
fn shared_and_mutable_views_cannot_overlap_while_live() {
    rejects(
        &slice_body(
            "var data = [int; 3]{1, 2, 3}
            let view = data[:]
            var part mut []int = data[1:]
            edit(part)
            _ = inspect(view)",
        ),
        "cannot borrow `data` as mutable because it is already borrowed",
    );
    accepts(&slice_body(
        "var data = [int; 3]{1, 2, 3}
        let view = data[:]
        _ = inspect(view)
        var part mut []int = data[1:]
        edit(part)",
    ));
    accepts(&slice_body(
        "var data = [int; 3]{1, 2, 3}
        let a = data[:]
        let b = data[1:]
        _ = inspect(a)
        _ = inspect(b)
        _ = data[0]",
    ));
}

#[test]
fn the_owner_is_usable_again_after_the_view_last_use() {
    rejects(
        &slice_body(
            "var data = [int; 3]{1, 2, 3}
            let view = data[:]
            data[0] = 5
            _ = inspect(view)",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    accepts(&slice_body(
        "var data = [int; 3]{1, 2, 3}
        let view = data[:]
        _ = inspect(view)
        data[0] = 5",
    ));
    rejects(
        &slice_body(
            "var data = [int; 3]{1, 2, 3}
            var part mut []int = data[:]
            _ = data[0]
            edit(part)",
        ),
        "cannot use `data[_]` while it is mutably borrowed",
    );
}

#[test]
fn returned_views_keep_the_argument_borrowed() {
    rejects(
        &slice_body(
            "var data = [int; 3]{1, 2, 3}
            let half = firstHalf(data[:])
            _ = data[0]
            _ = half[0]",
        ),
        "cannot use `data[_]` while it is mutably borrowed",
    );
    accepts(&slice_body(
        "var data = [int; 3]{1, 2, 3}
        let half = firstHalf(data[:])
        _ = half[0]
        _ = data[0]",
    ));
}

#[test]
fn copying_a_mutable_view_suspends_the_source() {
    rejects(
        &slice_program("func use(s mut []int) { var next = s\ns[0] = 1\nnext[0] = 2 }"),
        "cannot assign to `s[_]` while it is borrowed",
    );
    accepts(&slice_program(
        "func use(s mut []int) { var next = s\nnext[0] = 2\ns[0] = 1 }",
    ));
    rejects(
        &slice_program(
            "func use(s mut []int) { var tail mut []int = s[1:]\n_ = s[0]\ntail[0] = 2 }",
        ),
        "cannot use `s[_]` while it is mutably borrowed",
    );
    accepts(&slice_program(
        "func use(s mut []int) { var tail mut []int = s[1:]\ntail[0] = 2\n_ = s[0] }",
    ));
    accepts(&slice_program(
        "func use(s []int) int { let copy = s\nreturn s[0] + copy[0] }",
    ));
    accepts(&slice_program(
        "func use(s []int) int { var rest = s\nrest = rest[1:]\nreturn rest[0] }",
    ));
}

#[test]
fn return_contracts_require_parameter_backed_views() {
    accepts(&slice_program(
        "type View struct { Items []int }
        func same(s mut []int) mut []int { return s }
        func whole(a [int; 2]) []int { return a[:] }
        func exclusive(a mut [int; 2]) mut []int { return a[1:] }
        func wrap(items []int) View { return View{Items: items} }
        func forward(view View) View { return view }",
    ));
    for (decl, message) in [
        (
            "func f() []int { let a = [int; 2]{1, 2}\nreturn a[:] }",
            "cannot return a view of local `a`",
        ),
        (
            "func f() []int { let a = [int; 2]{1, 2}\nreturn a[0:0] }",
            "cannot return a view of local `a`",
        ),
        (
            "func mk() [int; 2] { return [int; 2]{1, 2} }\nfunc f() []int { return mk()[:] }",
            "cannot return a view of a temporary value",
        ),
        (
            "func f(a own [int; 2]) []int { return a[:] }",
            "cannot return a view of `own` parameter `a`",
        ),
        (
            "type View struct { Items []int }\nfunc f() View { let a = [int; 2]{1, 2}\nreturn View{Items: a[:]} }",
            "cannot return a view of local `a`",
        ),
    ] {
        rejects(&slice_program(decl), message);
    }
    accepts(&slice_program(
        "type Holder struct { Items []int }
        func f(h own Holder) []int { return h.Items }",
    ));
}

#[test]
fn contracts_bind_results_only_to_the_inputs_they_derive_from() {
    let pick_first = "func pick(a []int, b []int) []int { return a }";
    accepts(&slice_program(&format!(
        "{pick_first}
        func use() {{ var x = [int; 2]{{1, 2}}\nvar y = [int; 2]{{3, 4}}\nlet r = pick(x[:], y[:])\ny[0] = 7\n_ = r[0] }}"
    )));
    let either = "func either(c bool, a []int, b []int) []int { if c { return a }\nreturn b }";
    rejects(
        &slice_program(&format!(
            "{either}
            func use() {{ var x = [int; 2]{{1, 2}}\nvar y = [int; 2]{{3, 4}}\nlet r = either(true, x[:], y[:])\ny[0] = 7\n_ = r[0] }}"
        )),
        "cannot assign to `y[_]` while it is borrowed",
    );
    let wrap = "type View struct { Items []int }\nfunc wrap(items []int) View { return View{Items: items} }";
    rejects(
        &slice_program(&format!(
            "{wrap}
            func use() {{ var x = [int; 2]{{1, 2}}\nlet v = wrap(x[:])\nx[0] = 3\n_ = v.Items[0] }}"
        )),
        "cannot assign to `x[_]` while it is borrowed",
    );
}

#[test]
fn recursive_functions_get_consistent_contracts() {
    let rec =
        "func rec(n int, s mut []int) mut []int { if n == 0 { return s }\nreturn rec(n - 1, s) }";
    rejects(
        &slice_program(&format!(
            "{rec}
            func use() {{ var x = [int; 2]{{1, 2}}\nlet r = rec(3, x[:])\n_ = x[0]\n_ = r[0] }}"
        )),
        "cannot use `x[_]` while it is mutably borrowed",
    );
    accepts(&slice_program(&format!(
        "{rec}
        func use() {{ var x = [int; 2]{{1, 2}}\nlet r = rec(3, x[:])\n_ = r[0]\n_ = x[0] }}"
    )));
}

#[test]
fn zero_slices_returned_by_propagation_have_no_loan() {
    accepts(&program(
        "func g() error { return nil }
        func f() ([]int, error) {\ng()?\nreturn f() }",
    ));
}

#[test]
fn views_cannot_outlive_their_owner() {
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var s []int = data[:]
            if true {
                let inner = [int; 2]{3, 4}
                s = inner[:]
            }
            _ = inspect(s)",
        ),
        "`inner` does not live long enough",
    );
    accepts(&slice_body(
        "var data = [int; 2]{1, 2}
        var s []int = data[:]
        if true {
            let inner = [int; 2]{3, 4}
            s = inner[:]
            _ = inspect(s)
            s = data[:]
        }
        _ = inspect(s)",
    ));
    rejects(
        &slice_program(
            "func mk() [int; 2] { return [int; 2]{1, 2} }
            func use() { let s = mk()[:]\n_ = inspect(s) }",
        ),
        "temporary value does not live long enough",
    );
    accepts(&slice_program(
        "func mk() [int; 2] { return [int; 2]{1, 2} }
        func use() { _ = inspect(mk()[:]) }",
    ));
}

#[test]
fn loops_carry_live_views_across_iterations() {
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var s []int = data[:]
            for var i = 0; i < 2; i += 1 {
                let inner = [int; 2]{3, 4}
                _ = inspect(s)
                s = inner[:]
            }",
        ),
        "does not live long enough",
    );
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var other = [int; 2]{3, 4}
            var previous mut []int = other[:]
            for var i = 0; i < 2; i += 1 {
                var current mut []int = data[:]
                edit(previous)
                previous = current
            }",
        ),
        "cannot borrow `data` as mutable because it is already borrowed",
    );
    accepts(&slice_body(
        "var data = [int; 2]{1, 2}
        var keep mut []int = data[:]
        for var i = 0; i < 2; i += 1 {
            edit(keep)
            keep = data[:]
        }",
    ));
    accepts(&slice_body(
        "var data = [int; 2]{1, 2}
        for var i = 0; i < 2; i += 1 {
            var part mut []int = data[:]
            edit(part)
        }",
    ));
}

#[test]
fn backing_owners_cannot_be_moved_replaced_or_dropped_while_viewed() {
    let resource = "type Res struct { id int }\nfunc (r mut Res) drop() {}\nfunc first(rs []Res) int { return rs[0].id }";
    for (stmts, message) in [
        ("let taken = rs", "cannot move `rs` while it is borrowed"),
        (
            "rs = [Res; 2]{Res{id: 3}, Res{id: 4}}",
            "cannot assign to `rs` while it is borrowed",
        ),
        ("drop(rs)", "cannot move `rs` while it is borrowed"),
    ] {
        rejects(
            &program(&format!(
                "{resource}
                func use() {{ var rs = [Res; 2]{{Res{{id: 1}}, Res{{id: 2}}}}\nlet view = rs[:]\n{stmts}\n_ = first(view) }}"
            )),
            message,
        );
    }
    accepts(&program(&format!(
        "{resource}
        func use() {{ var rs = [Res; 2]{{Res{{id: 1}}, Res{{id: 2}}}}\nlet view = rs[:]\n_ = first(view)\nlet taken = rs }}"
    )));
}

#[test]
fn views_stored_through_mut_parameters_borrow_for_the_caller() {
    let fill = "type View struct { Items []int }
        func fill(out mut View, items []int) { out.Items = items }";
    rejects(
        &program(&format!(
            "{fill}\nfunc g() {{ var data = [int; 2]{{1, 2}}\nvar other = [int; 1]{{0}}\nvar v = View{{Items: other[:]}}\nfill(v, data[:])\ndata[0] = 5\nprintln(v.Items[0]) }}"
        )),
        "cannot assign to `data[_]` while it is borrowed",
    );
    accepts(&program(&format!(
        "{fill}\nfunc g() {{ var data = [int; 2]{{1, 2}}\nvar other = [int; 1]{{0}}\nvar v = View{{Items: other[:]}}\nfill(v, data[:])\nprintln(v.Items[0])\ndata[0] = 5 }}"
    )));
    rejects(
        &program(&format!(
            "{fill}\nfunc g() {{ var other = [int; 1]{{0}}\nvar v = View{{Items: other[:]}}\n{{ var data = [int; 1]{{1}}\nfill(v, data[:]) }}\nprintln(v.Items[0]) }}"
        )),
        "`data` does not live long enough",
    );
    rejects(
        &program(
            "type View struct { Items []int }
            func fill(out mut View) { var local = [int; 1]{1}\nout.Items = local[:] }",
        ),
        "cannot store a view of local `local` into `out`",
    );
    accepts(&program(
        "type View struct { Items []int }
        func fill(out mut View, items []int) { out.Items = items }
        func forward(out mut View, items []int) { fill(out, items) }
        func borrowArray(out mut View, data [int; 2]) { out.Items = data[:] }",
    ));
}

#[test]
fn views_stored_through_slice_elements_borrow_for_the_slice_owner() {
    let fill = "type View struct { Items []int }
        func fill(out mut View, items []int) { out.Items = items }
        func set(rows mut [][]int, items []int) { rows[0] = items }
        func setVia(rows mut [][]int, items []int) { var alias mut [][]int = rows[:]\nalias[0] = items }";
    accepts(&program(&format!(
        "{fill}
        func g() {{ var other = [int; 1]{{0}}\nvar rows = [View; 1]{{View{{Items: other[:]}}}}\nvar slots mut []View = rows[:]\nvar data = [int; 1]{{1}}\nfill(slots[0], data[:])\nprintln(rows[0].Items[0]) }}
        func h() {{ var other = [int; 1]{{0}}\nvar rows = [[]int; 1]{{other[:]}}\nvar data = [int; 1]{{1}}\nsetVia(rows[:], data[:])\nprintln(rows[0][0]) }}"
    )));
    rejects(
        &program(&format!(
            "{fill}
            func g() {{ var other = [int; 1]{{0}}\nvar rows = [[]int; 1]{{other[:]}}\n{{ var data = [int; 1]{{1}}\nset(rows[:], data[:]) }}\nprintln(rows[0][0]) }}"
        )),
        "`data` does not live long enough",
    );
    rejects(
        &program(&format!(
            "{fill}
            func g() {{ var other = [int; 1]{{0}}\nvar rows = [[]int; 1]{{other[:]}}\nvar slots mut [][]int = rows[:]\nvar data = [int; 1]{{1}}\nslots[0] = data[:]\ndata[0] = 2\nprintln(rows[0][0]) }}"
        )),
        "cannot assign to `data[_]` while it is borrowed",
    );
    rejects(
        &program("func set(rows mut [][]int) { var local = [int; 1]{1}\nrows[0] = local[:] }"),
        "cannot store a view of local `local` into `rows`",
    );
    rejects(
        &program(
            "type Holder struct { rows mut [][]int }
            func keep(h own Holder, items []int) { var moved = h\nmoved.rows[0] = items }
            func g() { var other = [int; 1]{0}\nvar rows = [[]int; 1]{other[:]}\n{ var data = [int; 1]{1}\nkeep(Holder{rows: rows[:]}, data[:]) }\nprintln(rows[0][0]) }",
        ),
        "`data` does not live long enough",
    );
    rejects(
        &body(
            "var other = [int; 1]{0}\nvar rows = [[]int; 1]{other[:]}\nvar slots mut [][]int = rows[:]
            let put = func(items mut [][]int, view []int) { items[0] = view }
            { var data = [int; 1]{1}\nput(slots, data[:]) }\nprintln(rows[0][0])",
        ),
        "`data` does not live long enough",
    );
}

#[test]
fn mutable_views_inside_collections_are_exclusive_reborrows() {
    accepts(&body(
        "var x = [int; 2]{1, 2}\nvar y = [int; 2]{3, 4}
        var views = Array<mut []int>{x[:], y[:]}
        var first = views[0]\nfirst[0] = 5
        var second = views[1]\nsecond[0] = 6
        var all mut []mut []int = views[:]\nvar again = all[0]\nagain[1] = 7
        var m = map[int]mut []int{1: x[:]}\nvar found, taken = m.remove(1)\nif found { taken[0] = 8 }",
    ));
    rejects(
        &body(
            "var x = [int; 2]{1, 2}\nvar views = Array<mut []int>{x[:]}
            var first = views[0]\nvar second = views[0]\nfirst[0] = 5\nsecond[0] = 6",
        ),
        "cannot borrow `views[_]` as mutable because it is already borrowed",
    );
    rejects(
        &body(
            "var x = [int; 2]{1, 2}\nvar views = Array<mut []int>{x[:]}\nx[0] = 5\nvar first = views[0]\nfirst[0] = 6",
        ),
        "cannot assign to `x[_]` while it is borrowed",
    );
}

#[test]
fn a_call_cannot_take_a_mutable_view_and_read_its_source() {
    rejects(
        &slice_program(
            "func both(a mut []int, b [int; 3]) {}
            func use() { var data = [int; 3]{1, 2, 3}\nboth(data[:], data) }",
        ),
        "cannot borrow `data` because it is mutably borrowed",
    );
}

#[test]
fn moving_an_element_out_of_a_slice_is_rejected() {
    rejects(
        &program(
            "type Res struct { id int }
            func (r mut Res) drop() {}
            func take(r own Res) {}
            func use(rs []Res) { take(rs[0]) }",
        ),
        "cannot move `rs[_]` out of a slice",
    );
}

#[test]
fn borrows_through_a_view_keep_the_view_backing_borrowed() {
    let grid = "var grid = [[int; 2]; 2]{[int; 2]{1, 2}, [int; 2]{3, 4}}";
    rejects(
        &slice_body(&format!(
            "{grid}\nlet rows = grid[:]\nlet row = rows[0][:]\ngrid[0][0] = 5\n_ = inspect(row)"
        )),
        "cannot assign to `grid[_][_]` while it is borrowed",
    );
    rejects(
        &slice_program(
            &format!(
                "func first(rows [][2]int) []int {{ return rows[0][:] }}
            func use() {{ {grid}\nlet row = first(grid[:])\ngrid[0][0] = 5\n_ = inspect(row) }}"
            )
            .replace("[][2]int", "[][int; 2]"),
        ),
        "cannot assign to `grid[_][_]` while it is borrowed",
    );
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var views = [[]int; 1]{data[:]}
            let all = views[:]
            data[0] = 5
            _ = inspect(all[0])",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    accepts(&slice_body(&format!(
        "{grid}\nlet rows = grid[:]\nlet row = rows[0][:]\n_ = inspect(row)\ngrid[0][0] = 5"
    )));
}

const GUARD: &str = "type Guard struct { id int }\nfunc (g mut Guard) drop() {}";

#[test]
fn dynamic_arrays_move_whole() {
    rejects(
        &body("let xs = Array<int>{1}\nlet ys = xs\nlet zs = xs"),
        "use of moved value `xs`",
    );
    rejects(
        &program(
            "func take(xs own Array<int>) {}\nfunc use() { let xs = Array<int>{1}\ntake(xs)\n_ = xs[0] }",
        ),
        "use of moved value `xs`",
    );
    accepts(&program(
        "func read(xs Array<int>) int { return xs[0] }\nfunc use() { let xs = Array<int>{1}\n_ = read(xs)\n_ = read(xs) }",
    ));
}

#[test]
fn dynamic_array_elements_cannot_be_moved_out() {
    for stmts in [
        "let first = gs[0]",
        "consume(gs[0])",
        "let bag = Bag{Items: Array<Guard>{Guard{id: 1}}}\nlet first = bag.Items[0]",
    ] {
        rejects(
            &program(&format!(
                "{GUARD}\ntype Bag struct {{ Items Array<Guard> }}\nfunc consume(g own Guard) {{}}\nfunc use() {{ let gs = Array<Guard>{{Guard{{id: 1}}}}\n{stmts} }}"
            )),
            "out of a dynamic array",
        );
    }
    accepts(&program(&format!(
        "{GUARD}\nfunc read(g Guard) int {{ return g.id }}\nfunc use() {{ let gs = Array<Guard>{{Guard{{id: 1}}}}\n_ = read(gs[0])\n_ = read(Array<Guard>{{Guard{{id: 2}}}}[0]) }}"
    )));
}

#[test]
fn views_of_a_dynamic_array_keep_its_owner_in_place() {
    for (stmts, message) in [
        (
            "xs = Array<int>{4}",
            "cannot assign to `xs` while it is borrowed",
        ),
        ("let moved = xs", "cannot move `xs` while it is borrowed"),
        ("drop(xs)", "cannot move `xs` while it is borrowed"),
        ("xs[0] = 9", "cannot assign to `xs[_]` while it is borrowed"),
        (
            "bump(xs)",
            "cannot borrow `xs` as mutable because it is already borrowed",
        ),
    ] {
        rejects(
            &slice_program(&format!(
                "func bump(xs mut Array<int>) {{}}\nfunc use() {{ var xs = Array<int>{{1, 2}}\nlet view = xs[:]\n{stmts}\n_ = inspect(view) }}"
            )),
            message,
        );
    }
    accepts(&slice_program(
        "func use() { var xs = Array<int>{1, 2}\nlet view = xs[:]\n_ = inspect(view)\nxs = Array<int>{4} }",
    ));
    rejects(
        &slice_program("func use() { let view = Array<int>{1}[:]\n_ = inspect(view) }"),
        "temporary value does not live long enough",
    );
    accepts(&slice_program(
        "func use() { _ = inspect(Array<int>{1}[:]) }",
    ));
}

#[test]
fn views_of_dynamic_arrays_return_only_from_borrowed_parameters() {
    accepts(&slice_program(
        "func all(xs Array<int>) []int { return xs[:] }\nfunc edit_all(xs mut Array<int>) mut []int { return xs[:] }",
    ));
    rejects(
        &slice_program("func all(xs own Array<int>) []int { return xs[:] }"),
        "cannot return a view of `own` parameter `xs`",
    );
    rejects(
        &slice_program("func all() []int { let xs = Array<int>{1}\nreturn xs[:] }"),
        "cannot return a view of local `xs`",
    );
}

#[test]
fn views_stored_in_dynamic_arrays_keep_their_backing_borrowed() {
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var views = Array<[]int>{data[:]}
            data[0] = 5
            _ = inspect(views[0])",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var other = [int; 2]{3, 4}
            var views = Array<[]int>{other[:]}
            views[0] = data[:]
            data[0] = 5
            _ = inspect(views[0])",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var views = Array<[]int>{data[:]}
            let all = views[:]
            data[0] = 5
            _ = inspect(all[0])",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    accepts(&program(
        "func fill(views mut Array<[]int>, items []int) { views[0] = items }",
    ));
    rejects(
        &program(
            "func fill(views mut Array<[]int>, items []int) { views[0] = items }
            func g() { var data = [int; 1]{1}\nvar other = [int; 1]{0}\nvar views = Array<[]int>{other[:]}\nfill(views, data[:])\ndata[0] = 5\nprintln(views[0][0]) }",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
}

#[test]
fn two_mutable_element_borrows_of_a_dynamic_array_conflict() {
    rejects(
        &program(
            "func both(a mut int, b mut int) {}\nfunc use() { var xs = Array<int>{1, 2}\nboth(xs[0], xs[1]) }",
        ),
        "is also borrowed by another argument of this call",
    );
}

#[test]
fn maps_move_whole_and_removed_values_belong_to_the_caller() {
    rejects(
        &body("let m = map[string]int{}\nlet n = m\nlet f, v = m[\"a\"]"),
        "use of moved value `m`",
    );
    rejects(
        &program(&format!(
            "{GUARD}\nfunc consume(g own Guard) {{}}\nfunc use() {{ var m = map[string]Guard{{\"a\": Guard{{id: 1}}}}\nlet found, g = m.remove(\"a\")\nconsume(g)\nlet h = g }}"
        )),
        "use of moved value `g`",
    );
    accepts(&program(&format!(
        "{GUARD}\nfunc consume(g own Guard) {{}}\nfunc use() {{ var m = map[string]Guard{{\"a\": Guard{{id: 1}}}}\nlet found, g = m.remove(\"a\")\nconsume(g) }}"
    )));
    rejects(
        &program(&format!(
            "{GUARD}\nfunc use() {{ let g = Guard{{id: 1}}\nvar m = map[string]Guard{{}}\nm[\"a\"] = g\nlet h = g }}"
        )),
        "use of moved value `g`",
    );
}

#[test]
fn views_stored_in_maps_keep_their_backing_borrowed() {
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var views = map[string][]int{}
            views[\"a\"] = data[:]
            data[0] = 5
            let found, view = views[\"a\"]
            _ = inspect(view)",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    rejects(
        &slice_body(
            "var data = [int; 2]{1, 2}
            var views = map[string][]int{\"a\": data[:]}
            let found, view = views.remove(\"a\")
            data[0] = 5
            _ = inspect(view)",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    accepts(&slice_body(
        "var data = [int; 2]{1, 2}
        var views = map[string][]int{\"a\": data[:]}
        let found, view = views[\"a\"]
        _ = inspect(view)
        drop(views)
        data[0] = 5",
    ));
    rejects(
        &program(
            "func fill(m mut map[string][]int, items []int) { m[\"a\"] = items }
            func g() { var data = [int; 1]{1}\nvar m = map[string][]int{}\nfill(m, data[:])\ndata[0] = 5\nlet found, items = m[\"a\"] }",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
}

#[test]
fn a_later_argument_cannot_remove_from_an_earlier_borrowed_map() {
    rejects(
        &program(
            "func f(m map[string]int, found bool) {}
            func pick(m mut map[string]int) bool { let found, _ = m.remove(\"a\")\nreturn found }
            func use() { var m = map[string]int{}\nf(m, pick(m)) }",
        ),
        "`m` is borrowed by an earlier argument and mutated by a later one",
    );
}

const RESOURCE: &str = "type Res struct { id int }
func (r mut Res) drop() {}
func take(r own Res) {}
func show(r Res) {}";

fn resource_body(stmts: &str) -> String {
    format!("package main\n\n{RESOURCE}\n\nfunc main() {{\n{stmts}\n}}\n")
}

#[test]
fn a_shared_capture_blocks_writes_until_the_closure_last_use() {
    let case = rejects(
        &body(
            "var n = 1
            let f = func() { println(n) }
            n = 2
            f()",
        ),
        "cannot assign to `n` while it is borrowed",
    );
    let rendered = case.checked.diagnostics[0].render(&case.sources).unwrap();
    assert!(
        rendered.contains("`n` captured by this function literal"),
        "{rendered}"
    );
    assert!(
        rendered.contains("the closure `f` is used later"),
        "{rendered}"
    );
    accepts(&body(
        "var n = 1
        let f = func() { println(n) }
        f()
        n = 2
        println(n)",
    ));
    accepts(&body(
        "let n = 1
        let a = func() { println(n) }
        let b = func() { println(n) }
        a()
        b()
        println(n)",
    ));
}

#[test]
fn an_exclusive_capture_blocks_every_other_use() {
    rejects(
        &body(
            "var count = 0
            let f = func() { count += 1 }
            println(count)
            f()",
        ),
        "cannot use `count` while it is mutably borrowed",
    );
    rejects(
        &body(
            "var count = 0
            let a = func() { count += 1 }
            let b = func() { count += 2 }
            a()
            b()",
        ),
        "cannot borrow `count` as mutable because it is already borrowed",
    );
    rejects(
        &program(
            "func run(f func(), n mut int) { f() }
            func g() { var count = 0
            let f = func() { count += 1 }
            run(f, count) }",
        ),
        "cannot borrow `count` as mutable because it is already borrowed",
    );
    accepts(&body(
        "var count = 0
        let a = func() { count += 1 }
        a()
        let b = func() { count += 2 }
        b()
        println(count)",
    ));
}

#[test]
fn captured_slices_keep_their_backing_borrowed() {
    rejects(
        &body(
            "var data = [int; 2]{1, 2}
            let view = data[:]
            let f = func() { println(view[0]) }
            data[0] = 5
            f()",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    rejects(
        &body(
            "var data = [int; 2]{1, 2}
            let f = func() { data[0] = 9 }
            let view = data[:]
            f()
            println(view[0])",
        ),
        "cannot borrow `data` because it is mutably borrowed",
    );
}

#[test]
fn a_closure_cannot_outlive_what_it_captures() {
    rejects(
        &body(
            "var f = func() {}
            {
                let x = 1
                f = func() { println(x) }
            }
            f()",
        ),
        "`x` does not live long enough",
    );
    accepts(&body(
        "var f = func() {}
        {
            let x = 1
            f = func() { println(x) }
            f()
        }",
    ));
}

#[test]
fn closures_are_move_values_and_calls_use_them_exclusively() {
    rejects(
        &body("let f = func() {}\nlet g = f\nf()\ng()"),
        "use of moved value `f`",
    );
    rejects(
        &body(
            "var f = func() {}
            let g = func() { f() }
            f()
            g()",
        ),
        "cannot call `f` while it is borrowed",
    );
    accepts(&body(
        "let f = func() {}
        let g = func() { f() }
        g()
        g()",
    ));
    rejects(
        &program("func keep(f func()) { let g = f }"),
        "cannot move borrowed value `f`",
    );
}

#[test]
fn borrowing_closures_never_consume_captured_values() {
    accepts(&resource_body(
        "let r = Res{id: 1}
        let f = func() { show(r) }
        f()
        f()
        take(r)",
    ));
    accepts(&resource_body(
        "let r = Res{id: 1}
        let f = func() { take(r) }
        f()",
    ));
    rejects(
        &resource_body(
            "let r = Res{id: 1}
            let f = func() { take(r) }
            f()
            show(r)",
        ),
        "use of moved value `r`",
    );
    rejects(
        &resource_body(
            "let r = Res{id: 1}
            let f = func() { take(r) }
            f()
            f()",
        ),
        "use of moved value `f`",
    );
    rejects(
        &resource_body(
            "let r = Res{id: 1}
            let f = func() { show(r) }
            take(r)
            f()",
        ),
        "cannot move `r` while it is borrowed",
    );
    rejects(
        &resource_body(
            "let r = Res{id: 1}
            take(r)
            let f = func() { show(r) }
            f()",
        ),
        "use of moved value `r`",
    );
}

#[test]
fn storing_through_a_capture_is_limited_to_values_without_borrows() {
    accepts(&body(
        "var n = 0
        let f = func() { n = 5 }
        f()
        println(n)",
    ));
    accepts(&body(
        "var f = func() {}
        let g = func() { f = func() {} }
        g()
        f()",
    ));
    rejects(
        &body(
            "var a = [int; 1]{1}
            var b = [int; 1]{2}
            var s = a[:]
            let point = func() { s = b[:] }
            point()
            b[0] = 5
            println(s[0])",
        ),
        "cannot assign to `b[_]` while it is borrowed",
    );
    accepts(&body(
        "var a = [int; 1]{1}
        var b = [int; 1]{2}
        var s = a[:]
        let point = func() { s = b[:] }
        point()
        println(s[0])
        b[0] = 5",
    ));
    rejects(
        &body(
            "var a = [int; 1]{1}
            var s = a[:]
            let point = func(items []int) { s = items }",
        ),
        "storing a view from a function literal's parameter into captured `s` is not supported yet",
    );
    rejects(
        &body(
            "var a = [int; 1]{1}
            var s = a[:]
            let point = func() { var local = [int; 1]{2}\ns = local[:] }",
        ),
        "cannot store a view of local `local` into `s`",
    );
}

#[test]
fn clone_borrows_its_argument_and_never_consumes_it() {
    accepts(&body(
        "let a = Array<int>{1, 2}
        let b = clone(a)
        let c = clone(a)
        println(a[0] + b[0] + c[1])",
    ));
    rejects(
        &body("let a = Array<int>{1}\nlet b = a\nlet c = clone(a)"),
        "use of moved value `a`",
    );
    rejects(
        &body("let a = Array<int>{1}\nlet c = clone(a)\nlet b = a\nlet d = clone(a)"),
        "use of moved value `a`",
    );
    accepts(&body(
        "var a = Array<int>{1}\nlet c = clone(a)\na[0] = 5\nprintln(c[0])",
    ));
}

#[test]
fn clone_of_a_value_holding_views_keeps_the_backing_borrowed() {
    let views = "type View struct { items []int }";
    rejects(
        &program(&format!(
            "{views}\nfunc g() {{ var d = [int; 2]{{1, 2}}\nlet v = View{{items: d[:]}}\nlet c = clone(v)\nd[0] = 5\nprintln(c.items[0]) }}"
        )),
        "cannot assign to `d[_]` while it is borrowed",
    );
    accepts(&program(&format!(
        "{views}\nfunc g() {{ var d = [int; 2]{{1, 2}}\nlet v = View{{items: d[:]}}\nlet c = clone(v)\nprintln(c.items[0])\nd[0] = 5 }}"
    )));
}

#[test]
fn clone_can_be_discarded_and_borrowed_through_places() {
    accepts(&resource_body(
        "let list = Array<int>{1}
        clone(list)
        let nested = Array<Array<int>>{Array<int>{1}}
        let copy = clone(nested[0])
        println(copy[0])",
    ));
    rejects(
        &body("var a = Array<int>{1}\nlet s = a[:]\nlet c = clone(a)\na[0] = 2\nprintln(s[0])"),
        "cannot assign to `a[_]` while it is borrowed",
    );
}

const WINDOW: &str = "type Window struct { items mut []int\nlabel int }";

fn window_body(stmts: &str) -> String {
    format!("package main\n\n{WINDOW}\n\nfunc main() {{\n{stmts}\n}}\n")
}

#[test]
fn a_struct_holding_a_mutable_view_borrows_its_backing_exclusively() {
    rejects(
        &window_body(
            "var data = [int; 2]{1, 2}
            let w = Window{items: data[:], label: 1}
            println(data[0])
            w.items[0] = 5",
        ),
        "cannot use `data[_]` while it is mutably borrowed",
    );
    accepts(&window_body(
        "var data = [int; 2]{1, 2}
        let w = Window{items: data[:], label: 1}
        w.items[0] = 5
        println(data[0])",
    ));
}

#[test]
fn copying_a_struct_reborrows_the_mutable_views_inside_it() {
    rejects(
        &window_body(
            "var data = [int; 2]{1, 2}
            let w = Window{items: data[:], label: 1}
            let copy = w
            w.items[0] = 5
            copy.items[0] = 6",
        ),
        "cannot assign to `w.items[_]` while it is borrowed",
    );
    accepts(&window_body(
        "var data = [int; 2]{1, 2}
        let w = Window{items: data[:], label: 1}
        let copy = w
        println(w.label)
        copy.items[0] = 6
        w.items[1] = 7",
    ));
    rejects(
        &window_body(
            "var a = [int; 1]{1}
            var b = [int; 1]{2}
            var rows = [mut []int; 2]{a[:], b[:]}
            let other = rows
            rows[0][0] = 3
            other[1][0] = 4",
        ),
        "cannot assign to `rows[_][_]` while it is borrowed",
    );
}

#[test]
fn replacing_a_struct_ends_reborrows_through_its_old_views() {
    accepts(&window_body(
        "var a = [int; 1]{1}
        var b = [int; 1]{2}
        var w = Window{items: a[:], label: 1}
        w = Window{items: b[:], label: 2}
        a[0] = 10
        w.items[0] = 30",
    ));
}

#[test]
fn a_struct_holding_a_mutable_view_cannot_outlive_its_backing() {
    rejects(
        &program(&format!(
            "{WINDOW}\nfunc escape() Window {{ var data = [int; 1]{{1}}\nreturn Window{{items: data[:], label: 0}} }}"
        )),
        "cannot return a view of local `data`",
    );
    rejects(
        &window_body(
            "var outer = [int; 1]{0}
            var w = Window{items: outer[:], label: 0}
            {
                var data = [int; 1]{1}
                w = Window{items: data[:], label: 1}
            }
            w.items[0] = 2",
        ),
        "does not live long enough",
    );
}

const WATCH: &str = "type Watch struct { items []int\nid int }
func (w mut Watch) drop() { println(w.items[0]) }
func keep(w own Watch) {}";

fn watch_body(stmts: &str) -> String {
    format!("package main\n\n{WATCH}\n\nfunc main() {{\n{stmts}\n}}\n")
}

#[test]
fn a_drop_that_reads_a_view_keeps_it_borrowed_until_the_drop() {
    accepts(&watch_body(
        "var data = [int; 2]{1, 2}
        let w = Watch{items: data[:], id: 7}
        println(w.id)",
    ));
    let case = rejects(
        &watch_body(
            "var data = [int; 2]{1, 2}
            let w = Watch{items: data[:], id: 7}
            println(w.id)
            data[0] = 5",
        ),
        "cannot assign to `data[_]` while it is borrowed",
    );
    let rendered = case.checked.diagnostics[0].render(&case.sources).unwrap();
    assert!(
        rendered.contains("`w`'s custom `drop` can read this borrow until `w` is dropped"),
        "{rendered}"
    );
    rejects(
        &watch_body(
            "var list = Array<int>{1}
            let w = Watch{items: list[:], id: 1}
            let other = list",
        ),
        "cannot move `list` while it is borrowed",
    );
    rejects(
        &program(&format!(
            "{WATCH}\nfunc g(flag bool) {{ var data = [int; 1]{{6}}\nlet w = Watch{{items: data[:], id: 1}}\nif flag {{ return }}\ndata[0] = 1 }}"
        )),
        "cannot assign to `data[_]` while it is borrowed",
    );
    accepts(&watch_body(
        "var data = [int; 1]{3}
        {
            let w = Watch{items: data[:], id: 1}
        }
        data[0] = 9",
    ));
}

#[test]
fn moving_or_dropping_the_value_ends_its_borrows() {
    accepts(&watch_body(
        "var data = [int; 1]{1}
        let w = Watch{items: data[:], id: 1}
        keep(w)
        data[0] = 5",
    ));
    accepts(&watch_body(
        "var data = [int; 1]{1}
        let w = Watch{items: data[:], id: 1}
        drop(w)
        data[0] = 5",
    ));
    accepts(&watch_body(
        "var a = [int; 1]{1}
        var b = [int; 1]{2}
        var w = Watch{items: a[:], id: 1}
        w = Watch{items: b[:], id: 2}
        a[0] = 10",
    ));
    rejects(
        &watch_body(
            "var a = [int; 1]{1}
            var b = [int; 1]{2}
            var w = Watch{items: a[:], id: 1}
            a[0] = 10
            w = Watch{items: b[:], id: 2}",
        ),
        "cannot assign to `a[_]` while it is borrowed",
    );
}

#[test]
fn the_viewed_storage_must_be_declared_before_the_observing_value() {
    rejects(
        &watch_body(
            "var first = [int; 1]{1}
            var w = Watch{items: first[:], id: 1}
            var data = [int; 1]{2}
            w = Watch{items: data[:], id: 2}",
        ),
        "`w` borrows `data`, which is dropped before `w`'s custom `drop` runs",
    );
    rejects(
        &watch_body("var w = Watch{items: [int; 1]{0}[:], id: 0}"),
        "temporary value does not live long enough",
    );
    accepts(&program(&format!(
        "{WATCH}\ntype Holder struct {{ w Watch }}\nfunc g() {{ var data = [int; 1]{{3}}\nlet h = Holder{{w: Watch{{items: data[:], id: 1}}}}\nvar heap = Array<Watch>{{Watch{{items: data[:], id: 2}}}} }}"
    )));
    rejects(
        &program(&format!(
            "{WATCH}\ntype Holder struct {{ w Watch }}\nfunc g() {{ var data = [int; 1]{{3}}\nlet h = Holder{{w: Watch{{items: data[:], id: 1}}}}\ndata[0] = 4 }}"
        )),
        "cannot assign to `data[_]` while it is borrowed",
    );
}

#[test]
fn a_collection_loop_borrows_its_collection_for_the_whole_loop() {
    accepts(&body(
        "var xs = Array<int>{1, 2}
        for x in xs { println(x + xs[0] + xs.len()) }
        xs.push(3)
        var m = map[string]int{\"a\": 1}
        for k, v in m { let found, w = m[k]\nprintln(v + w)\n_ = found }
        m[\"b\"] = 2",
    ));
    for (stmts, message) in [
        (
            "var xs = Array<int>{1}\nfor x in xs { xs.push(x) }",
            "cannot borrow `xs` as mutable because it is already borrowed",
        ),
        (
            "var xs = Array<int>{1}\nfor _ in xs { xs[0] = 2 }",
            "cannot assign to `xs[_]` while it is borrowed",
        ),
        (
            "var xs = Array<int>{1}\nfor i, _ in xs { let found, last = xs.pop()\n_ = found\n_ = last\nprintln(i) }",
            "cannot borrow `xs` as mutable",
        ),
        (
            "var xs = Array<int>{1}\nfor _ in xs { xs = Array<int>{} }",
            "cannot assign to `xs` while it is borrowed",
        ),
        (
            "var m = map[int]int{1: 2}\nfor k, v in m { m[k] = v }",
            "cannot borrow `m` as mutable",
        ),
        (
            "var m = map[int]int{1: 2}\nfor k, _ in m { let found, v = m.remove(k)\n_ = found\n_ = v }",
            "cannot borrow `m` as mutable",
        ),
        (
            "var d = [int; 2]{1, 2}\nvar s mut []int = d[:]\nfor x in d { s[0] = x }",
            "cannot borrow `d`",
        ),
    ] {
        let case = rejects(&body(stmts), message);
        let _ = case;
    }
    let case = rejects(
        &body("var xs = Array<int>{1}\nfor x in xs { xs.push(x) }"),
        "already borrowed",
    );
    assert!(
        case.checked.diagnostics[0]
            .notes()
            .iter()
            .any(|note| note.contains("the loop over `xs` keeps it borrowed until the loop ends"))
    );
}

#[test]
fn a_loop_item_is_a_shared_borrow_of_the_element() {
    let res = "type Res struct { id int }
        func (r mut Res) drop() {}
        func take(r own Res) {}
        func peek(r Res) {}
        func edit(r mut Res) {}";
    accepts(&program(&format!(
        "{res}\nfunc g(xs Array<Res>) {{ for r in xs {{ peek(r)\nprintln(r.id) }} }}"
    )));
    rejects(
        &program(&format!(
            "{res}\nfunc g(xs Array<Res>) {{ for r in xs {{ take(r) }} }}"
        )),
        "cannot move borrowed value `r`",
    );
    rejects(
        &program(&format!(
            "{res}\nfunc g(xs mut Array<Res>) {{ for r in xs {{ edit(r) }} }}"
        )),
        "cannot pass loop item `r` as a `mut` argument",
    );
    rejects(
        &program(&format!(
            "{res}\nfunc g(m map[int]Res) {{ for _, r in m {{ let kept = r\n_ = kept }} }}"
        )),
        "cannot move borrowed value `r`",
    );
}

#[test]
fn views_copied_from_loop_items_keep_the_element_provenance() {
    accepts(&body(
        "var x = [int; 2]{1, 2}\nvar keep []int = x[:0]
        { var views = Array<[]int>{x[:]}\nfor v in views { keep = v } }
        println(keep[1])",
    ));
    rejects(
        &body(
            "var x = [int; 2]{1, 2}\nvar keep []int = x[:0]
            { var views = Array<[]int>{x[:]}\nfor v in views { keep = v } }
            x[0] = 5\nprintln(keep[1])",
        ),
        "cannot assign to `x[_]` while it is borrowed",
    );
    accepts(&program(
        "func first(xs Array<[]int>) []int { for v in xs { return v }\nreturn xs[0] }",
    ));
    rejects(
        &body(
            "var keep []int = [int; 0]{}[:]
            { var x = [int; 2]{1, 2}\nvar views = Array<[]int>{x[:]}\nfor v in views { keep = v } }
            println(keep.len())",
        ),
        "does not live long enough",
    );
}

#[test]
fn push_and_pop_move_values_and_carry_views() {
    let res = "type Res struct { id int }\nfunc (r mut Res) drop() {}";
    rejects(
        &program(&format!(
            "{res}\nfunc g() {{ var xs = Array<Res>{{}}\nlet r = Res{{id: 1}}\nxs.push(r)\nprintln(r.id) }}"
        )),
        "use of moved value `r.id`",
    );
    rejects(
        &body(
            "var xs = Array<[]int>{}
            { var d = [int; 1]{1}\nxs.push(d[:]) }
            println(xs.len())",
        ),
        "`d` does not live long enough",
    );
    rejects(
        &body(
            "var d = [int; 1]{1}\nvar xs = Array<[]int>{}\nxs.push(d[:])
            let found, v = xs.pop()\n_ = found\nd[0] = 2\nprintln(v[0])",
        ),
        "cannot assign to `d[_]` while it is borrowed",
    );
    rejects(
        &body("var xs = Array<int>{1}\nvar s []int = xs[:]\nxs.push(2)\nprintln(s[0])"),
        "cannot borrow `xs` as mutable",
    );
    rejects(
        &program("func keep(xs mut Array<[]int>) { var d = [int; 1]{1}\nxs.push(d[:]) }"),
        "cannot store a view of local `d` into `xs`",
    );
}

#[test]
fn an_escaping_closure_owns_copies_and_moves_of_its_captures() {
    accepts(&program(
        "func counter() func() int { var count = 0\nreturn func() int { count += 1\nreturn count } }
        func adder(base int) func(int) int { let f = func(x int) int { return base + x }\nlet g = f\nreturn g }
        type Handler struct { run func(int) int }
        func handler(scale int) Handler { return Handler{run: func(x int) int { return x * scale }} }
        func keep(f own func() int) Array<func() int> { var out = Array<func() int>{}\nout.push(f)\nreturn out }",
    ));
    accepts(&body(
        "var seen = 1
        var fs = Array<func() int>{}
        fs.push(func() int { return seen + 1 })
        seen = 5
        println(seen + fs.len())",
    ));
    rejects(
        &body(
            "var count = 0
            var fs = Array<func()>{}
            fs.push(func() { count += 1 })
            println(count)",
        ),
        "use of moved value `count`",
    );
    rejects(
        &resource_body(
            "let r = Res{id: 1}
            var fs = Array<func()>{}
            fs.push(func() { show(r) })
            show(r)",
        ),
        "use of moved value `r`",
    );
}

#[test]
fn an_owning_closure_keeps_the_borrows_of_captured_views() {
    rejects(
        &program(
            "func view() func() int { var data = [int; 2]{1, 2}\nlet s = data[:]\nreturn func() int { return s[0] } }",
        ),
        "cannot return a view of local `data`",
    );
    accepts(&program(
        "func view(data []int) func() int { return func() int { return data[0] } }",
    ));
    rejects(
        &body(
            "var fs = Array<func() int>{}
            { var data = [int; 2]{1, 2}\nlet s = data[:]\nfs.push(func() int { return s[0] }) }
            println(fs.len())",
        ),
        "`data` does not live long enough",
    );
    accepts(&program(
        "func outer() func() int { var n = 1\nlet inner = func() int { return n }\nreturn inner }",
    ));
}

#[test]
fn an_owning_closure_cannot_view_one_capture_from_another() {
    rejects(
        &program(
            "func f(empty []int) func() int { var data = [int; 2]{1, 2}\nvar keep = empty
            return func() int { keep = data[:]\nreturn keep.len() } }",
        ),
        "an owning closure cannot store a view of one captured value in another",
    );
}

#[test]
fn a_borrowing_closure_still_cannot_outlive_its_captures() {
    rejects(
        &body(
            "var keep = func() int { return 0 }
            { let n = 1\nkeep = func() int { return n } }
            println(keep())",
        ),
        "`n` does not live long enough",
    );
}
