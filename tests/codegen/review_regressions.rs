use super::*;

#[test]
fn empty_interface_adapters_destroy_transferred_arguments_in_plain_and_async_calls() {
    for asynchronous in [false, true] {
        for receiver in ["", "mut ", "own "] {
            for borrowed in [false, true] {
                if borrowed && receiver == "own " {
                    continue;
                }
                let conversion = if borrowed {
                    "invoke(empty)"
                } else {
                    "var smaller Small = empty; smaller.Take(Item{id: 1}, Item{id: 2})"
                };
                let mode = if receiver == "mut " { "mut " } else { "" };
                let prefix = if asynchronous { "async " } else { "" };
                let main = if asynchronous {
                    "func main() { let task = go work(); task.wait() }"
                } else {
                    "func main() { work() }"
                };
                let invoke = if borrowed {
                    format!(
                        "{prefix}func invoke(s {mode}Small) {{ s.Take(Item{{id: 1}}, Item{{id: 2}}) }}"
                    )
                } else {
                    String::new()
                };
                let conversion = if borrowed && asynchronous {
                    "await invoke(empty)"
                } else {
                    conversion
                };
                let source = format!(
                    "package main
                    type Item struct {{ id int }}
                    func (item mut Item) drop() {{ println(item.id) }}
                    type Large interface {{ {receiver}Take(a own Item, b own Item); Extra() }}
                    type Small interface {{ {receiver}Take(a own Item, b own Item) }}
                    {invoke}
                    {prefix}func work() {{
                        let ch = channel<Large>(1)
                        ch.close()
                        var empty, ok = ch.receive()
                        _ = ok
                        {conversion}
                    }}
                    {main}"
                );
                let output = run(&source);
                assert_eq!(
                    output.status.code(),
                    Some(2),
                    "{source}\n{}",
                    stderr(&output)
                );
                assert_eq!(stdout(&output), "2\n1\n", "{source}");
                assert!(stderr(&output).contains("method call on an empty interface value"));
            }
        }
    }
}

#[test]
fn float32_formatting_rounds_before_classifying_special_values() {
    prints(
        r#"package main
import "zore/strconv"
func main() {
    for format in "fFeEgG" {
        if format == 'F' { continue }
        println(strconv.FormatFloat(1e100, format, -1, 32))
        println(strconv.FormatFloat(-1e100, format, 3, 32))
    }
    println(strconv.FormatFloat(-1e-100, 'g', -1, 32))
    println(strconv.FormatFloat(1e-100, 'g', -1, 32))
    println(strconv.FormatFloat(1e100, 'e', -1, 64))
}"#,
        "+Inf\n-Inf\n+Inf\n-Inf\n+Inf\n-Inf\n+Inf\n-Inf\n+Inf\n-Inf\n-0.0\n0.0\n1e+100\n",
    );
}

