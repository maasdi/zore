use super::{run_controlled, stderr, stdout};

fn consuming_dispatch(receiver: &str, outcome: i32, suspending: bool) -> String {
    let wait = if suspending {
        "let _, _ = gate.receive()"
    } else {
        ""
    };
    let asynchronous = if suspending { "async " } else { "" };
    let await_call = if suspending { "await " } else { "" };
    let call = if suspending {
        "let task = go invoke(owned, argument, gate)
    waitPending(1)
    println(\"pending\")
    gate.send(1)
    waitIdle(0)
    println(\"quiescent\")
    let result, err = task.wait()"
    } else {
        "let result, err = invoke(owned, argument, gate)"
    };
    format!(
        r#"package main
type Consumer interface {{ {asynchronous}own Take(argument own Resource, gate channel<int>) (Resource, error) }}
type Resource struct {{ id int; values Array<int> }}
func (resource mut Resource) drop() {{
    if resource.id != 0 {{ println(resource.id) }}
}}
func fail() error {{
    if {outcome} == 1 {{ return error("interface failure") }}
    if {outcome} == 2 {{ panic("interface panic") }}
    return nil
}}
{asynchronous}func (receiver {receiver}Resource) Take(argument own Resource, gate channel<int>) (Resource, error) {{
    {wait}
    println("resumed")
    println(receiver.values[0] + argument.values[0])
    fail()?
    return argument, nil
}}
{asynchronous}func invoke(owned own Consumer, argument own Resource, gate channel<int>) (Resource, error) {{
    let result = {await_call}owned.Take(argument, gate)?
    return result, nil
}}
func main() {{
    waitIdle(0)
    let gate = channel<int>()
    let owned Consumer = Resource{{id: 11, values: Array<int>{{111}}}}
    let argument = Resource{{id: 22, values: Array<int>{{222}}}}
    {call}
    println(err == {expected_error})
    if err == nil {{
        println("caller owns result")
        println(result.id)
        println(result.values[0])
    }}
    drop(result)
    println("caller finished")
}}
"#,
        expected_error = if outcome == 1 {
            "error(\"interface failure\")"
        } else {
            "nil"
        }
    )
}

fn assert_consuming_dispatch(receiver: &str, outcome: i32, suspending: bool) {
    let source = consuming_dispatch(receiver, outcome, suspending);
    let output = run_controlled(&source);
    let mut expected = String::new();
    if suspending {
        expected.push_str("pending\n");
    }
    expected.push_str("resumed\n333\n");
    if outcome != 0 {
        expected.push_str("22\n");
    }
    expected.push_str("11\n");
    if suspending {
        expected.push_str("quiescent\n");
    }
    if outcome == 0 {
        expected.push_str("true\ncaller owns result\n22\n222\n22\ncaller finished\n");
    } else if outcome == 1 {
        expected.push_str("true\ncaller finished\n");
    }
    assert_eq!(
        output.status.code(),
        Some(if outcome == 2 { 2 } else { 0 }),
        "{source}\n{}",
        stderr(&output)
    );
    assert_eq!(stdout(&output), expected, "{source}");
    let report = stderr(&output);
    if outcome == 2 {
        assert!(
            report.contains("panic in the main task: interface panic at test.ore:"),
            "{source}\n{report}"
        );
        if suspending {
            assert!(
                report.contains("panic in task 1: interface panic at test.ore:"),
                "{source}\n{report}"
            );
        }
        assert!(!report.contains("leak:"), "{source}\n{report}");
    } else {
        assert!(report.is_empty(), "{source}\n{report}");
    }
}

#[test]
fn consuming_interfaces_clean_up_owned_and_weaker_receivers_on_every_exit() {
    for receiver in ["own ", "", "mut "] {
        for outcome in 0..3 {
            assert_consuming_dispatch(receiver, outcome, false);
        }
    }
}

#[test]
fn consuming_interfaces_clean_up_after_observed_pending_and_deliver_every_outcome() {
    for receiver in ["own ", "", "mut "] {
        for outcome in 0..3 {
            assert_consuming_dispatch(receiver, outcome, true);
        }
    }
}
