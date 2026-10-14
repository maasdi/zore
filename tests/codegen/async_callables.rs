use super::{prints, run, stderr, stdout};

#[test]
fn method_values_specialize_generic_capture_roots_for_plain_async_and_nested_calls() {
    prints(
        r#"package main
import "std/time"
type Counter struct { N int }
func (c Counter) readPlain() int { return c.N }
async func (c Counter) read() int { time.Sleep(time.Millisecond); return c.N }
async func (c mut Counter) bump(n int) int { time.Sleep(time.Millisecond); c.N += n; return c.N }
type Resource struct { ID int }
func (r mut Resource) drop() { println(r.ID) }
type Holder<T any> struct { Counter Counter; Value T }
func reader<T any>(h own Holder<T>) async func() int { return h.Counter.read }
func plain<T any>(h own Holder<T>) func() int { return h.Counter.readPlain }
func nested<T any>(h own Holder<T>) async func() int {
    return async func() int { let read = h.Counter.read; return await read() }
}
func counter<T any>(state own Holder<T>) async func(int) int {
    var h = state
    return h.Counter.bump
}
async func work() {
    let first = reader(Holder<int>{Counter: Counter{N: 10}, Value: 1})
    let second = reader(Holder<float64>{Counter: Counter{N: 20}, Value: 1.5})
    let third = nested(Holder<Array<int>>{Counter: Counter{N: 30}, Value: Array<int>{1}})
    let fourth = plain(Holder<string>{Counter: Counter{N: 40}, Value: "value"})
    let bump = counter(Holder<int>{Counter: Counter{N: 50}, Value: 1})
    println(await first())
    println(await second())
    println(await third())
    println(fourth())
    println(await bump(2))
    println(await bump(3))
}
func main() {
    let task = go work()
    task.wait()
    let read = reader(Holder<Resource>{Counter: Counter{N: 60}, Value: Resource{ID: 7}})
    let task2 = go read()
    println(task2.wait())
}
"#,
        "10\n20\n30\n40\n52\n55\n7\n60\n",
    );
}

#[test]
fn task_capture_panics_destroy_returned_results_before_wait_or_await_rethrows() {
    for mode in ["", "async "] {
        for retrieve in [
            "func main() { let guard = Guard{ID: 9}; let task = go job(); let a, b, err = task.wait(); println(err == nil) }",
            "async func work() { let guard = Guard{ID: 9}; let task = go job(); let a, b, err = await task; println(err == nil) }\nfunc main() { let task = go work(); task.wait() }",
        ] {
            let source = format!(
                r#"package main
import "std/time"
type Guard struct {{ ID int }}
func (r mut Guard) drop() {{ println(r.ID) }}
type Capture struct {{ ID int }}
func (c mut Capture) drop() {{ println(c.ID); panic("capture drop failed") }}
type Result struct {{ ID int; Values Array<int>; Notifications channel<int> }}
func (r mut Result) drop() {{ println(r.ID) }}
func job() (Result, Result, error) {{
    let c = Capture{{ID: 1}}
    let f = {mode}func() (Result, Result, error) {{
        time.Sleep(time.Millisecond)
        return Result{{ID: 2, Values: Array<int>{{c.ID}}, Notifications: channel<int>()}}, Result{{ID: 3, Values: Array<int>{{c.ID}}, Notifications: channel<int>()}}, error("returned error")
    }}
    let task = go f()
    return task.wait()
}}
{retrieve}
"#
            );
            let output = run(&source);
            assert_eq!(output.status.code(), Some(2), "{output:?}");
            assert_eq!(stdout(&output), "1\n3\n2\n9\n", "{output:?}");
            assert!(
                stderr(&output).contains("capture drop failed"),
                "{output:?}"
            );
            assert!(!stderr(&output).contains("leak:"), "{output:?}");
        }
    }
}

