use super::prints;

#[test]
fn text_shared_sliced_and_wrapped_across_tasks_is_freed_without_leaks() {
    prints(
        r#"package main

import "std/errors"
import "std/strconv"
import "std/strings"

let Greeting = "hello " + strconv.Itoa(7)
let Base = errors.New("base " + strconv.Itoa(1))

func extend(shared string, seed int) string {
    var mine = shared + "-" + strconv.Itoa(seed)
    for var i = 0; i < 1000; i += 1 {
        mine += strconv.Itoa(i % 10)
    }
    return mine[0:shared.len() + 2] + mine[mine.len() - 3:]
}

func wrapped(seed int) error {
    return errors.Wrap(Base, "task " + strconv.Itoa(seed))
}

func middle() string {
    var text = ""
    for var i = 0; i < 600000; i += 1 {
        text += strconv.Itoa(i % 10)
    }
    return text[300000:300005]
}

func words() string {
    let line = "  alpha beta  " + strconv.Itoa(3) + "  "
    let trimmed = strings.TrimSpace(line)
    let parts = strings.Split(trimmed, " ")
    return parts[1]
}

func main() {
    let shared = "shared " + strconv.Itoa(42)
    let a = go extend(shared, 1)
    let b = go extend(shared, 2)
    let c = go extend(shared, 3)
    let d = go extend(shared, 4)
    println(a.wait())
    println(b.wait())
    println(c.wait())
    println(d.wait())
    println(shared)
    let e = go wrapped(5)
    let err = e.wait()
    println(errors.Message(err))
    println(errors.Unwrap(err) == Base)
    let m = go middle()
    println(m.wait())
    let w = go words()
    println(w.wait())
    println(Greeting)
}
"#,
        "shared 42-1789\nshared 42-2789\nshared 42-3789\nshared 42-4789\nshared 42\n\
         task 5: base 1\ntrue\n01234\nbeta\nhello 7\n",
    );
}
