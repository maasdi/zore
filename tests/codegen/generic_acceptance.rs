use super::{run_controlled, stderr, stdout};

fn assert_generic_cleanup(wrapped: bool) {
    for asynchronous in [false, true] {
        for mode in 0..3 {
            let prefix = if asynchronous { "async " } else { "" };
            let suspension = if asynchronous {
                "let _, _ = gate.receive(); println(\"resumed\")"
            } else {
                ""
            };
            let (parameter, result, input, declarations, transferred) = if wrapped {
                (
                    "Box<T>",
                    "Box<T>",
                    "Box<Item>{tag: 20, item: Item{id: 1}}",
                    "type Box<T any> struct { tag int; item T }\nfunc (b mut Box<T>) drop() { if b.tag != 0 { println(b.tag) } }",
                    "20\n1\n",
                )
            } else {
                ("T", "T", "Item{id: 1}", "", "1\n")
            };
            let call = if asynchronous {
                format!(
                    "let task = go process({input}, gate, {mode})
                    waitPending(1)
                    println(\"pending\")
                    gate.send(1)
                    waitIdle(0)
                    let result, err = task.wait()"
                )
            } else {
                format!("let result, err = process({input}, gate, {mode})")
            };
            let source = format!(
                "package main
                type Item struct {{ id int }}
                func (item mut Item) drop() {{ if item.id != 0 {{ println(item.id) }} }}
                {declarations}
                func check(mode int) error {{
                    if mode == 1 {{ return error(\"generic error\") }}
                    return nil
                }}
                {prefix}func process<T any>(value own {parameter}, gate channel<int>, mode int) ({result}, error) {{
                    let local = Item{{id: 2}}
                    {suspension}
                    if mode == 2 {{ panic(\"generic panic\") }}
                    check(mode)?
                    return value, nil
                }}
                func main() {{
                    waitIdle(0)
                    let caller = Item{{id: 9}}
                    let gate = channel<int>()
                    {call}
                    println(\"retrieved\")
                    println(err == error(\"generic error\"))
                    drop(result)
                }}"
            );
            let output = run_controlled(&source);
            let resumed = if asynchronous {
                "pending\nresumed\n"
            } else {
                ""
            };
            let expected = match mode {
                0 => format!("{resumed}2\nretrieved\nfalse\n{transferred}9\n"),
                1 => format!("{resumed}2\n{transferred}retrieved\ntrue\n9\n"),
                _ => format!("{resumed}2\n{transferred}9\n"),
            };
            assert_eq!(
                output.status.code(),
                Some(if mode == 2 { 2 } else { 0 }),
                "{source}\n{}",
                stderr(&output)
            );
            assert_eq!(stdout(&output), expected, "{source}");
            if mode == 2 {
                let report = stderr(&output);
                assert!(
                    report.contains("panic in the main task: generic panic at test.ore:"),
                    "{source}\n{report}"
                );
                if asynchronous {
                    assert!(
                        report.contains("panic in task 1: generic panic at test.ore:"),
                        "{source}\n{report}"
                    );
                }
            } else {
                assert!(output.stderr.is_empty(), "{}", stderr(&output));
            }
            assert!(!stderr(&output).contains("leak:"), "{}", stderr(&output));
        }
    }
}

#[test]
fn generic_owned_arguments_clean_up_on_success_propagation_and_panic_after_pending() {
    assert_generic_cleanup(false);
}

#[test]
fn generic_struct_fields_clean_up_on_success_propagation_and_panic_after_pending() {
    assert_generic_cleanup(true);
}

#[test]
fn generic_methods_with_the_same_name_keep_their_receiver_identity() {
    let source = r#"package main
type Left<T copyable> struct { value T }
type Right<T copyable> struct { value T }
func (left Left<T>) Get() T { return left.value }
func (right Right<T>) Get() T { return right.value }
func (left mut Left<T>) drop() { println("left") }
func (right mut Right<T>) drop() { println("right") }
type Getter interface { Get() int }
func read(value Getter) int { return value.Get() }
func Get<T copyable>(value T) T { return value }
func main() {
    let left = Left<int>{value: 11}
    let right = Right<int>{value: 22}
    println(read(left))
    println(read(right))
    println(Get(33))
}
"#;
    let output = super::run(source);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "11\n22\n33\nright\nleft\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}
