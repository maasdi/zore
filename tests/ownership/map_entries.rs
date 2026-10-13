use super::{accepts, body, rejects};

#[test]
fn map_entries_pair_shared_and_mutable_access_with_alias_rejections() {
    accepts(&body(
        r#"
        var values = map[string]Array<int>{"entry": Array<int>{1}}
        let read = values.withEntry("entry", func(v Array<int>) { println(v[0]); println(values.len()) })
        let edited = values.withEntryMut("entry", func(v mut Array<int>) { v.push(2) })
        println(read)
        println(edited)
    "#,
    ));
    for access in [
        r#"values.withEntryMut("entry", func(v mut Array<int>) { println(values.len()) })"#,
        r#"values.withEntry("entry", func(v Array<int>) { let found, old = values.remove("entry"); println(found) })"#,
        r#"values.withEntryMut("entry", func(v mut Array<int>) { values["other"] = Array<int>{2} })"#,
        r#"values.withEntryMut("entry", func(v mut Array<int>) { let found = values.withEntry("other", func(other Array<int>) {}); println(found) })"#,
    ] {
        rejects(
            &body(&format!(
                r#"var values = map[string]Array<int>{{"entry": Array<int>{{1}}}}
            let found = {access}
            println(found)"#
            )),
            "borrow",
        );
    }
    rejects(
        &body(
            r#"let values = map[int]int{1: 2}
        let found = values.withEntryMut(1, func(v mut int) { v += 1 })
        println(found)"#,
        ),
        "mutable",
    );
    rejects(
        r#"package main
        func key(values mut Array<map[int]int>) int { let found, removed = values.pop(); println(found); return 1 }
        func main() {
            var values = Array<map[int]int>{map[int]int{1: 2}}
            let found = values[0].withEntry(key(values), func(value int) {})
            println(found)
        }"#,
        "borrow",
    );
}

#[test]
fn map_entries_reject_borrow_escape_moves_and_spawned_borrows() {
    for (callback, expected) in [
        (
            "func(v Array<int>) { let stolen = v; println(stolen.len()) }",
            "borrow",
        ),
        (
            "func(v Array<int>) []int { return v[:] }",
            "cannot return borrowed views",
        ),
        (
            "func(v Array<int>) Array<[]int> { return Array<[]int>{v[:]} }",
            "cannot return borrowed views",
        ),
        (
            "func(v Array<int>) { saved = v[:] }",
            "parameter into captured",
        ),
    ] {
        let text = body(&format!(
            r#"let backing = Array<int>{{0}}
            var saved = backing[:]
            let values = map[int]Array<int>{{1: Array<int>{{2}}}}
            let found = values.withEntry(1, {callback})
            println(found)
            println(saved[0])"#
        ));
        rejects(&text, expected);
    }
    rejects(
        r#"package main
        func work(value Array<int>) {}
        func main() {
            let values = map[int]Array<int>{1: Array<int>{2}}
            let found = values.withEntry(1, func(v Array<int>) { let task = go work(v); task.wait() })
            println(found)
        }"#,
        "borrow",
    );
}

#[test]
fn map_entries_shared_external_views_keep_provenance_and_mutable_view_entries_are_rejected() {
    accepts(&body(
        r#"var backing = Array<int>{7}
        let views = map[int][]int{1: backing[:]}
        let found, number = views.withEntry(1, func(v []int) int { return v[0] })
        println(found)
        println(number)
        backing[0] = 8
    "#,
    ));
    rejects(
        &body(
            r#"var backing = Array<int>{7}
        let views = map[int][]int{1: backing[:]}
        let found = views.withEntry(1, func(v []int) { backing[0] = 8; println(v[0]) })
        println(found)
    "#,
        ),
        "borrow",
    );
    rejects(
        &body(
            r#"let backing = Array<int>{7}
        var views = map[int][]int{1: backing[:]}
        let found = views.withEntryMut(1, func(v mut []int) {})
        println(found)
    "#,
        ),
        "mutable entries cannot contain borrowed views",
    );
    rejects(
        &body(
            r#"var backing = Array<int>{7}
        let views = map[int]mut []int{1: backing[:]}
        let found = views.withEntry(1, func(v mut []int) {})
        println(found)
    "#,
        ),
        "mutable views",
    );
}

#[test]
fn map_entries_reject_invalid_callback_signatures_and_result_types() {
    for callback in [
        "func(v mut int) {}",
        "func(v own int) {}",
        "func() {}",
        "func(v int, second int) {}",
        "5",
        "func(v string) {}",
    ] {
        rejects(
            &body(&format!(
                "let values = map[int]int{{1: 2}}\nlet found = values.withEntry(1, {callback})\nprintln(found)"
            )),
            "synchronous function",
        );
    }
    rejects(
        r#"package main
        async func callback(value int) {}
        func main() { let values = map[int]int{1: 2}; let found = values.withEntry(1, callback); println(found) }
    "#,
        "synchronous function",
    );
    for call in [
        "values.withEntry(1)",
        "values.withEntry(1, func(v int) {}, 2)",
        "values.withEntryMut(1)",
    ] {
        rejects(
            &body(&format!(
                "var values = map[int]int{{1: 2}}\nlet found = {call}\nprintln(found)"
            )),
            "takes 2 arguments",
        );
    }
    rejects(
        &body(
            "let values = map[int]int{1: 2}\nlet found = values.withEntry(\"wrong\", func(v int) {})\nprintln(found)",
        ),
        "int",
    );
}

#[test]
fn map_entries_validate_specialized_generic_types_and_preserve_call_once_restrictions() {
    rejects(
        r#"package main
        func touch<T any>(values mut map[int]T) {
            let found = values.withEntryMut(1, func(value mut T) {})
            println(found)
        }
        func main() {
            let backing = Array<int>{1}
            var views = map[int][]int{1: backing[:]}
            touch(views)
        }"#,
        "mutable entries cannot contain borrowed views",
    );
    rejects(
        r#"package main
        func inspect<T any>(values map[int]int, callback func(int) T) {
            let found, result = values.withEntry(1, callback)
            println(found)
        }
        func main() {
            let backing = Array<int>{1}
            let values = map[int]int{1: 2}
            inspect(values, func(value int) []int { return backing[:] })
        }"#,
        "cannot return borrowed views",
    );
    rejects(
        &body(
            r#"let owned = Array<int>{1}
        let callback = func(value int) Array<int> { return owned }
        let values = map[int]int{1: 2}
        let found, result = values.withEntry(1, callback)
        println(found)
    "#,
        ),
        "call-once",
    );
}
