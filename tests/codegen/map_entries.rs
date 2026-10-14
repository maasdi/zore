use super::{prints, run, stderr, stdout};

#[test]
fn map_entries_support_interface_borrows_and_generic_functions() {
    prints(
        r#"package main
type Counter interface { Read() int; mut Add(value int) }
type OwnedCounter struct { value int }
func (c mut OwnedCounter) drop() { println(c.value) }
func (c OwnedCounter) Read() int { return c.value }
func (c mut OwnedCounter) Add(value int) { c.value += value }
func inspect<T ordered>(values map[int]T, fallback T) T {
    if value = values.entry(1) { return value }
    return fallback
}
func main() {
    var counters = map[int]Counter{1: OwnedCounter{value: 3}}
    if c = counters.entry(1) { println(c.Read()) }
    if mut c = counters.entry(1) { c.Add(2) }
    if c = counters.entry(1) { println(c.Read()) }
    let numbers = map[int]int{1: 4}
    println(inspect(numbers, 0))
    println(inspect(map[int]int{}, 9))
}
"#,
        "3\n5\n4\n9\n5\n",
    );
}

#[test]
fn map_entries_read_and_edit_move_resources_without_removing_or_cloning() {
    prints(
        r#"package main
type Resource struct { id int; values Array<int> }
func (r mut Resource) drop() { println(r.id) }
func main() {
    var resources = map[string]Resource{"main": Resource{id: 1, values: Array<int>{10}}}
    if r = resources.entry("main") { println(r.values[0]) }
    if mut r = resources.entry("main") { r.values[0] += 5 }
    if r = resources.entry("main") { println(r.values[0]) }
    println(resources.len())
    let removed, owned = resources.remove("main")
    println(removed)
    println(owned.values[0])
}
"#,
        "10\n15\n1\ntrue\n15\n1\n",
    );
}

#[test]
fn map_entries_misses_evaluate_once_release_borrows_and_select_else_branches() {
    prints(
        r#"package main
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
func key() string { println("key"); return "missing" }
func receiver() map[string]Resource { println("receiver"); return map[string]Resource{"hit": Resource{id: 5}} }
func main() {
    if r = receiver().entry(key()) { panic("must not run") } else { println("missing") }
    if r = receiver().entry("hit") { println(r.id) }
    var values = map[int]int{1: 2}
    if mut value = values.entry(3) { panic("must not run") } else { values[3] = 7 }
    if value = values.entry(4) { panic("must not run") } else if value = values.entry(3) { println(value) } else { panic("must not run") }
    if value = values.entry(4) { panic("must not run") }
    println(values.len())
}
"#,
        "receiver\nkey\n5\nmissing\nreceiver\n5\n5\n7\n2\n",
    );
}

#[test]
fn map_entries_return_errors_break_and_continue_preserve_mutations_and_cleanup() {
    prints(
        r#"package main
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
func failure() error { return error("failure") }
func update(resources mut map[string]Resource) error {
    if mut r = resources.entry("main") {
        r.id = 2
        let temporary = Resource{id: 9}
        failure()?
    }
    return nil
}
func create(resources map[string]Resource) Resource {
    if r = resources.entry("main") { return Resource{id: r.id + 1} }
    return Resource{id: 7}
}
func main() {
    var resources = map[string]Resource{"main": Resource{id: 1}}
    let err = update(resources)
    println(err != nil)
    let result = create(resources)
    println(result.id)
    for var index = 0; index < 2; index += 1 {
        if mut r = resources.entry("main") { r.id += 1; continue }
    }
    for { if mut r = resources.entry("main") { r.id += 1; break } }
    resources["next"] = Resource{id: 6}
    if r = resources.entry("main") { println(r.id) }
}
"#,
        "9\ntrue\n3\n5\n3\n5\n6\n",
    );
}

#[test]
fn map_entries_mutable_panic_and_replacement_panic_drop_each_initialized_part_once() {
    for (body, expected) in [
        ("r.id = 3; panic(\"entry failed\")", "3\n2\n"),
        ("r = Resource{id: 3}; panic(\"entry failed\")", "1\n3\n2\n"),
        ("r = Resource{id: 3}", "1\n3\n2\n"),
    ] {
        let replacement_panics = body == "r = Resource{id: 3}";
        let source = format!(
            r#"package main
type Resource struct {{ id int }}
func (r mut Resource) drop() {{ println(r.id); if {replacement_panics} && r.id == 1 {{ panic("replacement failed") }} }}
func main() {{
    var resources = map[string]Resource{{"selected": Resource{{id: 1}}, "other": Resource{{id: 2}}}}
    if mut r = resources.entry("selected") {{ {body} }}
}}
"#
        );
        let output = run(&source);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(stdout(&output), expected, "{output:?}");
        assert!(!stderr(&output).contains("leak:"), "{output:?}");
    }
    let output = run(r#"package main
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id); if r.id == 1 { panic("field replacement failed") } }
type Parts struct { first Resource; last Resource }
func main() {
    var values = map[int]Parts{1: Parts{first: Resource{id: 1}, last: Resource{id: 2}}, 2: Parts{first: Resource{id: 4}, last: Resource{id: 5}}}
    if mut value = values.entry(1) { value.first = Resource{id: 3} }
}
"#);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(stdout(&output), "1\n2\n3\n5\n4\n", "{output:?}");
    assert!(
        stderr(&output).contains("field replacement failed"),
        "{output:?}"
    );
    assert!(!stderr(&output).contains("leak:"), "{output:?}");
}

