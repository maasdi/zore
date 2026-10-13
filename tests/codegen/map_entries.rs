use super::{prints, run, stderr, stdout};

#[test]
fn map_entries_support_interface_borrows_generic_callbacks_and_callback_variables() {
    prints(
        r#"package main
type Counter interface { Read() int; mut Add(value int) }
type OwnedCounter struct { value int }
func (c mut OwnedCounter) drop() { println(c.value) }
func (c OwnedCounter) Read() int { return c.value }
func (c mut OwnedCounter) Add(value int) { c.value += value }
func inspect<T ordered>(values map[int]T, callback func(T) T) T {
    let found, value = values.withEntry(1, callback)
    println(found)
    return value
}
func main() {
    var counters = map[int]Counter{1: OwnedCounter{value: 3}}
    let found, before = counters.withEntry(1, func(c Counter) int { return c.Read() })
    println(found)
    println(before)
    let changed = counters.withEntryMut(1, func(c mut Counter) { c.Add(2) })
    println(changed)
    let present, after = counters.withEntry(1, func(c Counter) int { return c.Read() })
    println(present)
    println(after)
    let numbers = map[int]int{1: 4}
    var count = 0
    let callback = func(value int) int { count += 1; return value * 2 }
    println(inspect(numbers, callback))
    println(inspect(numbers, callback))
    println(count)
}
"#,
        "true\n3\ntrue\ntrue\n5\ntrue\n8\ntrue\n8\n2\n5\n",
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
    let found, old = resources.withEntry("main", func(r Resource) int { return r.values[0] })
    println(found)
    println(old)
    let edited = resources.withEntryMut("main", func(r mut Resource) { r.values[0] += 5 })
    println(edited)
    let present, current = resources.withEntry("main", func(r Resource) int { return r.values[0] })
    println(present)
    println(current)
    println(resources.len())
    let removed, owned = resources.remove("main")
    println(removed)
    println(owned.values[0])
}
"#,
        "true\n10\ntrue\ntrue\n15\n1\ntrue\n15\n1\n",
    );
}

#[test]
fn map_entries_misses_evaluate_arguments_once_and_return_zero_results() {
    prints(
        r#"package main
func key() string { println("key"); return "missing" }
func receiver() map[string]int { println("receiver"); return map[string]int{"hit": 5} }
func callback() func(int) (string, int, error) {
    println("callback")
    return func(value int) (string, int, error) { println("must not run"); return "bad", value, error("bad") }
}
func main() {
    let numbers = map[string]int{"hit": 5}
    let found, text, number, err = receiver().withEntry(key(), callback())
    println(found)
    println(text)
    println(number)
    println(err == nil)
    let present, answer = numbers.withEntry("hit", func(value int) int { return value + 1 })
    println(present)
    println(answer)
}
"#,
        "receiver\nkey\ncallback\nfalse\n\n0\ntrue\ntrue\n6\n",
    );
}

#[test]
fn map_entries_owned_results_and_error_propagation_preserve_mutations_and_cleanup() {
    prints(
        r#"package main
type Resource struct { id int }
func (r mut Resource) drop() { if r.id != 0 { println(r.id) } }
func update(resources mut map[string]Resource) error {
    let found = resources.withEntryMut("main", func(r mut Resource) error {
        r.id = 2
        let temporary = Resource{id: 9}
        return error("failure")
    })?
    println(found)
    return nil
}
func main() {
    var resources = map[string]Resource{"main": Resource{id: 1}}
    let err = update(resources)
    println(err != nil)
    let found, result = resources.withEntry("main", func(r Resource) Resource { return Resource{id: r.id + 1} })
    println(found)
    println(result.id)
    let missing, zero = resources.withEntry("missing", func(r Resource) Resource { return Resource{id: 7} })
    println(missing)
    println(zero.id)
}
"#,
        "9\ntrue\ntrue\n3\nfalse\n0\n3\n2\n",
    );
}

#[test]
fn map_entries_mutable_panic_and_replacement_panic_drop_each_initialized_part_once() {
    for (body, expected) in [
        ("r.id = 3; panic(\"callback failed\")", "3\n2\n"),
        (
            "r = Resource{id: 3}; panic(\"callback failed\")",
            "1\n3\n2\n",
        ),
        ("r = Resource{id: 3}", "1\n3\n2\n"),
    ] {
        let replacement_panics = body == "r = Resource{id: 3}";
        let source = format!(
            r#"package main
type Resource struct {{ id int }}
func (r mut Resource) drop() {{ println(r.id); if {replacement_panics} && r.id == 1 {{ panic("replacement failed") }} }}
func main() {{
    var resources = map[string]Resource{{"selected": Resource{{id: 1}}, "other": Resource{{id: 2}}}}
    let found = resources.withEntryMut("selected", func(r mut Resource) {{ {body} }})
    println(found)
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
    let found = values.withEntryMut(1, func(value mut Parts) { value.first = Resource{id: 3} })
    println(found)
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
fn map_entries_copy_text_shared_views_callbacks_and_async_arguments_run_normally() {
    prints(
        r#"package main
func main() {
    var values = map[int]int{1: 2}
    let changed = values.withEntryMut(1, func(value mut int) { value += 1 })
    println(changed)
    let found, current = values[1]
    println(found)
    println(current)
}
"#,
        "true\ntrue\n3\n",
    );
    let failed = run(r#"package main
type Owner struct { values map[int]int }
func (owner mut Owner) drop() { println(owner.values.len()) }
func main() {
    var owner = Owner{values: map[int]int{1: 2, 2: 3}}
    let changed = owner.values.withEntryMut(1, func(value mut int) { value = 4; panic("copy callback failed") })
    println(changed)
}
"#);
    assert_eq!(failed.status.code(), Some(2), "{failed:?}");
    assert_eq!(stdout(&failed), "1\n", "{failed:?}");
    assert!(
        stderr(&failed).contains("copy callback failed"),
        "{failed:?}"
    );
    prints(
        r#"package main
async func key() string { return "entry" }
async func check() {
    var texts = map[string]string{"entry": "initial" + " value"}
    let found, text = texts.withEntry(await key(), func(value string) string { return value + " read" })
    println(found)
    println(text)
    let changed = texts.withEntryMut("entry", func(value mut string) { value = "new" + " value" })
    println(changed)
    let present, copied = texts["entry"]
    println(present)
    println(copied)
}
func main() {
    let work = go check()
    work.wait()
    let backing = Array<int>{7}
    let views = map[string][]int{"entry": backing[:]}
    let found, value = views.withEntry("entry", func(view []int) int { return view[0] })
    println(found)
    println(value)
}
"#,
        "true\ninitial value read\ntrue\ntrue\nnew value\ntrue\n7\n",
    );
}
