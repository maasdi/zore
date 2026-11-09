use super::*;
use std::time::Duration;

const SUPPORT: &str = r#"
import "std/compress/gzip"
import "std/io"
import "std/bytes"
import "std/errors"

func check(ok bool, label string) { if !ok { panic(label) } }

type Sink struct {
    total int
    calls int
    mode int
    done channel<int>
    gate channel<bool>
}
func (s mut Sink) Write(data []byte) (int, error) {
    s.calls += 1
    if s.mode == 7 { let _, _ = s.gate.receive(); s.mode = 0 }
    if s.mode == 8 && s.total >= 10 {
        s.total += 1
        return 1, error("payload failed")
    }
    if s.mode == 1 { return 0, nil }
    if s.mode == 2 { return data.len() + 1, nil }
    if s.mode == 3 { return -1, nil }
    if s.mode == 4 && s.calls == 2 {
        s.total += 1
        return 1, error("sink failed")
    }
    var count = data.len()
    if count > 2 { count = 2 }
    s.total += count
    return count, nil
}
func (s mut Sink) drop() { s.done.send(s.total) }
func sink(mode int, done channel<int>) Sink {
    return Sink{total: 0, calls: 0, mode: mode, done: done, gate: channel<bool>(1)}
}

type Feed struct {
    data Array<byte>
    at int
    mode int
    done channel<int>
}
func (f mut Feed) Read(buf mut []byte) (int, error) {
    if f.mode == 1 { return 0, nil }
    if f.mode == 2 { return buf.len() + 1, nil }
    if f.mode == 3 { return -1, nil }
    if f.mode == 4 { return 0, error("source failed") }
    if f.at == f.data.len() { return 0, io.EOF }
    buf[0] = f.data[f.at]
    f.at += 1
    if f.at == f.data.len() {
        if f.mode == 5 { return 1, io.EOF }
        if f.mode == 6 { return 1, error("last byte failed") }
    }
    return 1, nil
}
func (f mut Feed) drop() { f.done.send(f.at) }
func feed(data []byte, mode int, done channel<int>) Feed {
    return Feed{data: bytes.Clone(data), at: 0, mode: mode, done: done}
}
func emptyMember() Array<byte> {
    return Array<byte>{31, 139, 8, 0, 0, 0, 0, 0, 0, 255, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0}
}
"#;