#[test]
fn detached_tasks_drop_returned_resources_when_capture_destruction_panics() {
    for mode in ["", "async "] {
        for detach in ["drop(task)", ""] {
            let after = if detach.is_empty() { "drop(task)" } else { "" };
            let source = format!(
                r#"package main
import "std/time"
type Capture struct {{ ID int }}
func (c mut Capture) drop() {{ println(c.ID); panic("capture drop failed") }}
type Completion struct {{ Done channel<int> }}
func (c mut Completion) drop() {{ println(3); c.Done.send(1) }}
type Result struct {{ Completion Completion; Notifications channel<int>; Values Array<int> }}
func (r mut Result) drop() {{ println(2) }}
func main() {{
    let completed = channel<int>()
    let capture = Capture{{ID: 1}}
    let job = {mode}func() Result {{
        time.Sleep(time.Millisecond)
        return Result{{Completion: Completion{{Done: completed}}, Notifications: channel<int>(), Values: Array<int>{{capture.ID}}}}
    }}
    let task = go job()
    {detach}
    let done, ok = completed.receive()
    {after}
    println(done + 3)
}}
"#
            );
            let output = super::run_bounded(&source, None, false);
            assert_eq!(output.status.code(), Some(0), "{output:?}");
            assert_eq!(stdout(&output), "1\n2\n3\n4\n", "{output:?}");
            assert!(!stderr(&output).contains("leak:"), "{output:?}");
        }
    }
}