#[test]
fn map_entries_copy_text_shared_views_and_awaited_keys_and_bodies_run_normally() {
    prints(
        r#"package main
async func key() string { return "entry" }
async func suffix() string { return " read" }
async func check() {
    var texts = map[string]string{"entry": "initial" + " value"}
    if value = texts.entry(await key()) { println(value + await suffix()) }
    if mut value = texts.entry(await key()) { value = "new" + await suffix() }
    let present, copied = texts["entry"]
    println(present)
    println(copied)
}
func main() {
    let work = go check()
    work.wait()
    let backing = Array<int>{7}
    let views = map[string][]int{"entry": backing[:]}
    if view = views.entry("entry") { println(view[0]) }
    var values = map[int]int{1: 2}
    if mut value = values.entry(1) { value += 1 }
    let found, current = values[1]
    println(found)
    println(current)
}
"#,
        "initial value read\ntrue\nnew read\n7\ntrue\n3\n",
    );
    let failed = run(r#"package main
type Owner struct { values map[int]int }
func (owner mut Owner) drop() { println(owner.values.len()) }
func main() {
    var owner = Owner{values: map[int]int{1: 2, 2: 3}}
    if mut value = owner.values.entry(1) { value = 4; panic("copy entry failed") }
}
"#);
    assert_eq!(failed.status.code(), Some(2), "{failed:?}");
    assert_eq!(stdout(&failed), "1\n", "{failed:?}");
    assert!(stderr(&failed).contains("copy entry failed"), "{failed:?}");
}

#[test]
fn map_entries_nested_guards_and_scope_destructor_panics_clean_up_in_order() {
    for (source, expected) in [
        (
            r#"package main
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id); if r.id == 9 { panic("return temporary failed") } }
type Owner struct { values map[int]Resource }
func (owner mut Owner) drop() { println(owner.values.len()) }
func finish(temporary Resource) error { return nil }
func work(values mut map[int]Resource) error {
    if mut r = values.entry(1) { r.id = 3; return finish(Resource{id: 9}) }
    return nil
}
func main() {
    var owner = Owner{values: map[int]Resource{1: Resource{id: 1}, 2: Resource{id: 2}}}
    let err = work(owner.values)
    println(err == nil)
}
"#,
            "9\n3\n1\n2\n",
        ),
        (
            r#"package main
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
func main() {
    var values = map[int]map[int]Resource{1: map[int]Resource{1: Resource{id: 1}, 2: Resource{id: 2}}, 2: map[int]Resource{1: Resource{id: 3}}}
    if mut inner = values.entry(1) {
        if mut r = inner.entry(1) { r.id = 4; panic("nested failure") }
    }
}
"#,
            "4\n2\n3\n",
        ),
        (
            r#"package main
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id); if r.id == 9 { panic("local drop failed") } }
func main() {
    var values = map[int]Resource{1: Resource{id: 1}, 2: Resource{id: 2}}
    if mut r = values.entry(1) { let temporary = Resource{id: 9} }
}
"#,
            "9\n1\n2\n",
        ),
        (
            r#"package main
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
func main() {
    var values = map[int]Resource{1: Resource{id: 1}, 2: Resource{id: 2}}
    if mut r = values.entry(1) { r.id = 3 }
    panic("after scope")
}
"#,
            "3\n2\n",
        ),
    ] {
        let output = run(source);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(stdout(&output), expected, "{output:?}");
        assert!(!stderr(&output).contains("leak:"), "{output:?}");
    }
}

#[test]
fn map_entries_async_move_cleanup_survives_timer_suspension_and_key_panics() {
    for (header, body, expected) in [
        (
            "values.entry(1)",
            "time.Sleep(time.Millisecond); r.id = 3; panic(\"entry failed\")",
            "3\n2\n",
        ),
        (
            "values.entry(await failKey())",
            "panic(\"must not run\")",
            "1\n2\n",
        ),
    ] {
        let source = format!(
            r#"package main
import "std/time"
type Resource struct {{ id int }}
func (r mut Resource) drop() {{ println(r.id) }}
async func failKey() int {{ time.Sleep(time.Millisecond); panic("key failed"); return 1 }}
async func work() {{
    var values = map[int]Resource{{1: Resource{{id: 1}}, 2: Resource{{id: 2}}}}
    if mut r = {header} {{ {body} }}
}}
func main() {{ let task = go work(); task.wait() }}
"#
        );
        let output = run(&source);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(stdout(&output), expected, "{output:?}");
        assert!(!stderr(&output).contains("leak:"), "{output:?}");
    }
}
