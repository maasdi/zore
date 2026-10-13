use super::{prints, run, stderr, stdout};

#[test]
fn mixed_bindings_in_generic_functions_keep_specialized_local_types() {
    prints(
        r#"package main
func pair<T any>(first own T, second own T) (T, T) { return first, second }
func choose<T any>(first own T, second own T) T {
    (let kept, var result) = pair(first, second)
    result = kept
    return result
}
func main() {
    println(choose(7, 9))
    println(choose("first", "second"))
    let original = Array<int>{4, 5}
    let chosen = choose(clone(original), Array<int>{6})
    println(original.len())
    println(chosen[0])
    println(chosen.len())
}"#,
        "7\nfirst\n2\n4\n2\n",
    );
}

#[test]
fn mixed_bindings_evaluate_once_and_preserve_scopes_and_mutability() {
    prints(
        r#"package main
func load(value int) (int, error) { println(value); return value, nil }
func split(value int) (int, int) { return value + 1, value + 2 }
func bounds() (int, int) { return 3, 0 }
func main() {
    (let first, var err) = load(1)
    if err != nil { panic("first") }
    (let second, err) = load(first + 1)
    if err != nil { panic("second") }
    println(first)
    println(second)
    var outer = 10
    {
        (let outer, var other) = split(outer)
        other += 1
        println(outer)
        println(other)
    }
    println(outer)
    (outer, let next) = split(outer)
    println(outer)
    println(next)
    for (let limit, var index) = bounds(); index < limit; index += 1 {
        println(index)
    }
}"#,
        "1\n2\n1\n2\n11\n13\n10\n11\n12\n0\n1\n2\n",
    );
}

#[test]
fn mixed_bindings_replace_reinitialize_and_discard_owned_values_once() {
    prints(
        r#"package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id + 100) }
func pair() (Guard, Guard) { println(90); return Guard{id: 2}, Guard{id: 3} }
func main() {
    {
        var previous = Guard{id: 1}
        (let current, previous) = pair()
        println(current.id)
        println(previous.id)
        let taken = previous
        (let next, previous) = pair()
        println(next.id)
        println(previous.id)
        drop(taken)
    }
    {
        (let kept, _) = pair()
        println(kept.id)
    }
}"#,
        "90\n101\n2\n3\n90\n2\n3\n103\n102\n102\n103\n90\n103\n2\n102\n",
    );
}

#[test]
fn mixed_bindings_propagation_skips_stores_and_cleans_all_results() {
    prints(
        r#"package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id + 100) }
func pair(fail bool) (Guard, Guard, error) {
    if fail { return Guard{id: 2}, Guard{id: 3}, error("failed") }
    return Guard{id: 2}, Guard{id: 3}, nil
}
func work(fail bool) error {
    var previous = Guard{id: 1}
    (let current, previous) = pair(fail)?
    println(current.id + 10)
    return nil
}
func main() {
    let failure = work(true)
    println(failure != nil)
    let success = work(false)
    println(success == nil)
}"#,
        "103\n102\n101\ntrue\n101\n12\n102\n103\ntrue\n",
    );
}

#[test]
fn mixed_bindings_panic_keeps_uninitialized_targets_out_of_cleanup() {
    let output = run(r#"package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id + 100) }
func explode() (Guard, Guard) {
    let inner = Guard{id: 2}
    panic("initializer failed")
}
func main() {
    var previous = Guard{id: 1}
    (let current, previous) = explode()
}"#);
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "102\n101\n");
    assert!(stderr(&output).contains("initializer failed"));
}

#[test]
fn mixed_bindings_awaited_results_keep_owned_locals_and_error_checks() {
    prints(
        r#"package main
type Guard struct { id int }
func (g mut Guard) drop() { println(g.id + 100) }
async func produce(release channel<bool>) (Guard, error) {
    let _, _ = release.receive()
    return Guard{id: 2}, nil
}
async func work(release channel<bool>) error {
    let outer = Guard{id: 1}
    var err = error("handled")
    _ = err
    (let result, err) = await produce(release)
    if err != nil { return err }
    println(outer.id + result.id)
    return nil
}
func main() {
    let release = channel<bool>()
    let task = go work(release)
    release.send(true)
    let err = task.wait()
    println(err == nil)
}"#,
        "3\n102\n101\ntrue\n",
    );
}
