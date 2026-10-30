use super::{run_controlled, stderr, stdout};

#[test]
fn buffered_senders_wait_for_capacity_before_transferring_owned_messages() {
    for asynchronous in [false, true] {
        let prefix = if asynchronous { "async " } else { "" };
        let checkpoint = if asynchronous {
            "waitPending(1)"
        } else {
            "waitBlocked(1)"
        };
        let source = format!(
            r#"package main
type Resource struct {{ id int; values Array<int> }}
func (resource mut Resource) drop() {{ println(resource.id) }}
{prefix}func forward(queue channel<Resource>, value own Resource) {{ queue.send(value) }}
func main() {{
    let queue = channel<Resource>(1)
    queue.send(Resource{{id: 1, values: Array<int>{{11}}}})
    let task = go forward(queue, Resource{{id: 2, values: Array<int>{{22}}}})
    {checkpoint}
    println("full")
    let first, firstOK = queue.receive()
    waitIdle(0)
    task.wait()
    let second, secondOK = queue.receive()
    queue.close()
    println(first.id)
    println(first.values[0])
    println(second.id)
    println(second.values[0])
    println(firstOK)
    println(secondOK)
    drop(first)
    drop(second)
}}
"#
        );
        let output = run_controlled(&source);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        assert_eq!(stdout(&output), "full\n1\n11\n2\n22\ntrue\ntrue\n1\n2\n");
        assert!(output.stderr.is_empty(), "{}", stderr(&output));
    }
}