fn run(body: &str, controlled: bool) -> Output {
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("gzip");
    let body = body
        .replace("func main() {", "func scenario() error {")
        .replace("println(\"ok\")", "println(\"ok\")\n    return nil");
    let source = format!(
        "package main\n{SUPPORT}\n{body}\nfunc main() {{ let err = scenario(); if err != nil {{ panic(errors.Message(err)) }} }}\n"
    );
    if controlled {
        controlled_runtime::build_with_workers(&source, &executable, 1);
    } else {
        build_file(&source, &executable).unwrap_or_else(|error| panic!("build: {error:?}"));
    }
    bounded_process::run(
        Command::new(executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap()
}

fn succeeds(body: &str, controlled: bool) {
    let output = run(body, controlled);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn gzip_finalization_short_writes_and_drop_have_explicit_semantics() {
    succeeds(
        r#"func main() {
    let done = channel<int>(20)
    var empty = gzip.NewWriter(sink(0, done), 0, 20)?
    check(empty.Finish() == nil && empty.Finish() == nil, "idempotent finish")
    let count, after = empty.Write(Array<byte>{}[:])
    _ = after
    check(count == 0 && after == error("gzip: writer finished"), "write after finish")
    check(empty.Close() == nil, "close finished")
    let size, _ = done.receive()
    check(size == 20, "empty member length")
    var writer = gzip.NewWriter(sink(0, done), 100, 1000)?
    let data = Array<byte>{1, 2, 3, 1, 2, 3}
    let accepted, err = writer.Write(data[:])
    _ = err
    check(accepted == 6 && err == nil, "short writes retried")
    check(writer.Close() == nil, "explicit finalization")
    let compressed, _ = done.receive()
    check(compressed > 20, "compressed output")
    let unfinished = gzip.NewWriter(sink(0, done), 0, 20)?
    drop(unfinished)
    let dropped, _ = done.receive()
    check(dropped == 0, "drop does not write")
    println("ok")
}
"#,
        false,
    );
}

#[test]
fn gzip_sink_errors_are_sticky_and_constructor_failures_release_ownership() {
    succeeds(
        r#"func main() {
    let done = channel<int>(20)
    for mode in Array<int>{1, 2, 3, 4} {
        var writer = gzip.NewWriter(sink(mode, done), 10, 100)?
        let count, err = writer.Write(Array<byte>{1, 2, 3}[:])
    _ = err
        check(count == 0 && err != nil, "header error")
        let again, repeated = writer.Write(Array<byte>{1}[:])
    _ = repeated
        check(again == 0 && repeated == err && writer.Finish() == err, "sticky failure")
        check(writer.Close() == err, "close reports failure")
        let _, _ = done.receive()
    }
    for limit in Array<int>{-1, gzip.MaxBytes + 1} {
        let failed, err = gzip.NewWriter(sink(0, done), limit, 100)
    _ = err
        check(err == error("gzip: invalid byte limit"), "invalid input limit")
        drop(failed)
        let size, _ = done.receive()
        check(size == 0, "invalid constructor no output")
    }
    var partialWriter = gzip.NewWriter(sink(8, done), 10, 100)?
    let acceptedData, payloadErr = partialWriter.Write(Array<byte>{1, 2, 3}[:])
    check(payloadErr == error("payload failed"), "payload error")
    check(acceptedData == 3 && partialWriter.Close() == payloadErr, "accepted chunk and sticky payload error")
    let written, _ = done.receive()
    check(written == 11, "partial payload prefix retained")
    var trailerWriter = gzip.NewWriter(sink(8, done), 0, 100)?
    check(trailerWriter.Finish() == error("payload failed"), "sink trailer error")
    check(trailerWriter.Close() == error("payload failed"), "sticky sink trailer error")
    let trailerBytes, _ = done.receive()
    check(trailerBytes == 11, "partial trailer retained")
    var bounded = gzip.NewWriter(sink(0, done), 2, 100)?
    let accepted, limitErr = bounded.Write(Array<byte>{1, 2, 3}[:])
    check(accepted == 0 && limitErr == error("gzip: input byte limit exceeded"), "input preflight")
    check(bounded.Close() == limitErr, "failed close no output")
    let size, _ = done.receive()
    check(size == 0, "rejected write no header")
    var compressed = gzip.NewWriter(sink(0, done), 0, 19)?
    let finishErr = compressed.Finish()
    check(finishErr == error("gzip: compressed byte limit exceeded"), "trailer limit")
    check(compressed.Close() == finishErr, "sticky trailer failure")
    let partial, _ = done.receive()
    check(partial == 12, "raw DEFLATE prefix retained before trailer limit")
    println("ok")
}
"#,
        false,
    );
}

#[test]
fn gzip_reader_handles_final_bytes_with_errors_and_releases_sources() {
    succeeds(
        r#"func main() {
    let done = channel<int>(30)
    let data = emptyMember()
    for mode in Array<int>{0, 5} {
        var reader = gzip.NewReader(feed(data[:], mode, done), 20, 0)?
        var buf = Array<byte>{0}
        let zero, emptyErr = reader.Read(buf[:0])
    _ = emptyErr
        check(zero == 0 && emptyErr == nil, "empty read")
        let count, err = reader.Read(buf[:])
    _ = err
        check(count == 0 && err == io.EOF, "validated EOF")
        let again, repeated = reader.Read(buf[:])
    _ = repeated
        check(again == 0 && repeated == io.EOF, "sticky EOF")
        check(reader.Close() == nil, "close after EOF")
        let consumed, _ = done.receive()
        check(consumed == 20, "one byte source")
    }
    var failed = gzip.NewReader(feed(data[:], 6, done), 20, 0)?
    let _, err = io.ReadAll(failed)
    _ = err
    check(err == error("last byte failed"), "source error alongside trailer")
    check(failed.Close() == err, "close returns failure")
    let _, _ = done.receive()
    let unread = gzip.NewReader(feed(data[:], 0, done), 20, 0)?
    check(unread.Close() == nil, "close does not validate")
    let consumed, _ = done.receive()
    check(consumed == 0, "close does not drain")
    println("ok")
}
"#,
        false,
    );
}

#[test]
fn gzip_reader_rejects_stalls_bad_counts_and_limits_with_sticky_errors() {
    succeeds(
        r#"func main() {
    let done = channel<int>(30)
    let data = emptyMember()
    for mode in Array<int>{1, 2, 3, 4} {
        var reader = gzip.NewReader(feed(data[:], mode, done), 20, 0)?
        let partial, err = io.ReadAll(reader)
    _ = err
        check(partial.len() == 0 && err != nil, "bad source")
        var buf = Array<byte>{0}
        let count, repeated = reader.Read(buf[:])
    _ = repeated
        check(count == 0 && repeated == err, "sticky reader error")
        check(reader.Close() == err, "close reports source error")
        let _, _ = done.receive()
    }
    var limited = gzip.NewReader(feed(data[:], 0, done), 19, 0)?
    let _, limitErr = io.ReadAll(limited)
    check(limitErr == error("gzip: compressed byte limit exceeded"), "compressed reader limit")
    drop(limited)
    let consumed, _ = done.receive()
    check(consumed == 20, "at most one extra source byte")
    for limit in Array<int>{-1, gzip.MaxBytes + 1} {
        let invalid, err = gzip.NewReader(feed(data[:], 0, done), 20, limit)
    _ = err
        check(err == error("gzip: invalid byte limit"), "invalid reader limit")
        drop(invalid)
        let consumed, _ = done.receive()
        check(consumed == 0, "invalid constructor does not read")
    }
    println("ok")
}
"#,
        false,
    );
}

