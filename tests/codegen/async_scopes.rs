use super::*;

#[test]
fn async_scope_awaited_error_joins_the_remaining_child_before_propagation() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("parent drop") }
async func read(v Owner, gate channel<bool>) { let _, _ = gate.receive(); println(v.Values[0]) }
async func fail(gate channel<bool>) (int, error) { let _, _ = gate.receive(); return 9, error("stop") }
async func parent(ready channel<bool>, a channel<bool>, b channel<bool>) (int, error) {
    scope {
        let owner = Owner{Values: Array<int>{7}}
        let failed = go fail(a)
        go read(owner, b)
        ready.send(true)
        let value = await failed?
        return value, nil
    }
}
func main() {
    let ready = channel<bool>()
    let a = channel<bool>()
    let b = channel<bool>()
    let child = go parent(ready, a, b)
    let _, _ = ready.receive()
    waitPending(3)
    a.close()
    waitIdle(2)
    waitPending(2)
    println("waiting remainder")
    b.close()
    let value, err = child.wait()
    println(value)
    println(err == error("stop"))
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "waiting remainder\n7\nparent drop\n0\ntrue\n"
    );
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_scope_pending_owned_return_is_transferred_or_unwound_once() {
    for fail in [false, true] {
        let source = format!(
            r#"package main
type Owner struct {{ Values Array<int> }}
func (v mut Owner) drop() {{ println("parent drop") }}
type Result struct {{ Values Array<int> }}
func (v mut Result) drop() {{ println("return drop"); println(v.Values[0]) }}
async func read(v Owner, ready channel<bool>, gate channel<bool>) {{
    ready.send(true)
    let _, _ = gate.receive()
    println(v.Values[0])
    if {fail} {{ panic("child failure") }}
}}
func evaluate() Result {{ println("evaluate"); return Result{{Values: Array<int>{{9}}}} }}
async func parent(ready channel<bool>, gate channel<bool>) Result {{
    scope {{
        let owner = Owner{{Values: Array<int>{{7}}}}
        go read(owner, ready, gate)
        return evaluate()
    }}
}}
func main() {{
    let ready = channel<bool>()
    let gate = channel<bool>()
    let child = go parent(ready, gate)
    let _, _ = ready.receive()
    waitPending(2)
    gate.close()
    let result = child.wait()
    println(result.Values[0])
    drop(result)
    waitIdle(0)
}}
"#
        );
        let output = run_controlled_one_worker(&source);
        assert_eq!(
            output.status.code(),
            Some(if fail { 2 } else { 0 }),
            "{}",
            stderr(&output)
        );
        let printed = stdout(&output);
        assert_eq!(printed.matches("evaluate\n").count(), 1);
        assert_eq!(printed.matches("parent drop\n").count(), 1);
        assert_eq!(printed.matches("return drop\n").count(), 1);
        assert!(printed.starts_with("evaluate\n7\n"));
        if fail {
            assert!(stderr(&output).contains("panic in the main task: child failure at"));
        } else {
            assert_eq!(printed, "evaluate\n7\nparent drop\n9\nreturn drop\n9\n");
            assert!(output.stderr.is_empty(), "{}", stderr(&output));
        }
    }
}

