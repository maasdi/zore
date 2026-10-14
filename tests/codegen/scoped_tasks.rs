use super::*;

#[test]
fn scope_shared_mutable_and_literal_children_join_before_parent_drop() {
    prints(
        r#"package main
import "std/time"
type Owner struct { Values Array<int> }
func (o mut Owner) drop() { println("drop") }
func read(o Owner) int { return o.Values[0] }
func inc(v mut int) { v += 1 }
func main() {
    var n = 0
    scope {
        let owner = Owner{Values: Array<int>{7}}
        let a = go read(owner)
        let b = go read(owner)
        println(a.wait() + b.wait())
        let child = go inc(n)
        child.wait()
        let captured = go func() { n += 1 }()
        captured.wait()
        go func() { time.Sleep(time.Millisecond); println("child") }()
    }
    println(n)
}
"#,
        "14\nchild\ndrop\n2\n",
    );
}

#[test]
fn scope_ignored_handles_and_repeated_capture_sites_keep_their_storage() {
    prints(
        r#"package main
func main() {
    let gate = channel<bool>()
    let results = channel<int>(4)
    scope {
        for var i = 0; i < 4; i += 1 {
            go func(g channel<bool>, r channel<int>, n int) { let _, _ = g.receive(); r.send(n) }(gate, results, i)
        }
        gate.close()
    }
    var total = 0
    for var i = 0; i < 4; i += 1 { let n, ok = results.receive(); if !ok { panic("missing result") }; total += n }
    println(total)
}
"#,
        "6\n",
    );
}

#[test]
fn scope_return_and_error_propagation_evaluate_once_and_join_before_drop() {
    prints(
        r#"package main
type Guard struct { Label string }
func (g mut Guard) drop() { println(g.Label) }
func child(g Guard, gate channel<bool>) { let _, _ = gate.receive(); println("child") }
func evaluate(gate channel<bool>) int { println("evaluate"); gate.close(); return 9 }
func fail(gate channel<bool>) error { gate.close(); return error("stop") }
func returning() int {
    scope {
        let g = Guard{Label: "return drop"}
        let gate = channel<bool>()
        go child(g, gate)
        return evaluate(gate)
    }
}
func propagating() error {
    scope {
        let g = Guard{Label: "error drop"}
        let gate = channel<bool>()
        go child(g, gate)
        fail(gate)?
    }
    return nil
}
func main() {
    println(returning())
    let err = propagating()
    if err != error("stop") { panic("lost error") }
    println("error")
}
"#,
        "evaluate\nchild\nreturn drop\n9\nchild\nerror drop\nerror\n",
    );
}

#[test]
fn scope_loop_exits_join_only_when_crossing_the_boundary() {
    let output = run_poll_bounded(
        r#"package main
func blocked(g channel<bool>, r channel<int>, n int) { let _, _ = g.receive(); r.send(n) }
func main() {
    let results = channel<int>(4)
    for var i = 0; i < 4; i += 1 {
        scope {
            let gate = channel<bool>()
            go blocked(gate, results, i)
            for var j = 0; j < 3; j += 1 {
                if j == 0 { continue }
                break
            }
            gate.close()
            if i == 0 { continue }
            if i == 2 { break }
        }
    }
    var total = 0
    for var i = 0; i < 3; i += 1 { let n, _ = results.receive(); total += n }
    println(total)
}
"#,
        None,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "3\n");
}

#[test]
fn scope_parent_panic_joins_borrowing_children_before_parent_drop() {
    let output = run_poll_bounded(
        r#"package main
import "std/time"
type Guard struct { Label string }
func (g mut Guard) drop() { println(g.Label) }
func child(g Guard) { time.Sleep(time.Millisecond); println("child") }
func main() {
    scope {
        let g = Guard{Label: "parent drop"}
        go child(g)
        panic("parent")
    }
}
"#,
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "child\nparent drop\n");
    assert!(stderr(&output).contains("panic in the main task: parent"));
}

#[test]
fn scope_child_panic_priority_follows_spawn_order_after_all_children_finish() {
    let output = run_poll_bounded(
        r#"package main
type Guard struct { Label string }
func (g mut Guard) drop() { println(g.Label) }
func first(g Guard, gate channel<bool>) { let _, _ = gate.receive(); panic("first") }
func second(g Guard, gate channel<bool>) { gate.close(); panic("second") }
func main() {
    scope {
        let g = Guard{Label: "parent drop"}
        let gate = channel<bool>()
        go first(g, gate)
        go second(g, gate)
    }
}
"#,
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "parent drop\n");
    let errors = stderr(&output);
    assert!(errors.contains("panic in the main task: first"), "{errors}");
    assert_eq!(errors.matches("panic in task ").count(), 2);
}