#[test]
fn gzip_owned_endpoints_suspend_and_clean_up_on_one_worker() {
    succeeds(
        r#"async func worker(done channel<int>, result channel<bool>, gate channel<bool>) error {
    let blocked = Sink{total: 0, calls: 0, mode: 7, done: done, gate: gate}
    var writer = gzip.NewWriter(blocked, 10, 100)?
    let _, err = writer.Write(Array<byte>{1, 2, 3}[:])
    _ = err
    check(err == nil, "async write")
    check(writer.Close() == nil, "async finish")
    let data = emptyMember()
    var reader = gzip.NewReader(feed(data[:], 5, done), 20, 0)?
    let output, readErr = io.ReadAll(reader)
    _ = readErr
    check(output.len() == 0 && readErr == nil, "async read")
    check(reader.Close() == nil, "async close")
    result.send(true)
    return nil
}
func fail(done channel<int>) error {
    let writer = gzip.NewWriter(sink(0, done), 100, 100)?
    var reader = gzip.NewReader(bytes.NewBufferString("invalid"), 100, 100)?
    let _, err = io.ReadAll(reader)
    _ = err
    check(err != nil, "malformed")
    return error("intentional")
}
func main() {
    let done = channel<int>(30)
    let result = channel<bool>(1)
    let gate = channel<bool>(1)
    go worker(done, result, gate)
    waitBlocked(1)
    gate.send(true)
    let ok, _ = result.receive()
    check(ok, "completed on one worker")
    let _, _ = done.receive()
    let _, _ = done.receive()
    check(fail(done) == error("intentional"), "early return")
    let count, _ = done.receive()
    check(count == 0, "error exit drops writer without finish")
    waitIdle(0)
    println("ok")
}
"#,
        true,
    );
}

#[test]
fn gzip_panic_drops_owned_endpoints_without_leaks() {
    let output = run(
        r#"func main() {
    let done = channel<int>(10)
    let writer = gzip.NewWriter(sink(0, done), 10, 100)?
    let data = emptyMember()
    let reader = gzip.NewReader(feed(data[:], 0, done), 20, 0)?
    panic("gzip cleanup probe")
}
"#,
        false,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("gzip cleanup probe"));
    assert!(!stderr(&output).contains("leak:"), "{}", stderr(&output));
}

#[test]
fn gzip_guide_example_compiles_and_runs() {
    let guide = include_str!("../../docs/gzip.md");
    let source = guide
        .split("```ore\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("gzip-guide");
    build_file(source, &executable).unwrap();
    let output = bounded_process::run(
        Command::new(executable)
            .arg(directory.path().join("hello.gz"))
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "11\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn gzip_unverified_partial_output_is_preserved_with_checksum_truncation_and_limit_errors() {
    succeeds(
        r#"func main() {
    let done = channel<int>(10)
    let valid = Array<byte>{31, 139, 8, 0, 0, 0, 0, 0, 4, 3, 1, 3, 0, 252, 255, 97, 98, 99, 194, 65, 36, 53, 3, 0, 0, 0}
    for mode in Array<int>{0, 1, 2} {
        var data = bytes.Clone(valid[:])
        var limit = 10
        if mode == 0 { data[data.len() - 8] ^= 1 }
        if mode == 1 { let _, _ = data.pop() }
        if mode == 2 { limit = 2 }
        var reader = gzip.NewReader(feed(data[:], 5, done), 100, limit)?
        var buf = Array<byte>{0, 0, 0, 0, 0, 0}
        let count, err = reader.Read(buf[:])
        check(err != nil, "explicit failure")
        var expected = 3
        if mode == 2 { expected = 2 }
        check(count == expected && buf[0] == 97 && buf[1] == 98, "unverified partial bytes")
        if mode == 0 { check(err == error("gzip: checksum mismatch"), "checksum") }
        if mode == 1 { check(err == error("gzip: truncated stream"), "truncation") }
        if mode == 2 { check(err == error("gzip: output byte limit exceeded"), "output bound") }
        check(reader.Close() == err, "close reports terminal failure")
        let _, _ = done.receive()
    }
    println("ok")
}
"#,
        false,
    );
}
