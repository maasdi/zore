use super::{accepts, body, rejects};

#[test]
fn map_entries_pair_shared_and_mutable_access_with_alias_rejections() {
    accepts(&body(
        r#"
        var values = map[string]Array<int>{"entry": Array<int>{1}}
        if v = values.entry("entry") { println(v[0]); println(values.len()) }
        if mut v = values.entry("entry") { v.push(2) }
        values["other"] = Array<int>{3}
        if mut v = values.entry("missing") { v.push(4) } else { values["missing"] = Array<int>{5} }
    "#,
    ));
    for access in [
        r#"if mut v = values.entry("entry") { println(values.len()) }"#,
        r#"if v = values.entry("entry") { let found, old = values.remove("entry"); println(found) }"#,
        r#"if mut v = values.entry("entry") { values["other"] = Array<int>{2} }"#,
        r#"if mut v = values.entry("entry") { if other = values.entry("other") { println(other.len()) } }"#,
        r#"if v = values.entry("entry") { v.push(2) }"#,
        r#"if mut v = values.entry("entry") { let read = func() { println(values.len()) }; read() }"#,
    ] {
        rejects(
            &body(&format!(
                r#"var values = map[string]Array<int>{{"entry": Array<int>{{1}}}}
            {access}"#
            )),
            if access.contains("v.push") {
                "mut"
            } else {
                "borrow"
            },
        );
    }
    rejects(
        &body("let values = map[int]int{1: 2}\nif mut v = values.entry(1) { v += 1 }"),
        "mutable",
    );
    rejects(
        r#"package main
        func key(values mut Array<map[int]int>) int { let found, removed = values.pop(); println(found); return 1 }
        func main() {
            var values = Array<map[int]int>{map[int]int{1: 2}}
            if value = values[0].entry(key(values)) { println(value) }
        }"#,
        "borrow",
    );
}

#[test]
fn map_entries_reject_borrow_escape_moves_and_spawned_borrows() {
    rejects(
        &body(
            "let values = map[int]Array<int>{1: Array<int>{2}}\nif v = values.entry(1) { let stolen = v; println(stolen.len()) }",
        ),
        "cannot move borrowed",
    );
    rejects(
        r#"package main
        func escape(values map[int]Array<int>, fallback []int) []int {
            if v = values.entry(1) { return v[:] }
            return fallback
        }
        func main() {}"#,
        "cannot return a view of local `v`",
    );
    for operation in ["saved = v[:]", "saved = Array<[]int>{v[:]}[0]"] {
        rejects(
            &body(&format!(
                r#"let backing = Array<int>{{0}}
            var saved = backing[:]
            let values = map[int]Array<int>{{1: Array<int>{{2}}}}
            if v = values.entry(1) {{ {operation} }}
            println(saved[0])"#
            )),
            "does not live long enough",
        );
    }
    rejects(
        r#"package main
        func work(value Array<int>) {}
        func main() {
            let values = map[int]Array<int>{1: Array<int>{2}}
            if v = values.entry(1) { let task = go work(v); task.wait() }
        }"#,
        "borrow",
    );
    rejects(
        r#"package main
        func escape(values map[int]Array<int>) func() int {
            if v = values.entry(1) { return func() int { return v[0] } }
            return func() int { return 0 }
        }
        func main() {}"#,
        "cannot move borrowed",
    );
}

#[test]
fn map_entries_shared_external_views_keep_provenance_and_mutable_view_entries_are_rejected() {
    accepts(&body(
        r#"var backing = Array<int>{7}
        let views = map[int][]int{1: backing[:]}
        if v = views.entry(1) { println(v[0]) }
        backing[0] = 8
    "#,
    ));
    rejects(
        &body(
            r#"var backing = Array<int>{7}
        let views = map[int][]int{1: backing[:]}
        if v = views.entry(1) { backing[0] = 8; println(v[0]) }
    "#,
        ),
        "borrow",
    );
    rejects(
        &body(
            r#"let backing = Array<int>{7}
        var views = map[int][]int{1: backing[:]}
        if mut v = views.entry(1) { println(v[0]) }
    "#,
        ),
        "mutable entries cannot contain borrowed views",
    );
    rejects(
        &body(
            r#"var backing = Array<int>{7}
        let views = map[int]mut []int{1: backing[:]}
        if v = views.entry(1) { println(v[0]) }
    "#,
        ),
        "mutable views",
    );
}

#[test]
fn map_entries_validate_headers_scope_and_key_types() {
    for (header, expected) in [
        ("if v = values.entry() {}", "takes 1 argument"),
        ("if v = values.entry(1, 2) {}", "takes 1 argument"),
        ("if v = values.entry(\"wrong\") {}", "int"),
        ("if v = 5 {}", "requires `map.entry(key)`"),
        ("if v = values.remove(1) {}", "requires `map.entry(key)`"),
        ("if v = numbers.entry(1) {}", "map receiver"),
        ("if mut v = values.entry(1) { let v = 3 }", "duplicate"),
        (
            "if v = values.entry(1) {} else { println(v) }",
            "cannot find",
        ),
        ("if v = values.entry(1) {}\nprintln(v)", "cannot find"),
        ("let v = values.entry(1)", "entry"),
        (
            "let found = values.withEntry(1, func(v int) {})",
            "withEntry",
        ),
        ("if _ = values.entry(1) {}", "cannot be used as a value"),
        ("if let v = values.entry(1) {}", "expected"),
        ("if var v = values.entry(1) {}", "expected"),
        ("if (v = values.entry(1)) {}", "expected"),
    ] {
        rejects(
            &body(&format!(
                "var values = map[int]int{{1: 2}}\nlet numbers = Array<int>{{1}}\n{header}"
            )),
            expected,
        );
    }
    accepts(&body(
        r#"let value = 1
        let values = map[int]int{1: 2}
        if value = values.entry(value) { println(value) } else if missing = values.entry(3) { println(missing) } else { println(value) }
        println(value)
        if value == 1 { println(value) }
    "#,
    ));
}

#[test]
fn map_entries_validate_generic_types_and_allow_independent_control_flow() {
    rejects(
        r#"package main
        func touch<T any>(values mut map[int]T) { if mut value = values.entry(1) {} }
        func main() { let backing = Array<int>{1}; var views = map[int][]int{1: backing[:]}; touch(views) }
    "#,
        "mutable entries cannot contain borrowed views",
    );
    accepts(
        r#"package main
        func inspect<T ordered>(values map[int]T, fallback T) T {
            if value = values.entry(1) { return value }
            return fallback
        }
        func stop() error { return error("stop") }
        func update(values mut map[int]int) error {
            if mut value = values.entry(1) { value += 1; stop()? }
            return nil
        }
        async func key() int { return 1 }
        async func run() {
            var values = map[int]int{1: 2}
            if mut value = values.entry(await key()) { value += await key() }
        }
        func main() {
            var values = map[int]int{1: 2}
            println(inspect(values, 0))
            let err = update(values)
            println(err != nil)
            for { if mut value = values.entry(1) { value += 1; break } }
            values[2] = 3
        }
    "#,
    );
}
