use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

const STREAM_CLIENT: &str = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net/http"
import "std/os"
import "std/time"
type Sum struct { n int; sum int }
func (s mut Sum) Write(data []byte) (int, error) {
    for b in data { s.sum = (s.sum * 31 + int(b)) % 1000000007 }
    s.n += data.len()
    return data.len(), nil
}
func message(err error) string {
    if err == nil { return "nil" }
    return errors.Message(err)
}
func run() error {
    let args = os.Args()
    let mode = args[2]
    var config = http.DefaultClientConfig()
    config.Timeout = 3 * time.Second
    if mode == "steady" || mode == "headers" { config.Timeout = 300 * time.Millisecond }
    var settings = http.DefaultStreamConfig()
    if mode == "limit" { settings.MaxBodyBytes = 5 }
    if mode == "idle" { settings.IdleTimeout = 300 * time.Millisecond }
    if mode == "total" { settings.Timeout = 300 * time.Millisecond }
    let client = http.NewClient(config)?
    var method = "GET"
    if mode == "head" { method = "HEAD" }
    let done = channel<bool>()
    let ctx, cancel = context.WithCancelOn(context.Background(), done)
    let started = time.Now()
    var stream = client.Open(ctx, method, args[1], http.NewHeader(), Array<byte>{}[:], settings)?
    println(stream.StatusCode())
    var first = [byte; 4]{0, 0, 0, 0}
    if mode == "early" || mode == "cancel" || mode == "close" {
        let n, err = stream.Read(first[:])
        if err != nil { return err }
        println(n)
        if mode == "early" { println(time.Since(started) < time.Second) }
        if mode == "cancel" { cancel() }
        if mode == "close" { return stream.Close() }
    }
    var sink = Sum{n: 0, sum: 0}
    let copied, err = io.Copy(sink, stream)
    println(copied)
    println(sink.sum)
    println(message(err))
    let again, sticky = stream.Read(first[:])
    println(again)
    println(message(sticky))
    let trailers, complete = stream.Trailers()
    println(complete)
    for value in trailers.Values("X-Sum") { println(value) }
    cancel()
    return stream.Close()
}
func main() { let err = run(); if err != nil { println(message(err)) } }
"#;

#[derive(Clone)]
enum Step {
    Send(Vec<u8>),
    Pause(u64),
    ExpectClose,
}

fn checksum(body: &[u8]) -> u64 {
    body.iter().fold(0u64, |sum, byte| {
        (sum * 31 + u64::from(*byte)) % 1_000_000_007
    })
}

fn completed(status: u16, body: &[u8], trailer: Option<&str>) -> String {
    let mut text = format!(
        "{status}\n{}\n{}\nnil\n0\nEOF\ntrue\n",
        body.len(),
        checksum(body)
    );
    if let Some(value) = trailer {
        text.push_str(value);
        text.push('\n');
    }
    text
}

fn failed(status: u16, body: &[u8], error: &str) -> String {
    format!(
        "{status}\n{}\n{}\n{error}\n0\n{error}\nfalse\n",
        body.len(),
        checksum(body)
    )
}

fn serve(listener: TcpListener, steps: Vec<Step>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("no connection".into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(format!("accept: {error}")),
        }
    };
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => request.push(byte[0]),
            other => return Err(format!("request: {other:?}")),
        }
    }
    for step in steps {
        match step {
            Step::Send(bytes) => {
                if stream.write_all(&bytes).is_err() {
                    return Ok(());
                }
            }
            Step::Pause(millis) => std::thread::sleep(Duration::from_millis(millis)),
            Step::ExpectClose => return expect_close(&mut stream),
        }
    }
    Ok(())
}

fn expect_close(stream: &mut TcpStream) -> Result<(), String> {
    let mut byte = [0];
    match stream.read(&mut byte) {
        Ok(0) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ) =>
        {
            Ok(())
        }
        other => Err(format!("client did not close the connection: {other:?}")),
    }
}