#[test]
fn repeated_async_scope_spawns_keep_distinct_captures_and_registered_children() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("parent drop") }
async func send(v Owner, n int, gate channel<bool>, results channel<int>) {
    let _, _ = gate.receive()
    results.send(v.Values[0] + n)
}
async func parent(ready channel<bool>, gate channel<bool>, results channel<int>) {
    scope {
        let owner = Owner{Values: Array<int>{7}}
        for var i = 0; i < 64; i += 1 { go send(owner, i, gate, results) }
        ready.send(true)
    }
}
func main() {
    let ready = channel<bool>()
    let gate = channel<bool>()
    let results = channel<int>(64)
    let child = go parent(ready, gate, results)
    let _, _ = ready.receive()
    waitPending(65)
    gate.close()
    child.wait()
    var total = 0
    for var i = 0; i < 64; i += 1 { let n, _ = results.receive(); total += n }
    println(total)
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "parent drop\n2464\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_scope_result_views_remain_valid_across_later_awaits() {
    let output = run_controlled_one_worker(
        r#"package main
async func view(values Array<int>, gate channel<bool>) []int { let _, _ = gate.receive(); return values[:] }
async func read(values []int) int { for var i = 0; i < 500; i += 1 {}; return values[0] }
async func parent(ready channel<bool>, gate channel<bool>) int {
    var values = Array<int>{7}
    scope {
        let child = go view(values, gate)
        ready.send(true)
        let result = await child
        println(await read(result))
    }
    values[0] = 9
    return values[0]
}
func main() {
    let ready = channel<bool>()
    let gate = channel<bool>()
    let child = go parent(ready, gate)
    let _, _ = ready.receive()
    waitPending(2)
    gate.close()
    println(child.wait())
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "7\n9\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_scope_unwind_skips_unentered_and_already_joined_registries() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println(v.Values[0]) }
async func read(v Owner) { println(v.Values[0] + 1) }
async func parent(fail bool) {
    scope { let owner = Owner{Values: Array<int>{7}}; go read(owner) }
    if fail { panic("between scopes") }
    scope { let owner = Owner{Values: Array<int>{99}}; go read(owner) }
}
func main() { let child = go parent(true); waitIdle(0); child.wait() }
"#,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "8\n7\n");
    assert!(stderr(&output).contains("panic in the main task: between scopes at"));
}

#[test]
fn async_scope_internal_cycles_fail_without_dropping_borrowed_parent_storage() {
    let output = run_poll_bounded(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("must not drop") }
async func blocked(v Owner, gate channel<bool>) { let _, _ = gate.receive(); println(v.Values[0]) }
async func parent() { scope { let owner = Owner{Values: Array<int>{7}}; go blocked(owner, channel<bool>()) } }
func main() { let child = go parent(); child.wait() }
"#,
        None,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("all tasks are asleep"));
}

#[test]
fn async_scope_fallthrough_discards_results_after_all_children_before_parent_drop() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("parent drop") }
type Result struct { Values []int; Label string }
func (v mut Result) drop() { println(v.Label); println(v.Values[0]) }
async func make(values Array<int>, gate channel<bool>, label string) (Result, error) {
    let _, _ = gate.receive()
    return Result{Values: values[:], Label: label}, error("discarded")
}
async func parent(ready channel<bool>, a channel<bool>, b channel<bool>) {
    scope {
        let owner = Owner{Values: Array<int>{7}}
        go make(owner.Values, a, "first result")
        go make(owner.Values, b, "second result")
        ready.send(true)
    }
    println("after")
}
func main() {
    let ready = channel<bool>()
    let a = channel<bool>()
    let b = channel<bool>()
    let child = go parent(ready, a, b)
    let _, _ = ready.receive()
    waitPending(3)
    b.close()
    waitIdle(2)
    println("second complete")
    a.close()
    child.wait()
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "second complete\nsecond result\n7\nfirst result\n7\nparent drop\nafter\n"
    );
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_scope_loop_transfers_resume_cleanup_and_budget_states() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Label int }
func (v mut Owner) drop() { println(v.Label) }
async func read(v Owner, gate channel<bool>) { let _, _ = gate.receive(); println(v.Label + 10) }
async func parent(ready channel<bool>, gate channel<bool>) int {
    for var i = 0; i < 3; i += 1 {
        scope {
            let owner = Owner{Label: i}
            go read(owner, gate)
            if i == 0 { ready.send(true); continue }
            if i == 1 { break }
        }
    }
    var total = 0
    scope {
        let owner = Owner{Label: 2}
        go read(owner, gate)
        for var i = 0; i < 500; i += 1 { total += 1; if i == 499 { break } }
        for { break }
    }
    return total
}
func main() {
    let ready = channel<bool>()
    let gate = channel<bool>()
    let child = go parent(ready, gate)
    let _, _ = ready.receive()
    waitPending(2)
    gate.close()
    println(child.wait())
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "10\n0\n11\n1\n12\n2\n500\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_literal_scopes_join_nested_borrowed_children_and_empty_boundaries() {
    let output = run_controlled_one_worker(
        r#"package main
async func parent() int {
    scope {}
    let values = Array<int>{7}
    scope {
        let child = go async func() int {
            scope {
                let nested = go func() int { return values[0] }()
                return await nested
            }
        }()
        return await child
    }
}
func main() { let child = go parent(); println(child.wait()); waitIdle(0) }
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "7\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_scope_nil_retrieval_joins_the_discarded_original_child() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("parent drop") }
async func read(v Owner, gate channel<bool>) int { let _, _ = gate.receive(); println(v.Values[0]); return 9 }
async func parent(ready channel<bool>, gate channel<bool>) int {
    scope {
        let owner = Owner{Values: Array<int>{7}}
        var child = go read(owner, gate)
        child = nil
        ready.send(true)
        return await child
    }
}
func main() {
    let ready = channel<bool>()
    let gate = channel<bool>()
    let child = go parent(ready, gate)
    let _, _ = ready.receive()
    waitPending(2)
    gate.close()
    waitIdle(0)
    println(child.wait())
}
"#,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "7\nparent drop\n");
    assert!(stderr(&output).contains("wait on a nil scoped task"));
}

#[test]
fn async_scope_destructor_panic_unwinds_only_after_all_children_finish() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("parent drop") }
type Result struct { Values []int }
func (v mut Result) drop() { println(v.Values[0]); panic("result drop") }
async func make(values Array<int>, gate channel<bool>) Result { let _, _ = gate.receive(); return Result{Values: values[:]} }
async func other(gate channel<bool>) { let _, _ = gate.receive(); println("other finished") }
async func parent(ready channel<bool>, a channel<bool>, b channel<bool>) {
    scope {
        let owner = Owner{Values: Array<int>{7}}
        go make(owner.Values, a)
        go other(b)
        ready.send(true)
    }
}
func main() {
    let ready = channel<bool>()
    let a = channel<bool>()
    let b = channel<bool>()
    let child = go parent(ready, a, b)
    let _, _ = ready.receive()
    waitPending(3)
    a.close()
    waitIdle(2)
    b.close()
    waitIdle(0)
    child.wait()
}
"#,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "other finished\n7\nparent drop\n");
    assert!(stderr(&output).contains("panic in the main task: result drop at"));
}

