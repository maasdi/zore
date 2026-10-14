use super::*;

#[test]
fn async_scope_parent_backed_views_and_mutable_awaits_follow_existing_loans() {
    accepts(
        "package main
async func view(values Array<int>) []int { return values[:] }
async func update(value mut int) int { value += 1; return value }
async func parent(values Array<int>) int {
    var count = 0
    scope {
        let child = go view(values)
        let result = await child
        println(result[0])
        let changed = go update(count)
        _ = await changed
        println(count)
    }
    return count
}
func main() {}",
    );
    accepts(
        "package main\nasync func update(value mut int) { value += 1 }\nasync func parent() { var value = 0; scope { let child = go update(value); await child; println(value) } }\nfunc main() {}",
    );
}

#[test]
fn async_scope_borrows_remain_registered_across_handle_drop_and_body_await() {
    rejects(
        "package main
async func read(values Array<int>) int { return values[0] }
async func pause() int { return 0 }
async func parent() {
    var values = Array<int>{1}
    scope { let child = go read(values); drop(child); _ = await pause(); values[0] = 2 }
}
func main() {}",
        "borrowed",
    );
    rejects(
        "package main
async func update(value mut int) int { value += 1; return value }
async func pause() int { return 0 }
async func parent() {
    var count = 0
    scope { go update(count); _ = await pause(); println(count) }
}
func main() {}",
        "borrowed",
    );
}

#[test]
fn async_scope_retrieval_cannot_extend_handles_or_view_lifetimes() {
    rejects(
        "package main
async func view(values Array<int>) []int { return values[:] }
async func parent(values Array<int>) []int { scope { let child = go view(values); return await child } }
func main() {}",
        "escape",
    );
    rejects(
        "package main
async func read(values Array<int>) int { return values[0] }
async func parent() { scope { for var i = 0; i < 2; i += 1 { let values = Array<int>{1}; let child = go read(values); _ = await child } } }
func main() {}",
        "shorter",
    );
    rejects(
        "package main
async func read(values Array<int>) int { return values[0] }
async func parent() { let values = Array<int>{1}; scope { var child = go read(values); if true { child = go read(values) }; _ = await child; drop(values) } }
func main() {}",
        "borrowed",
    );
    rejects(
        "package main
async func read() int { return 1 }
async func parent() { scope { let child = go read(); _ = await child; _ = await child } }
func main() {}",
        "moved",
    );
}

#[test]
fn async_scope_keeps_wait_syntax_and_helper_body_boundaries() {
    rejects(
        "package main\nasync func parent() { await 1 }\nfunc main() {}",
        "`await` needs",
    );
    rejects(
        "package main\nasync func fail() error { return error(\"failed\") }\nasync func parent() { scope { let child = go fail(); await child } }\nfunc main() {}",
        "error result must be used",
    );
    rejects(
        "package main
async func read() int { return 1 }
async func parent() { scope { let child = go read(); println(child.wait()) } }
func main() {}",
        "`wait` blocks",
    );
    rejects(
        "package main
async func read(values Array<int>) int { return values[0] }
async func helper(values Array<int>) { go read(values) }
async func parent(values Array<int>) { scope { await helper(values) } }
func main() {}",
        "own",
    );
}

#[test]
fn scope_shared_exclusive_and_literal_borrows_are_accepted() {
    accepts(
        "package main\nfunc read(v Array<int>) int { return v[0] }\nfunc inc(v mut int) { v += 1 }\nfunc main() { let v = Array<int>{1}; var n = 0; scope { let a = go read(v); let b = go read(v); println(a.wait() + b.wait()); let c = go inc(n); c.wait(); println(n); let d = go func() { n += 1 }(); d.wait(); println(n) } }\n",
    );
}

#[test]
fn scope_dropped_handles_retain_exclusive_loans() {
    for ending in ["println(n)", "n = 3", "let b = go inc(n)"] {
        rejects(
            &format!(
                "package main\nfunc inc(v mut int) {{ v += 1 }}\nfunc main() {{ var n = 0; scope {{ let a = go inc(n); drop(a); {ending} }} }}\n"
            ),
            "borrow",
        );
    }
}

#[test]
fn scope_ignored_shared_children_prevent_move_and_replacement() {
    for ending in ["drop(v)", "v = Array<int>{2}"] {
        rejects(
            &format!(
                "package main\nfunc read(v Array<int>) {{ println(v[0]) }}\nfunc main() {{ var v = Array<int>{{1}}; scope {{ go read(v); {ending} }} }}\n"
            ),
            "borrow",
        );
    }
}

#[test]
fn scope_short_inner_referents_are_rejected_even_after_a_wait() {
    for opening in ["if true", "for true"] {
        rejects(
            &format!(
                "package main\nfunc read(v Array<int>) {{ println(v[0]) }}\nfunc main() {{ scope {{ {opening} {{ let v = Array<int>{{1}}; let a = go read(v); a.wait() }} }} }}\n"
            ),
            "shorter-lived",
        );
    }
}

#[test]
fn scope_async_bodies_are_accepted_without_dynamic_scope_inheritance() {
    accepts("package main\nasync func work() { scope {} }\nfunc main() {}\n");
    rejects(
        "package main\nfunc helper(v Array<int>) { go read(v) }\nfunc read(v Array<int>) {}\nfunc main() { let v = Array<int>{1}; scope { helper(v) } }\n",
        "own",
    );
}