fn run_case(executable: &Path, mode: &str, steps: Vec<Step>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let peer = std::thread::spawn(move || serve(listener, steps));
    let output = bounded_process::run(
        Command::new(executable)
            .args([url.as_str(), mode])
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    let served = peer.join().unwrap();
    assert!(
        output.status.success(),
        "{mode}: {} {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(output.stderr.is_empty(), "{mode}: {}", stderr(&output));
    served.unwrap_or_else(|error| panic!("{mode}: {error}: {}", stdout(&output)));
    stdout(&output)
}

fn head(text: &str) -> Step {
    Step::Send(text.as_bytes().to_vec())
}

fn chunk(data: &[u8], extension: &str) -> Step {
    let mut bytes = format!("{:x}{extension}\r\n", data.len()).into_bytes();
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(b"\r\n");
    Step::Send(bytes)
}

fn large_body() -> Vec<u8> {
    (0..3 * 1024 * 1024).map(|i| (i * 7 % 251) as u8).collect()
}

#[test]
fn http_stream_framing_limits_and_errors_against_real_peers() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("stream");
    build_file(STREAM_CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let body = large_body();
    let third = body.len() / 3;

    let fixed = vec![
        head(&format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )),
        Step::Send(body[..third].to_vec()),
        Step::Pause(50),
        Step::Send(body[third..].to_vec()),
    ];
    assert_eq!(
        run_case(&executable, "plain", fixed),
        completed(200, &body, None)
    );

    let chunked = vec![
        head("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"),
        chunk(&body[..third], ";name=\"x\""),
        Step::Pause(50),
        chunk(&body[third..], ""),
        head("0\r\nX-Sum: 42\r\n\r\n"),
    ];
    assert_eq!(
        run_case(&executable, "plain", chunked),
        completed(200, &body, Some("42"))
    );

    let delimited = vec![
        head("HTTP/1.0 200 OK\r\n\r\n"),
        Step::Send(body[..third].to_vec()),
    ];
    assert_eq!(
        run_case(&executable, "plain", delimited),
        completed(200, &body[..third], None)
    );

    for (status, extra) in [("204 No Content", ""), ("304 Not Modified", "")] {
        let steps = vec![head(&format!("HTTP/1.1 {status}\r\n{extra}\r\n"))];
        let code = status[..3].parse().unwrap();
        assert_eq!(
            run_case(&executable, "plain", steps),
            completed(code, b"", None)
        );
    }
    assert_eq!(
        run_case(
            &executable,
            "head",
            vec![head("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n")]
        ),
        completed(200, b"", None)
    );
    assert_eq!(
        run_case(
            &executable,
            "plain",
            vec![head("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")]
        ),
        completed(200, b"", None)
    );

    let truncated = vec![head("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc")];
    assert_eq!(
        run_case(&executable, "plain", truncated),
        failed(200, b"abc", "http.Client: unexpected EOF in body")
    );
    let truncated_chunk = vec![head(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab",
    )];
    assert_eq!(
        run_case(&executable, "plain", truncated_chunk),
        failed(200, b"ab", "http.Client: unexpected EOF in body")
    );
    let missing_terminator = vec![head(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n",
    )];
    assert_eq!(
        run_case(&executable, "plain", missing_terminator),
        failed(200, b"ab", "http.Client: unexpected EOF in body")
    );
    let bad_crlf = vec![head(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nabXY0\r\n\r\n",
    )];
    assert_eq!(
        run_case(&executable, "plain", bad_crlf),
        failed(200, b"ab", "http.Client: expected chunk CRLF")
    );
    let forbidden_trailer = vec![head(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n0\r\nSet-Cookie: a=b\r\n\r\n",
    )];
    assert_eq!(
        run_case(&executable, "plain", forbidden_trailer),
        failed(200, b"ab", "http.Client: forbidden trailer")
    );
    let reset_content = vec![head(
        "HTTP/1.1 205 Reset Content\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n0\r\n\r\n",
    )];
    assert_eq!(
        run_case(&executable, "plain", reset_content),
        failed(205, b"", "http.Client: 205 forbids content")
    );
    assert_eq!(
        run_case(
            &executable,
            "plain",
            vec![head(
                "HTTP/1.1 205 Reset Content\r\nContent-Length: 3\r\n\r\nabc"
            )]
        ),
        "http.Client: 205 forbids content\n"
    );

    for (framing, exact) in [
        ("Content-Length: 5\r\n\r\n12345", true),
        ("Content-Length: 6\r\n\r\n123456", false),
        (
            "Transfer-Encoding: chunked\r\n\r\n5\r\n12345\r\n0\r\n\r\n",
            true,
        ),
        (
            "Transfer-Encoding: chunked\r\n\r\n6\r\n123456\r\n0\r\n\r\n",
            false,
        ),
    ] {
        let steps = vec![head(&format!("HTTP/1.1 200 OK\r\n{framing}"))];
        let expected = match (exact, framing.starts_with("Content")) {
            (true, _) => completed(200, b"12345", None),
            (false, true) => "http.Client: body too large\n".to_owned(),
            (false, false) => failed(200, b"", "http.Client: body too large"),
        };
        assert_eq!(run_case(&executable, "limit", steps), expected, "{framing}");
    }
    assert_eq!(
        run_case(
            &executable,
            "limit",
            vec![head("HTTP/1.0 200 OK\r\n\r\n12345")]
        ),
        completed(200, b"12345", None)
    );
    assert_eq!(
        run_case(
            &executable,
            "limit",
            vec![head("HTTP/1.0 200 OK\r\n\r\n123456")]
        ),
        failed(200, b"12345", "http.Client: body too large")
    );
}

#[test]
fn http_stream_delivers_incrementally_and_times_out_or_cancels_cleanly() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("stream");
    build_file(STREAM_CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));

    let early = vec![
        head("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nabcd"),
        Step::Pause(1500),
        head("efgh"),
    ];
    assert_eq!(
        run_case(&executable, "early", early),
        format!(
            "200\n4\ntrue\n4\n{}\nnil\n0\nEOF\ntrue\n",
            checksum(b"efgh")
        )
    );

    let mut steady = vec![head("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n")];
    for byte in b"0123456789" {
        steady.push(Step::Pause(100));
        steady.push(Step::Send(vec![*byte]));
    }
    assert_eq!(
        run_case(&executable, "steady", steady.clone()),
        completed(200, b"0123456789", None)
    );
    let total = run_case(&executable, "total", steady);
    assert!(
        total.ends_with("context deadline exceeded\n0\ncontext deadline exceeded\nfalse\n"),
        "{total}"
    );

    let idle = vec![
        head("HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nab"),
        Step::Pause(1500),
        head("cd"),
    ];
    assert_eq!(
        run_case(&executable, "idle", idle),
        failed(200, b"ab", "http.Client: body idle timeout")
    );

    let slow_headers = vec![
        Step::Pause(1000),
        head("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"),
    ];
    assert_eq!(
        run_case(&executable, "headers", slow_headers),
        "context deadline exceeded\n"
    );

    let cancel = vec![
        head("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nabcd"),
        Step::ExpectClose,
    ];
    assert_eq!(
        run_case(&executable, "cancel", cancel),
        "200\n4\n0\n0\ncontext canceled\n0\ncontext canceled\nfalse\n"
    );

    let close = vec![
        head("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nabcd\r\n"),
        Step::ExpectClose,
    ];
    assert_eq!(run_case(&executable, "close", close), "200\n4\n");
}

