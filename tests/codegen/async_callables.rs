use super::{prints, run, stderr, stdout};

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