#[test]
fn scope_handles_cannot_cross_calls_captures_or_scope_identity() {
    rejects(
        "package main\nfunc job() {}\nfunc use<T any>(v own T) {}\nfunc main() { scope { let a = go job(); use(a) } }\n",
        "cannot be passed",
    );
    rejects(
        "package main\nfunc job() {}\nfunc main() { scope { let a = go job(); let f = func() { a.wait() }; f() } }\n",
        "cannot be captured",
    );
    rejects(
        "package main\nfunc job() {}\nfunc main() { scope { let a = go job(); scope { let b = a; b.wait() } } }\n",
        "another task scope",
    );
}

#[test]
fn scope_parent_backed_results_work_and_cannot_escape() {
    accepts(
        "package main\nfunc view(v Array<int>) []int { return v[:] }\nfunc main() { let v = Array<int>{1}; scope { let a = go view(v); let s = a.wait(); println(s[0]) } }\n",
    );
    rejects(
        "package main\nfunc view(v Array<int>) []int { return v[:] }\nfunc bad(v Array<int>) []int { scope { let a = go view(v); return a.wait() } }\nfunc main() {}\n",
        "escape",
    );
    rejects(
        "package main\nfunc view(v [int; 1]) []int { return v[:] }\nfunc main() { let v = [int; 1]{1}; scope { let a = go view(v); let s = a.wait(); println(s[0]) } }\n",
        "child-owned",
    );
}

#[test]
fn scope_direct_locals_nested_scopes_and_own_inputs_are_accepted() {
    accepts(
        "package main\nfunc read(v Array<int>) {}\nfunc take(v own Array<int>) {}\nfunc main() { scope { let v = Array<int>{1}; go read(v); scope { go read(v) }; go take(Array<int>{2}) } }\n",
    );
}

#[test]
fn scope_repeated_mutable_spawn_and_wait_releases_each_iteration() {
    accepts(
        "package main\nfunc inc(v mut int) { v += 1 }\nfunc main() { var n = 0; scope { for var i = 0; i < 3; i += 1 { let a = go inc(n); a.wait(); println(n) } } }\n",
    );
}

#[test]
fn scope_annotations_remain_ordinary_tasks_and_callable_results_require_parent_backing() {
    rejects(
        "package main\nfunc job() int { return 1 }\nfunc main() { scope { let child Task<int> = go job(); println(child.wait()) } }\n",
        "expected",
    );
    accepts(
        "package main\nfunc main() { let values = Array<int>{1}; scope { let child = go func() []int { return values[:] }(); let view = child.wait(); println(view[0]) } }\n",
    );
    rejects(
        "package main\ntype Pair struct { Keep Array<int>; Take Array<int> }\nfunc consume(v own Array<int>) {}\nfunc main() { let p = Pair{Keep: Array<int>{1}, Take: Array<int>{2}}; scope { let child = go func() []int { consume(p.Take); return p.Keep[:] }(); let view = child.wait(); println(view[0]) } }\n",
        "child-owned",
    );
}

#[test]
fn scope_views_cannot_escape_through_an_outer_binding_or_wrapper_call() {
    for value in ["child.wait()", "identity(child.wait())"] {
        rejects(
            &format!(
                "package main\nfunc view(v Array<int>) []int {{ return v[:] }}\nfunc identity(v []int) []int {{ return v }}\nfunc main() {{ let v = Array<int>{{1}}; var out = v[:]; scope {{ let child = go view(v); out = {value} }}; println(out[0]) }}\n"
            ),
            "escape",
        );
    }
}

#[test]
fn scope_wait_through_a_merged_handle_does_not_release_other_registered_children() {
    rejects(
        "package main\nfunc read(v Array<int>) {}\nfunc main() { let v = Array<int>{1}; scope { var last = go read(v); if true { last = go read(v) }; last.wait(); drop(v) } }\n",
        "borrow",
    );
    rejects(
        "package main\nfunc none() {}\nfunc read(v Array<int>) {}\nfunc main() { let v = Array<int>{1}; scope { var last = go none(); for var i = 0; i < 2; i += 1 { last = go read(v) }; last.wait(); drop(v) } }\n",
        "borrow",
    );
}

#[test]
fn scope_function_arguments_borrow_exclusively_and_mutable_views_keep_backing_loans() {
    accepts(
        "package main\nfunc update(v mut []int) { v[0] += 1 }\nfunc main() { var v = Array<int>{1}; scope { let t = go update(v[:]); t.wait(); println(v[0]) } }\n",
    );
    rejects(
        "package main\nfunc call(f func()) { f() }\nfunc main() { var n = 0; let f = func() { n += 1 }; scope { go call(f); go call(f) } }\n",
        "borrow",
    );
}

#[test]
fn scope_borrowed_outputs_keep_parent_provenance_after_child_completion() {
    accepts(
        "package main\ntype Out struct { Values []int }\nfunc fill(out mut Out, values Array<int>) { out.Values = values[:] }\nfunc main() { let values = Array<int>{7}; var out = Out{Values: values[:]}; scope { go fill(out, values) }; println(out.Values[0]) }\n",
    );
    accepts(
        "package main\ntype Out struct { Values []int }\nfunc main() { let values = Array<int>{7}; var out = Out{Values: values[:]}; scope { go func() { out.Values = values[:] }() }; println(out.Values[0]) }\n",
    );
}

#[test]
fn scope_async_closures_are_accepted_and_child_local_results_remain_rejected() {
    accepts(
        "package main\nfunc main() { let f = async func() { scope {} }; let child = go f(); child.wait() }\n",
    );
    rejects(
        "package main\nfunc bad() []int { let v = Array<int>{1}; return v[:] }\nfunc main() { scope { let child = go bad(); let v = child.wait(); println(v[0]) } }\n",
        "return a view",
    );
}
