use super::{TempDir, run_bounded, run_controlled, stderr, stdout};

fn prints_bounded(source: &str, expected: &str) {
    let output = run_bounded(source, None, false);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), expected);
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

fn observer(imports: &str, parameters: &str, operation: &str, main: &str) -> String {
    format!(
        "package main
{imports}
type Owner struct {{ items Array<int> }}
func (owner mut Owner) drop() {{ println(\"owner\") }}
type Watch struct {{ stack []int; heap []int }}
func (watch mut Watch) drop() {{
    println(\"watch\")
    println(watch.stack[0] + watch.heap[0])
}}
async func observe({parameters}fail bool) int {{
    var stack = [int; 2]{{10, 11}}
    var owner = Owner{{items: Array<int>{{20, 21}}}}
    let watch = Watch{{stack: stack[:], heap: owner.items[:]}}
    {operation}
    println(\"resumed\")
    if fail {{ panic(\"suspension panic\") }}
    return watch.stack[1] + watch.heap[1]
}}
func main() {{ {main} }}"
    )
}

fn normal_and_panic_cleanup(source: &str) {
    for fail in [false, true] {
        let output = run_controlled(&source.replace("FAIL", if fail { "true" } else { "false" }));
        let expected = if fail {
            "resumed\nwatch\n30\nowner\n"
        } else {
            "resumed\nwatch\n30\nowner\n32\n"
        };
        assert_eq!(
            output.status.code(),
            Some(if fail { 2 } else { 0 }),
            "{}",
            stderr(&output)
        );
        assert_eq!(stdout(&output), expected);
        if fail {
            assert!(
                stderr(&output).contains("panic in task 1: suspension panic"),
                "{}",
                stderr(&output)
            );
        } else {
            assert!(output.stderr.is_empty(), "{}", stderr(&output));
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn destructor_views_survive_an_observed_timer_suspend_and_both_cleanup_paths() {
    normal_and_panic_cleanup(&observer(
        "import \"std/time\"",
        "",
        "time.Sleep(time.Hour)",
        "let task = go observe(FAIL); waitPending(1); expireTimers(1); println(task.wait())",
    ));
}

#[test]
fn destructor_views_survive_an_observed_mutex_suspend_and_both_cleanup_paths() {
    normal_and_panic_cleanup(&observer(
        "",
        "lock Mutex<int>, started channel<int>, ",
        "started.send(1); let value = lock.withLock(func(value mut int) int { return value })",
        "let lock = mutex(0)
        let started = channel<int>(1)
        let task = lock.withLock(func(value mut int) Task<int> {
            let child = go observe(lock, started, FAIL)
            let ready, ok = started.receive()
            waitPending(1)
            return child
        })
        println(task.wait())",
    ));
}

#[test]
fn destructor_views_survive_an_observed_socket_suspend_and_both_cleanup_paths() {
    normal_and_panic_cleanup(&observer(
        "import \"std/net\"",
        "conn own net.Conn, ",
        "var input = [byte; 1]{0}; let count, err = conn.Read(input[:]); if err != nil { panic(\"read failed\") }; if count != 1 { panic(\"short read\") }",
        "(let listener, var err) = net.Listen(\"tcp\", \"127.0.0.1:0\")
        if err != nil { panic(\"listen failed\") }
        (let peer, err) = net.Dial(\"tcp\", listener.Addr())
        if err != nil { panic(\"dial failed\") }
        (let conn, err) = listener.Accept()
        if err != nil { panic(\"accept failed\") }
        let task = go observe(conn, FAIL)
        waitPending(1)
        let data = [byte; 1]{9}
        let count, writeErr = peer.Write(data[:])
        if writeErr != nil { panic(\"write failed\") }
        if count != 1 { panic(\"short write\") }
        println(task.wait())",
    ));
}

#[test]
fn moved_file_buffers_and_socket_handles_keep_their_single_owner() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("contents.bin");
    std::fs::write(&path, [3u8, 5, 7]).unwrap();
    let source = r#"package main
import "std/os"
import "std/net"
import "std/io"
func load(path string) Array<byte> {
    let bytes, err = os.ReadFile(path)
    if err != nil { panic("read failed") }
    let payload = bytes
    return payload
}
func main() {
    let bytes = load(PATH)
    let payload = bytes
    println(payload[0])
    let info, statErr = os.Stat(PATH)
    if statErr != nil { panic("stat failed") }
    let copy = info
    println(copy.Size() == info.Size())
    println(payload.len())
    (let listener, var err) = net.Listen("tcp", "127.0.0.1:0")
    if err != nil { panic("listen failed") }
    let endpoint = listener
    (let conn, err) = net.Dial("tcp", endpoint.Addr())
    if err != nil { panic("dial failed") }
    (let accepted, err) = endpoint.Accept()
    if err != nil { panic("accept failed") }
    let client = conn
    let server = accepted
    let data = [byte; 1]{9}
    let sent, writeErr = client.Write(data[:])
    if writeErr != nil { panic("write failed") }
    if sent != 1 { panic("short write") }
    var received = [byte; 1]{0}
    let read, readErr = server.Read(received[:])
    if readErr != nil { panic("read failed") }
    if read != 1 { panic("short read") }
    println(received[0])
    let closeErr = client.Close()
    if closeErr != nil { panic("close failed") }
    let eof, eofErr = server.Read(received[:])
    if eofErr != io.EOF { panic("eof failed") }
    println(eof)
}"#
    .replace("PATH", &format!("{:?}", path.to_str().unwrap()));
    let output = run_bounded(&source, None, false);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "3\ntrue\n3\n9\n0\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn a_destructor_can_add_a_fresh_resource_without_resurrecting_itself() {
    prints_bounded(
        r#"package main
type Resource struct { name string; children Array<Resource> }
func (r mut Resource) drop() {
    println(r.name)
    if r.name == "original" { r.children.push(Resource{name: "fresh", children: Array<Resource>{}}) }
}
func main() { let value = Resource{name: "original", children: Array<Resource>{}} }
"#,
        "original\nfresh\n",
    );
}

#[test]
fn nested_returned_and_cloned_views_keep_their_backing_through_moves_and_calls() {
    prints_bounded(
        r#"package main
type View struct { items []int }
type OwnedView struct { items []int; extra Array<int> }
func rows(items []int) Array<Array<View>> { return Array<Array<View>>{Array<View>{View{items: items}}} }
func mappedRows(items []int) map[int]Array<View> { return map[int]Array<View>{1: Array<View>{View{items: items}}} }
func mappedViews(items []int) Array<map[int]View> { return Array<map[int]View>{map[int]View{1: View{items: items}}} }
func read(view OwnedView) []int { return view.items }
func forward(view own OwnedView) []int { return read(view) }
func main() {
    var backing = [int; 1]{7}
    {
        let result = rows(backing[:])
        let moved = result
        println(moved[0][0].items[0])
    }
    {
        let result = mappedRows(backing[:])
        var moved = result
        let found, rows = moved.remove(1)
        if !found { panic("missing row") }
        println(rows[0].items[0])
    }
    {
        let result = mappedViews(backing[:])
        let moved = result
        let found, view = moved[0][1]
        if !found { panic("missing view") }
        println(view.items[0])
    }
    {
        let original = OwnedView{items: backing[:], extra: Array<int>{1}}
        let duplicate = clone(original)
        drop(original)
        let result = forward(duplicate)
        println(result[0])
    }
    backing[0] = 8
    println(backing[0])
}"#,
        "7\n7\n7\n7\n8\n",
    );
}