#[test]
fn http_stream_bounds_memory_for_large_downloads() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let source = STREAM_CLIENT.replace(
        "    cancel()\n    return stream.Close()",
        "    cancel()\n    let status = os.ReadFile(\"/proc/self/status\")?\n    println(peak(strings.FromBytes(status[:])?)? < 32768)\n    return stream.Close()",
    ) + r#"
func peak(status string) (int, error) {
    for line in strings.Split(status, "\n") {
        if strings.HasPrefix(line, "VmHWM:") {
            return strconv.Atoi(strings.TrimSpace(strings.TrimSuffix(strings.TrimPrefix(line, "VmHWM:"), "kB")))
        }
    }
    return 0, error("no VmHWM")
}
"#;
    let source = source.replace(
        "import \"std/time\"",
        "import \"std/time\"\nimport \"std/strings\"\nimport \"std/strconv\"",
    );
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("stream");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let block: Vec<u8> = (0..1024 * 1024).map(|i| (i % 253) as u8).collect();
    let mut steps = vec![head(&format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
        block.len() * 64
    ))];
    let mut body = Vec::with_capacity(block.len() * 64);
    for _ in 0..64 {
        steps.push(Step::Send(block.clone()));
        body.extend_from_slice(&block);
    }
    let output = run_case(&executable, "plain", steps);
    assert_eq!(output, completed(200, &body, None) + "true\n");
}