#[test]
fn async_scope_return_suspends_and_keeps_parent_storage_until_children_finish() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("parent drop") }
async func read(v Owner, gate channel<bool>, ready channel<bool>) {
    ready.send(true)
    let _, _ = gate.receive()
    println(v.Values[0])
}
func evaluate() int { println("evaluate"); return 9 }
async func parent(ready channel<bool>, gate channel<bool>) int {
    scope {
        let v = Owner{Values: Array<int>{7}}
        let ignored = go read(v, gate, ready)
        drop(ignored)
        return evaluate()
    }
}


func main() {
    let ready = channel<bool>()
    let gate = channel<bool>()
    let child = go parent(ready, gate)
    let _, _ = ready.receive()
    waitPending(2)
    gate.close()
    println(child.wait())
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "evaluate\n7\nparent drop\n9\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_scope_await_retrieves_borrowed_children_once() {
    let output = run_controlled_one_worker(
        r#"package main
async func increment(value mut int, gate channel<bool>) int { let _, _ = gate.receive(); value += 1; return value }
async func parent(ready channel<bool>, gate channel<bool>) int {
    var value = 0
    scope {
        let first = go increment(value, gate)
        ready.send(true)
        _ = await first
        println(value)
        let second = go increment(value, gate)
        _ = await second
        println(value)
    }
    return value
}
func main() {
    let ready = channel<bool>()
    let gate = channel<bool>()
    let child = go parent(ready, gate)
    let _, _ = ready.receive()
    waitPending(2)
    gate.close()
    println(child.wait())
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "1\n2\n2\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_scope_error_propagation_suspends_after_evaluating_the_error_once() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("parent drop") }
async func read(v Owner, ready channel<bool>, gate channel<bool>) {
    ready.send(true)
    let _, _ = gate.receive()
    println(v.Values[0])
}
func fail() error { println("error evaluated"); return error("stop") }
async func parent(ready channel<bool>, gate channel<bool>) (int, error) {
    scope {
        let v = Owner{Values: Array<int>{7}}
        go read(v, ready, gate)
        fail()?
    }
    return 9, nil
}
func main() {
    let ready = channel<bool>()
    let gate = channel<bool>()
    let child = go parent(ready, gate)
    let _, _ = ready.receive()
    waitPending(2)
    gate.close()
    let value, err = child.wait()
    println(value)
    println(err == error("stop"))
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "error evaluated\n7\nparent drop\n0\ntrue\n"
    );
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn async_scope_panics_join_every_child_and_preserve_failure_priority() {
    for (exit, expected) in [
        ("panic(\"parent\")", "parent"),
        ("return 9, nil", "first"),
        ("fail()?", "first"),
        ("_ = await second", "second"),
    ] {
        let source = format!(
            r#"package main
type Owner struct {{ Values Array<int> }}
func (v mut Owner) drop() {{ println("parent drop") }}
async func failing(v Owner, gate channel<bool>, label string) int {{
    let _, _ = gate.receive()
    println(label)
    panic(label)
}}
async func healthy() int {{ return 5 }}
func fail() error {{ return error("pending error") }}
async func parent(ready channel<bool>, a channel<bool>, b channel<bool>) (int, error) {{
    scope {{
        let v = Owner{{Values: Array<int>{{7}}}}
        go failing(v, a, "first")
        let second = go failing(v, b, "second")
        ready.send(true)
        {exit}
    }}
    return 0, nil
}}
func main() {{
    let ready = channel<bool>()
    let a = channel<bool>()
    let b = channel<bool>()
    let child = go parent(ready, a, b)
    let _, _ = ready.receive()
    waitPending(3)
    b.close()
    waitIdle(2)
    waitPending(2)
    let probe = go healthy()
    println(probe.wait())
    a.close()
    waitIdle(0)
    let _, err = child.wait()
    _ = err
}}
"#
        );
        let output = run_controlled_one_worker(&source);
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert_eq!(stdout(&output), "second\n5\nfirst\nparent drop\n");
        assert!(
            stderr(&output).contains(&format!("panic in the main task: {expected} at")),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn async_scope_unwind_carries_the_parent_panic_to_a_replacement_worker() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Values Array<int> }
func (v mut Owner) drop() { println("parent drop") }
async func read(v Owner, gate channel<bool>) { let _, _ = gate.receive(); println(v.Values[0]) }
func blocked(gate channel<bool>) { let _, _ = gate.receive() }
async func healthy() int { return 5 }
async func parent(ready channel<bool>, gate channel<bool>) {
    scope {
        let v = Owner{Values: Array<int>{7}}
        go read(v, gate)
        ready.send(true)
        panic("migrating parent")
    }
}
func main() {
    let ready = channel<bool>()
    let gate = channel<bool>()
    let stalled = channel<bool>()
    let child = go parent(ready, gate)
    let _, _ = ready.receive()
    waitPending(2)
    let busy = go blocked(stalled)
    waitBlocked(1)
    gate.close()
    waitIdle(1)
    let probe = go healthy()
    println(probe.wait())
    stalled.close()
    busy.wait()
    waitIdle(0)
    child.wait()
}
"#,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "7\nparent drop\n5\n");
    assert!(stderr(&output).contains("panic in the main task: migrating parent at"));
}

#[test]
fn nested_async_scope_unwind_finishes_inner_cleanup_before_outer_children() {
    let output = run_controlled_one_worker(
        r#"package main
type Owner struct { Label string }
func (v mut Owner) drop() { println(v.Label + " drop") }
async func read(v Owner, gate channel<bool>) { let _, _ = gate.receive(); println(v.Label + " child") }
async func inner(ready channel<bool>, gate channel<bool>) {
    scope {
        let v = Owner{Label: "inner"}
        go read(v, gate)
        ready.send(true)
        panic("inner panic")
    }
}
async func outer(ready channel<bool>, a channel<bool>, b channel<bool>) {
    scope {
        let v = Owner{Label: "outer"}
        go read(v, b)
        await inner(ready, a)
    }
}
func main() {
    let ready = channel<bool>()
    let a = channel<bool>()
    let b = channel<bool>()
    let child = go outer(ready, a, b)
    let _, _ = ready.receive()
    waitPending(3)
    a.close()
    waitIdle(2)
    waitPending(2)
    b.close()
    waitIdle(0)
    child.wait()
}
"#,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "inner child\ninner drop\nouter child\nouter drop\n"
    );
    assert!(stderr(&output).contains("panic in the main task: inner panic at"));
}
