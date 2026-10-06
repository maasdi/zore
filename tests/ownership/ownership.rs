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
fn views_cannot_be_stored_through_parameters_or_slice_elements_yet() {
    rejects(
        &program(
            "type View struct { Items []int }
            func fill(out mut View, items []int) { out.Items = items }",
        ),
        "storing a borrowed view through `out.Items` is not supported yet",
    );
    rejects(
        &program("func set(rows mut [][]int, items []int) { rows[0] = items }"),
        "storing a borrowed view through `rows[_]` is not supported yet",
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
    rejects(
        &program("func fill(views mut Array<[]int>, items []int) { views[0] = items }"),
        "storing a borrowed view through `views[_]` is not supported yet",
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
        &program("func fill(m mut map[string][]int, items []int) { m[\"a\"] = items }"),
        "storing a borrowed view through `m` is not supported yet",
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
fn captured_move_values_are_borrowed_never_consumed() {
    accepts(&resource_body(
        "let r = Res{id: 1}
        let f = func() { show(r) }
        f()
        f()
        take(r)",
    ));
    rejects(
        &resource_body(
            "let r = Res{id: 1}
            let f = func() { take(r) }
            f()",
        ),
        "moving captured value `r` out of a function literal is not supported yet",
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
    rejects(
        &body(
            "var f = func() {}
            let g = func() { f = func() {} }
            g()",
        ),
        "storing a borrowed view through `f` is not supported yet",
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
