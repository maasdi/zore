use super::{accepts, program, rejects};

#[test]
fn async_captures_keep_loans_across_suspension_and_release_after_last_use() {
    let prelude = "async func pause() {}\n";
    accepts(&program(&format!(
        r#"{prelude}
async func work() int {{
    var count = 1
    let read = async func() int {{ await pause(); return count }}
    let first = await read()
    count += first
    let bump = async func() {{ await pause(); count += 1 }}
    await bump()
    await pause()
    await bump()
    return count
}}"#
    )));
    for (body, expected) in [
        (
            "let f = async func() { await pause(); count += 1 }; println(count); await f()",
            "borrowed",
        ),
        (
            "let f = async func() int { await pause(); return count }; count += 1; println(await f())",
            "borrowed",
        ),
        (
            "let inner = async func() { count += 1 }; let outer = async func() { await inner() }; await inner(); await outer()",
            "borrowed",
        ),
        (
            "let f = async func() { count += 1 }; let both = async func(a async func(), b async func()) { await a(); await b() }; await both(f, f)",
            "borrowed",
        ),
    ] {
        rejects(
            &program(&format!(
                "{prelude}\nasync func work() {{ var count = 1; {body} }}"
            )),
            expected,
        );
    }
    rejects(
        &program(
            "func create() async func() int { let backing = Array<int>{1}; let view = backing[:]; return async func() int { return view[0] } }",
        ),
        "cannot return a view",
    );
    rejects(
        &program(
            "async func work() []int { let f = async func() []int { let backing = Array<int>{1}; return backing[:] }; return await f() }",
        ),
        "cannot return a view",
    );
}

#[test]
fn async_call_once_and_spawned_inputs_follow_ordinary_ownership_rules() {
    let prelude = "type Resource struct { N int }\nfunc (r mut Resource) drop() {}\nfunc consume(r own Resource) {}\n";
    accepts(&program(&format!(
        r#"{prelude}
async func work() {{
    let r = Resource{{N: 1}}
    let once = async func() {{ consume(r) }}
    await once()
}}
func start() {{
    let r = Resource{{N: 2}}
    let once = async func() {{ consume(r) }}
    let task = go once()
    task.wait()
}}"#
    )));
    for (body, expected) in [
        (
            "let r = Resource{N: 1}; let once = async func() { consume(r) }; await once(); await once()",
            "moved value",
        ),
        (
            "let r = Resource{N: 1}; let once = async func() { consume(r) }; let alias = once",
            "can only be called directly",
        ),
        (
            "var count = 1; let f = async func() { count += 1 }; let t = go f(); drop(t)",
            "spawned closure cannot change",
        ),
        (
            "let backing = Array<int>{1}; let view = backing[:]; let f = async func() int { return view[0] }; let t = go f(); println(await t)",
            "holds a view",
        ),
        (
            "let r = Resource{N: 1}; let f = async func() int { return r.N }; let t = go f(); println(r.N); println(await t)",
            "moved value",
        ),
    ] {
        rejects(
            &program(&format!("{prelude}\nasync func work() {{ {body} }}")),
            expected,
        );
    }
    rejects(
        &program(&format!(
            "{prelude}\nfunc start(r Resource) {{ let f = async func() int {{ return r.N }}; let t = go f(); println(t.wait()) }}"
        )),
        "cannot move borrowed",
    );
    rejects(
        &program(
            "func start(n mut int) { let f = async func() int { return n }; let t = go f(); println(t.wait()) }",
        ),
        "`mut` parameter",
    );
    rejects(
        &program("func start(f async func()) { let t = go f(); t.wait() }"),
        "parameter",
    );
}

#[test]
fn async_method_captures_and_writeback_keep_receiver_and_backing_exclusive() {
    let prelude = "type Counter struct { N int }\nasync func (c Counter) read() int { return c.N }\nasync func (c mut Counter) bump() { c.N += 1 }\n";
    for body in [
        "let f = c.read; c.N += 1; println(await f())",
        "let f = c.bump; println(c.N); await f()",
    ] {
        rejects(
            &program(&format!(
                "{prelude}\nasync func work() {{ var c = Counter{{N: 1}}; {body} }}"
            )),
            "borrowed",
        );
    }
    accepts(&program(
        "async func work() { let backing = Array<int>{1}; var view = backing[:]; let f = async func() { view = backing[:] }; await f(); println(view[0]) }",
    ));
    rejects(
        &program(
            "async func work() { let backing = Array<int>{1}; var view = backing[:]; let f = async func() { let temporary = Array<int>{2}; view = temporary[:] }; await f() }",
        ),
        "local",
    );
    rejects(
        &program(
            "async func work() { let backing = Array<int>{1}; var view = backing[:]; let f = async func(source []int) { view = source }; await f(backing[:]) }",
        ),
        "parameter into captured",
    );
}