const SCRIPTED_STREAM: &str = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net/http"
import "std/strings"
import "std/time"
type Conn struct { cursor int; ticks int; mode int; drops channel<int> }
func (c mut Conn) drop() { c.drops.send(c.mode) }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) Write(data []byte) (int, error) {
    if c.mode == 9 { panic("injected stream panic") }
    return data.len(), nil
}
func (c mut Conn) Read(data mut []byte) (int, error) {
    let response = strings.Bytes("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nabcdef")
    if c.cursor == response.len() { return 0, io.EOF }
    if c.cursor >= 38 && c.ticks < 150 {
        c.ticks += 1
        return 0, http.ClientPollTimeout
    }
    c.ticks = 0
    data[0] = response[c.cursor]
    c.cursor += 1
    if c.cursor == response.len() {
        if c.mode == 1 { return 1, io.EOF }
        if c.mode == 2 { return 1, error("read failure") }
    }
    return 1, nil
}
type Text struct { data Array<byte> }
func (t mut Text) Write(data []byte) (int, error) {
    for b in data { t.data.push(b) }
    return data.len(), nil
}
func check(ok bool, what string) { if !ok { panic(what) } }
func released(drops channel<int>) bool {
    select {
        case drops.receive() { return true }
        default { return false }
    }
}
func open(mode int, drops channel<int>) (http.ResponseStream, error) {
    let client = http.NewClient(http.DefaultClientConfig())?
    let empty = Array<byte>{}
    return client.OpenExchange(context.Background(), Conn{cursor: 0, ticks: 0, mode: mode, drops: drops}, "GET", "https://example.test/", http.NewHeader(), empty[:], http.DefaultStreamConfig())
}
func exercise(mode int, drops channel<int>) error {
    var stream = open(mode, drops)?
    check(stream.StatusCode() == 200, "status")
    if mode == 3 {
        var one = [byte; 1]{0}
        let n = stream.Read(one[:])?
        check(n == 1 && one[0] == 97, "first byte")
        check(!released(drops), "released before close")
        stream.Close()?
        check(released(drops), "close released")
        return nil
    }
    if mode == 4 {
        var one = [byte; 1]{0}
        let n = stream.Read(one[:])?
        check(n == 1, "first byte before drop")
        return nil
    }
    var text = Text{data: Array<byte>{}}
    let copied, err = io.Copy(text, stream)
    let readFailure = err == error("read failure")
    let failed = err != nil
    check(copied == 6 && strings.FromBytes(text.data[:])? == "abcdef", "body")
    check(released(drops), "completion released the connection")
    if mode == 2 {
        check(readFailure, "terminal error after final bytes")
        var buf = [byte; 2]{0, 0}
        let n, again = stream.Read(buf[:])
        let sticky = again == error("read failure")
        check(n == 0 && sticky, "sticky failure")
        return stream.Close()
    }
    check(!failed, "clean completion")
    let trailers, complete = stream.Trailers()
    check(complete && trailers.Values("X-Sum").len() == 0, "complete without trailers")
    return stream.Close()
}
func rejected() error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let empty = Array<byte>{}
    let negative = http.StreamConfig{MaxBodyBytes: -1, Timeout: 0, IdleTimeout: 0}
    let _, configErr = client.Open(context.Background(), "GET", "http://example.test/", http.NewHeader(), empty[:], negative)
    check(configErr == error("http.Client: negative stream configuration"), "negative config")
    let _, tlsErr = client.Open(context.Background(), "GET", "https://example.test/", http.NewHeader(), empty[:], http.DefaultStreamConfig())
    check(tlsErr == error("http.Client: HTTPS requires verified TLS transport"), "https without transport")
    let ctx, cancel = context.WithCancel(context.Background())
    cancel()
    let _, cancelErr = client.Open(ctx, "GET", "http://127.0.0.1:9/", http.NewHeader(), empty[:], http.DefaultStreamConfig())
    check(cancelErr == error("context canceled"), "pre-canceled")
    return nil
}
func main() {
    let drops = channel<int>(4)
    for var mode = 0; mode < 5; mode += 1 {
        let err = exercise(mode, drops)
        if err != nil { panic(errors.Message(err)) }
        if mode == 4 { check(released(drops), "drop released") }
        check(!released(drops), "closed exactly once")
    }
    let err = rejected()
    if err != nil { panic(errors.Message(err)) }
    println("ok")
}
"#;