#[test]
fn task_body_panics_do_not_destroy_uninitialized_results() {
    for mode in ["", "async "] {
        let source = format!(
            r#"package main
import "std/time"
type Guard struct {{ ID int }}
func (r mut Guard) drop() {{ println(r.ID) }}
type Result struct {{ Values Array<int>; Notifications channel<int> }}
func (r mut Result) drop() {{ println(0) }}
func main() {{
    let guard = Guard{{ID: 9}}
    let capture = Guard{{ID: 1}}
    let job = {mode}func() Result {{
        time.Sleep(time.Millisecond)
        let local = Guard{{ID: 2}}
        println(capture.ID)
        panic("body failed")
    }}
    let task = go job()
    let result = task.wait()
}}
"#
        );
        let output = run(&source);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(stdout(&output), "1\n2\n1\n9\n", "{output:?}");
        assert!(stderr(&output).contains("body failed"), "{output:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_second_panic_while_discarding_task_results_aborts() {
    use std::os::unix::process::ExitStatusExt;
    for mode in ["", "async "] {
        let source = format!(
            r#"package main
import "std/time"
type Capture struct {{ ID int }}
func (c mut Capture) drop() {{ println(c.ID); panic("capture drop failed") }}
type Result struct {{ ID int }}
func (r mut Result) drop() {{ println(r.ID); panic("result drop failed") }}
func main() {{
    let c = Capture{{ID: 1}}
    let job = {mode}func() Result {{ time.Sleep(time.Millisecond); return Result{{ID: c.ID + 1}} }}
    let task = go job()
    let result = task.wait()
}}
"#
        );
        let output = run(&source);
        assert_eq!(output.status.signal(), Some(6), "{output:?}");
        assert_eq!(stdout(&output), "1\n2\n", "{output:?}");
    }
}

#[test]
fn async_borrowing_literals_and_nested_captures_survive_real_waits() {
    prints(
        r#"package main
import "std/time"
async func work() int {
    var values = Array<int>{1}
    let bump = async func(n int) { time.Sleep(time.Millisecond); values.push(n) }
    let nested = async func() { await bump(2); time.Sleep(time.Millisecond); await bump(3) }
    await nested()
    println(values.len())
    let read = async func() int { time.Sleep(time.Millisecond); return values[1] }
    let n = await read()
    values.push(n)
    return values.len()
}
func main() { let task = go work(); println(task.wait()) }
"#,
        "3\n4\n",
    );
}

#[test]
fn reusable_owning_async_literals_keep_environments_after_each_await() {
    prints(
        r#"package main
import "std/time"
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
type Holder struct { f async func() int }
func create(id int) async func() int {
    var r = Resource{id: id}
    return async func() int { time.Sleep(time.Millisecond); r.id += 1; return r.id }
}
async func invoke(f async func() int) int { return await f() }
async func work() {
    let f = create(10)
    println(await invoke(f))
    println(await f())
    var holder = Holder{f: create(20)}
    println(await (holder.f)())
    println(await (holder.f)())
}
func main() { let task = go work(); task.wait() }
"#,
        "11\n12\n21\n22\n22\n12\n",
    );
}

#[test]
fn async_method_values_borrow_field_receivers_and_consume_owned_receivers_once() {
    prints(
        r#"package main
import "std/time"
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
async func (r Resource) read() int { time.Sleep(time.Millisecond); return r.id }
async func (r mut Resource) bump(n int) int { time.Sleep(time.Millisecond); r.id += n; return r.id }
async func (r own Resource) finish() int { time.Sleep(time.Millisecond); return r.id }
type Count int
async func (n mut Count) bump() { time.Sleep(time.Millisecond); n += Count(1) }
type Holder struct { first Resource; second Resource }
async func work() {
    var holder = Holder{first: Resource{id: 1}, second: Resource{id: 20}}
    let read = holder.first.read
    println(await read())
    let bump = holder.first.bump
    println(await bump(2))
    println(await bump(3))
    let finish = holder.first.finish
    println(await finish())
    var count = Count(5)
    let increment = count.bump
    await increment()
    println(int(count))
    let unused = Resource{id: 30}
    let never = unused.finish
}
func main() { let task = go work(); task.wait() }
"#,
        "1\n3\n6\n6\n20\n6\n6\n30\n",
    );
}

#[test]
fn spawned_async_literals_method_values_and_call_once_jobs_destroy_captures_once() {
    prints(
        r#"package main
import "std/time"
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
async func (r Resource) read() int { time.Sleep(time.Millisecond); return r.id }
async func (r own Resource) finish() int { time.Sleep(time.Millisecond); return r.id }
func consume(r own Resource) { println(r.id + 100) }
func main() {
    let r = Resource{id: 1}
    let job = async func() int { time.Sleep(time.Millisecond); return r.id }
    let task = go job()
    println(task.wait())
    let r2 = Resource{id: 2}
    let task2 = go r2.finish()
    println(task2.wait())
    let r3 = Resource{id: 3}
    let read = r3.read
    let task3 = go read()
    println(task3.wait())
    let r4 = Resource{id: 4}
    let once = async func() { time.Sleep(time.Millisecond); consume(r4) }
    let task4 = go once()
    task4.wait()
    let r5 = Resource{id: 5}
    let task5 = go async func() { time.Sleep(time.Millisecond); consume(r5) }()
    task5.wait()
}
"#,
        "1\n1\n2\n2\n3\n3\n104\n4\n105\n5\n",
    );
}

#[test]
fn async_call_once_error_paths_drop_locals_and_remaining_capture_parts() {
    prints(
        r#"package main
import "std/time"
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
type Pair struct { first Resource; second Resource }
async func fail() error { time.Sleep(time.Millisecond); return error("failed") }
func consume(r own Resource) {}
async func work() error {
    let pair = Pair{first: Resource{id: 1}, second: Resource{id: 2}}
    let once = async func() error {
        let local = Resource{id: 3}
        time.Sleep(time.Millisecond)
        consume(pair.first)
        await fail()?
        return nil
    }
    await once()?
    return nil
}
func main() { let task = go work(); let err = task.wait(); println(err != nil) }
"#,
        "1\n3\n2\ntrue\n",
    );
}

#[test]
fn async_capture_panics_clean_initialized_parts_and_callers_without_leaks() {
    for (operation, expected) in [
        (
            "let once = async func() { time.Sleep(time.Millisecond); let moved = pair.first; panic(\"body failed\") }; await once()",
            "1\n2\n9\n",
        ),
        (
            "let f = async func() { time.Sleep(time.Millisecond); println(pair.first.id); panic(\"body failed\") }; await f()",
            "1\n2\n1\n9\n",
        ),
        (
            "let once = async func() { let moved = pair.first; time.Sleep(time.Millisecond); let failing = Resource{id: 8}; drop(failing) }; await once()",
            "8\n1\n2\n9\n",
        ),
    ] {
        let source = format!(
            r#"package main
import "std/time"
type Resource struct {{ id int }}
func (r mut Resource) drop() {{ println(r.id); if r.id == 8 {{ panic("drop failed") }} }}
type Pair struct {{ first Resource; second Resource }}
async func work() {{
    let guard = Resource{{id: 9}}
    let pair = Pair{{first: Resource{{id: 1}}, second: Resource{{id: 2}}}}
    {operation}
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

#[test]
fn async_callable_arguments_and_views_survive_waits_with_independent_cleanup() {
    prints(
        r#"package main
import "std/time"
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
async func invoke(f own async func() int) int { return await f() }
async func work() {
    var backing = Array<int>{1}
    let view = backing[:]
    let f = async func() int { time.Sleep(time.Millisecond); return view[0] }
    println(await f())
    backing[0] = 2
    for var i = 0; i < 2; i += 1 {
        let read = async func() int { time.Sleep(time.Millisecond); return backing[0] + i }
        println(await read())
    }
}
func main() {
    let r = Resource{id: 5}
    let task = go invoke(async func() int { time.Sleep(time.Millisecond); return r.id })
    println(task.wait())
    let worked = go work()
    worked.wait()
}
"#,
        "5\n5\n1\n2\n3\n",
    );
}

#[test]
fn async_callable_argument_and_environment_replacement_panics_clean_every_owner() {
    for (body, expected) in [
        (
            "let n = await make(1)(Resource{id: 2}, await failKey()); println(n)",
            "2\n1\n9\n",
        ),
        ("var f = make(8); f = make(2)", "8\n2\n9\n"),
    ] {
        let source = format!(
            r#"package main
import "std/time"
type Resource struct {{ id int }}
func (r mut Resource) drop() {{ println(r.id); if r.id == 8 {{ panic("environment drop failed") }} }}
func make(id int) async func(own Resource, int) int {{
    let r = Resource{{id: id}}
    return async func(arg own Resource, n int) int {{ time.Sleep(time.Millisecond); return r.id + arg.id + n }}
}}
async func failKey() int {{ time.Sleep(time.Millisecond); panic("key failed"); return 0 }}
async func work() {{ let guard = Resource{{id: 9}}; {body} }}
func main() {{ let task = go work(); task.wait() }}
"#
        );
        let output = run(&source);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(stdout(&output), expected, "{output:?}");
        assert!(!stderr(&output).contains("leak:"), "{output:?}");
    }
}

#[test]
fn async_closure_replacement_and_detached_execution_preserve_owned_lifetimes() {
    prints(
        r#"package main
import "std/time"
type Resource struct { id int }
func (r mut Resource) drop() { println(r.id) }
func consume(r own Resource) { println(r.id) }
type Holder struct { f async func() int }
func create(id int) async func() int {
    let r = Resource{id: id}
    return async func() int { time.Sleep(time.Millisecond); return r.id }
}
async func work() {
    var holder = Holder{f: create(1)}
    holder.f = create(2)
    println(await (holder.f)())
    let completed = channel<int>()
    {
        let r = Resource{id: 3}
        let task = go async func() { time.Sleep(time.Millisecond); consume(r); completed.send(1) }()
    }
    let value, ok = completed.receive()
    println(value)
    println(ok)
}
func main() { let task = go work(); task.wait() }
"#,
        "1\n2\n3\n3\n1\ntrue\n2\n",
    );
}