#[test]
fn scope_explicit_wait_panic_keeps_priority_and_still_joins_other_children() {
    let output = run_poll_bounded(
        r#"package main
import "std/time"
type Guard struct { Label string }
func (g mut Guard) drop() { println(g.Label) }
func later(g Guard) { time.Sleep(time.Millisecond); println("other child") }
func fail() { panic("observed") }
func main() {
    scope {
        let g = Guard{Label: "parent drop"}
        go later(g)
        let child = go fail()
        child.wait()
    }
}
"#,
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "other child\nparent drop\n");
    assert!(stderr(&output).contains("panic in the main task: observed"));
}

#[test]
fn scope_async_children_and_plain_helpers_progress_with_one_pool_worker() {
    let output = run_controlled_one_worker(
        r#"package main
async func read(values Array<int>) int { return values[0] }
func helper(values Array<int>) int {
    scope { let child = go read(values); return child.wait() }
}
async func parent(values own Array<int>) int { return helper(values) }
func main() {
    let child = go parent(Array<int>{7})
    println(child.wait())
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "7\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn scope_internal_deadlock_terminates_without_dropping_borrowed_parent_storage() {
    let output = run_poll_bounded(
        r#"package main
type Guard struct { Label string }
func (g mut Guard) drop() { println(g.Label) }
func blocked(g Guard, gate channel<bool>) { let _, _ = gate.receive(); println(g.Label) }
func main() {
    scope {
        let g = Guard{Label: "must not drop"}
        go blocked(g, channel<bool>())
    }
}
"#,
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("all tasks are asleep"));
}

#[test]
fn scope_result_destructors_finish_before_their_borrowed_parent_is_dropped() {
    let output = run_controlled(
        r#"package main
type Parent struct { Values Array<int> }
func (p mut Parent) drop() { println("parent") }
type Result struct { Values []int }
func (r mut Result) drop() { println(r.Values[0]) }
func result(p Parent) Result { return Result{Values: p.Values[:]} }
func ignored() error { return error("ignored") }
func main() {
    scope {
        let p = Parent{Values: Array<int>{7}}
        go result(p)
        go ignored()
    }
    waitIdle(0)
}
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "7\nparent\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn scope_nested_registries_close_before_enclosing_scope_storage() {
    prints(
        r#"package main
type Parent struct { Values Array<int>; Label string }
func (p mut Parent) drop() { println(p.Label) }
func read(p Parent) { println(p.Values[0]) }
func main() {
    scope {
        let outer = Parent{Values: Array<int>{1}, Label: "outer"}
        scope {
            let inner = Parent{Values: Array<int>{2}, Label: "inner"}
            go read(inner)
        }
        go read(outer)
    }
}
"#,
        "2\ninner\n1\nouter\n",
    );
}

#[test]
fn scope_mixed_owned_borrowed_captures_and_mutable_outputs_clean_up_once() {
    prints(
        r#"package main
type Owned struct { Label string }
func (o mut Owned) drop() { println(o.Label) }
type Out struct { Values []int }
func fill(out mut Out, values Array<int>) { out.Values = values[:] }
func main() {
    let values = Array<int>{7}
    let childOwned = Owned{Label: "child owned"}
    var out = Out{Values: values[:]}
    scope {
        go func() { println(values[0]); drop(childOwned) }()
        go fill(out, values)
    }
    println(out.Values[0])
}
"#,
        "7\nchild owned\n7\n",
    );
}

#[test]
fn scope_result_destructor_panic_occurs_after_other_children_join() {
    let output = run_poll_bounded(
        r#"package main
import "std/time"
type Parent struct { Values Array<int> }
func (p mut Parent) drop() { println("parent") }
type Result struct { Values []int }
func (r mut Result) drop() { println(r.Values[0]); panic("result drop") }
func result(p Parent) Result { return Result{Values: p.Values[:]} }
func other(p Parent) { time.Sleep(time.Millisecond); println("other") }
func main() {
    scope {
        let p = Parent{Values: Array<int>{7}}
        go result(p)
        go other(p)
    }
}
"#,
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "other\n7\nparent\n");
    assert!(stderr(&output).contains("panic in the main task: result drop"));
}

#[test]
fn scope_unobserved_child_panic_supersedes_a_pending_error_propagation() {
    let output = run_poll_bounded(
        r#"package main
type Guard struct { Label string }
func (g mut Guard) drop() { println(g.Label) }
func child(g Guard) { panic("scope failure") }
func fail() error { return error("pending error") }
func exercise() error {
    scope {
        let g = Guard{Label: "parent drop"}
        go child(g)
        fail()?
    }
    return nil
}
func main() { let err = exercise(); if err != nil { println("must not return error") } }
"#,
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "parent drop\n");
    assert!(stderr(&output).contains("panic in the main task: scope failure"));
}