#[test]
fn http_stream_hides_poll_ticks_and_releases_owned_connections_once() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("scripted");
    build_file(SCRIPTED_STREAM, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn http_stream_panic_drops_owned_connection() {
    let source = SCRIPTED_STREAM
        .replace(
            "func (c mut Conn) drop() { c.drops.send(c.mode) }",
            "func (c mut Conn) drop() { c.drops.send(c.mode); println(\"connection closed\") }",
        )
        .replace("var mode = 0; mode < 5", "var mode = 9; mode < 10");
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("scripted");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "connection closed\n");
    assert!(
        stderr(&output).contains("injected stream panic"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("leak"), "{}", stderr(&output));
}

#[test]
fn http_stream_plain_and_async_reads_progress_on_one_worker() {
    let source = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net"
import "std/net/http"
import "std/strings"
import "std/time"
type Count struct { n int }
func (c mut Count) Write(data []byte) (int, error) { c.n += data.len(); return data.len(), nil }
async func serve(listener own net.Listener) error {
    let conn = listener.Accept()?
    var seen = ""
    var one = Array<byte>{0}
    for !strings.HasSuffix(seen, "\r\n\r\n") {
        let n = conn.Read(one[:])?
        if n == 1 { seen += strings.FromBytes(one[:])? }
    }
    conn.Write(strings.Bytes("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n")[:])?
    time.Sleep(50 * time.Millisecond)
    conn.Write(strings.Bytes("3\r\ndef\r\n0\r\n\r\n")[:])?
    return nil
}
func fetch(target string) error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let empty = Array<byte>{}
    var stream = client.Open(context.Background(), "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig())?
    var sink = Count{n: 0}
    let copied = io.Copy(sink, stream)?
    if copied != 6 { return error("short stream") }
    return stream.Close()
}
async func fetchAsync(target string) error {
    return fetch(target)
}
func run() error {
    waitIdle(0)
    for var round = 0; round < 2; round += 1 {
        let listener = net.Listen("tcp", "127.0.0.1:0")?
        let target = "http://" + listener.Addr() + "/"
        let server = go serve(listener)
        if round == 0 {
            fetch(target)?
        } else {
            let client = go fetchAsync(target)
            client.wait()?
        }
        server.wait()?
    }
    return nil
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"#;
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("one_worker");
    controlled_runtime::build_with_workers(source, &executable, 1);
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn http_download_guide_example_saves_the_file() {
    let guide = include_str!("../../docs/http-downloads.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/report.csv", listener.local_addr().unwrap());
    let source = blocks[0].replace("http://localhost:8080/report.csv", &url);
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("guide");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let body = b"name,score\nAda,7\n";
    let peer = std::thread::spawn(move || {
        serve(
            listener,
            vec![
                head("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"),
                chunk(&body[..5], ""),
                chunk(&body[5..], ""),
                head("0\r\n\r\n"),
            ],
        )
    });
    let output = bounded_process::run(
        Command::new(&executable)
            .current_dir(dir.path())
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    peer.join().unwrap().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(std::fs::read(dir.path().join("report.csv")).unwrap(), body);
    assert!(!dir.path().join("report.csv.part").exists());
}

#[test]
fn http_stream_failure_after_trailers_reports_no_trailers() {
    let source = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net/http"
import "std/strings"
import "std/time"
type Conn struct { cursor int }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) Write(data []byte) (int, error) { return data.len(), nil }
func (c mut Conn) Read(data mut []byte) (int, error) {
    let response = strings.Bytes("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n0\r\nX-Sum: 7\r\n\r\n")
    if c.cursor == response.len() { return 0, io.EOF }
    var n = 0
    for n < data.len() && c.cursor < response.len() {
        data[n] = response[c.cursor]
        c.cursor += 1
        n += 1
    }
    if c.cursor == response.len() { return n, error("connection reset") }
    return n, nil
}
type Discard struct {}
func (d mut Discard) Write(data []byte) (int, error) { return data.len(), nil }
func run() error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let empty = Array<byte>{}
    var stream = client.OpenExchange(context.Background(), Conn{cursor: 0}, "GET", "https://example.test/", http.NewHeader(), empty[:], http.DefaultStreamConfig())?
    var sink = Discard{}
    let copied, err = io.Copy(sink, stream)
    println(copied)
    println(errors.Message(err))
    let trailers, complete = stream.Trailers()
    println(complete)
    println(trailers.Values("X-Sum").len())
    return stream.Close()
}
func main() { let err = run(); if err != nil { println(errors.Message(err)) } }
"#;
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("late_failure");
    build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "2\nconnection reset\nfalse\n0\n");
}