#[test]
fn empty_message_errors_remain_present_in_wrapped_and_formatted_chains() {
    prints(
        r#"package main
import "zore/errors"
import "zore/fmt"
func main() {
    let none error = nil
    let cause = error("")
    let wrapped = errors.Wrap(cause, "context")
    let copy = wrapped
    let outer = fmt.Errorf("outer: %w", copy)
    println(errors.Unwrap(wrapped) == cause)
    println(errors.Is(outer, cause))
    println(errors.Unwrap(errors.Unwrap(outer)) == cause)
    println(errors.Unwrap(cause) == nil)
    println(errors.Wrap(none, "context") == nil)
    let absent = fmt.Errorf("absent: %w", none)
    println(errors.Unwrap(absent) == nil)
    let empty = fmt.Errorf("%w", cause)
    println(errors.Unwrap(empty) == nil)
    println(empty == cause)
}"#,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn read_full_succeeds_when_the_final_bytes_arrive_with_an_error() {
    prints(
        r#"package main
import "zore/io"
type Reader struct { left int; err error }
func (r mut Reader) Read(p mut []byte) (int, error) {
    if r.left == 0 { return 0, r.err }
    p[0] = 65
    r.left -= 1
    if r.left == 0 { return 1, r.err }
    return 1, nil
}
func main() {
    var r = Reader{left: 2, err: io.EOF}
    var buf = [byte; 2]{0, 0}
    let n, err = io.ReadFull(r, buf[:])
    println(n)
    println(err == nil)
    var partial = Reader{left: 1, err: io.EOF}
    let m, short = io.ReadFull(partial, buf[:])
    println(m)
    println(short == error("unexpected EOF"))
    let zero, end = io.ReadFull(partial, buf[:])
    println(zero)
    println(end == io.EOF)
    var other = Reader{left: 2, err: error("other")}
    let full, last = io.ReadFull(other, buf[:])
    println(full)
    println(last == nil)
}"#,
        "2\ntrue\n1\ntrue\n0\ntrue\n2\ntrue\n",
    );
}

#[test]
fn copying_and_flushing_retry_short_writes_and_reject_zero_progress() {
    prints(
        r#"package main
import "zore/io"
import "zore/bytes"
import "zore/bufio"
type Writer struct { limit int }
func (w mut Writer) Write(p []byte) (int, error) {
    if w.limit == 0 { return 0, nil }
    println(p[0])
    w.limit -= 1
    return 1, nil
}
func main() {
    var src = bytes.NewBufferString("abc")
    var dst = Writer{limit: 3}
    let count, err = io.Copy(dst, src)
    println(count)
    println(err == nil)
    var stalled = Writer{limit: 1}
    var more = bytes.NewBufferString("ab")
    let n, fail = io.Copy(stalled, more)
    println(n)
    println(fail == error("io.Copy: no write progress"))
    var buffered = bufio.NewWriter(Writer{limit: 3})
    let _, _ = buffered.WriteString("abc")
    println(buffered.Flush() == nil)
    println(buffered.Buffered())
    var pending = bufio.NewWriter(Writer{limit: 1})
    let _, _ = pending.WriteString("abc")
    println(pending.Flush() == error("bufio.Writer: no write progress"))
    println(pending.Buffered())
    println(pending.Flush() == error("bufio.Writer: no write progress"))
    println(pending.Buffered())
}"#,
        "97\n98\n99\n3\ntrue\n97\n1\ntrue\n97\n98\n99\ntrue\n0\n97\ntrue\n2\ntrue\n2\n",
    );
}

#[test]
fn partial_io_errors_preserve_counts_and_unwritten_buffered_bytes() {
    prints(
        r#"package main
import "zore/io"
import "zore/bytes"
import "zore/bufio"
type Writer struct { count int; fail bool }
func (w mut Writer) Write(p []byte) (int, error) {
    if w.fail { return w.count, error("stopped") }
    return w.count, nil
}
func main() {
    var input = bytes.NewBufferString("abc")
    var writer = Writer{count: 1, fail: true}
    let n, err = io.Copy(writer, input)
    println(n)
    println(err == error("stopped"))
    var pending = bufio.NewWriter(Writer{count: 1, fail: true})
    let _, _ = pending.WriteString("abc")
    println(pending.Flush() == error("stopped"))
    println(pending.Buffered())
    for count in [int; 2]{-1, 4} {
        var src = bytes.NewBufferString("abc")
        var dst = Writer{count: count, fail: false}
        let copied, invalid = io.Copy(dst, src)
        println(copied)
        println(invalid == error("io.Copy: invalid write count"))
        var buf = bufio.NewWriter(Writer{count: count, fail: false})
        let _, _ = buf.WriteString("abc")
        println(buf.Flush() == error("bufio.Writer: invalid write count"))
        println(buf.Buffered())
    }
}"#,
        "1\ntrue\ntrue\n2\n0\ntrue\ntrue\n3\n0\ntrue\ntrue\n3\n",
    );
}

#[test]
fn copying_and_full_reads_reject_out_of_range_read_counts() {
    prints(
        r#"package main
import "zore/io"
import "zore/bytes"
type Reader struct { count int }
func (r mut Reader) Read(p mut []byte) (int, error) { return r.count, nil }
func main() {
    for count in [int; 2]{-1, 40000} {
        var src = Reader{count: count}
        var dst = bytes.NewBufferString("")
        let copied, err = io.Copy(dst, src)
        println(copied)
        println(err == error("io.Copy: invalid read count"))
    }
    for count in [int; 2]{-1, 3} {
        var src = Reader{count: count}
        var buf = [byte; 2]{0, 0}
        let read, err = io.ReadFull(src, buf[:])
        println(read)
        println(err == error("io.ReadFull: invalid read count"))
    }
}"#,
        "0\ntrue\n0\ntrue\n0\ntrue\n0\ntrue\n",
    );
}

#[test]
fn buffered_write_reports_accepted_bytes_when_its_flush_fails() {
    prints(
        r#"package main
import "zore/bufio"
type Writer struct {}
func (w mut Writer) Write(p []byte) (int, error) { return 0, error("stopped") }
func main() {
    var buf = bufio.NewWriter(Writer{})
    var data = Array<byte>{}
    for var i = 0; i < 5000; i += 1 { data.push(97) }
    let n, err = buf.Write(data[:])
    println(n)
    println(err == error("stopped"))
    println(buf.Buffered())
}"#,
        "5000\ntrue\n5000\n",
    );
}
