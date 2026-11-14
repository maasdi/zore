use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const UPLOAD_CLIENT: &str = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net/http"
import "std/os"
import "std/strconv"
import "std/strings"
import "std/time"
type Source struct { at int; size int; step int; mode string }
func (s mut Source) drop() { println("consumed " + strconv.Itoa(s.at)) }
func (s mut Source) Read(buf mut []byte) (int, error) {
    if s.mode == "zero" { panic("zero-length body was read") }
    if s.mode == "panic" { panic("injected source panic") }
    if s.mode == "stall" { return 0, nil }
    if s.mode == "over" { return buf.len() + 1, nil }
    if s.mode == "error" && s.at == 3 { return 0, error("source failure") }
    if s.at == s.size { return 0, io.EOF }
    var n = buf.len()
    if n > s.step { n = s.step }
    if n > s.size - s.at { n = s.size - s.at }
    for var i = 0; i < n; i += 1 { buf[i] = byte((s.at + i) % 251) }
    s.at += n
    if s.mode == "eofwith" && s.at == s.size { return n, io.EOF }
    return n, nil
}
func peak() (int, error) {
    let status = os.ReadFile("/proc/self/status")?
    for line in strings.Split(strings.FromBytes(status[:])?, "\n") {
        if strings.HasPrefix(line, "VmHWM:") {
            let digits = strings.TrimSpace(strings.TrimSuffix(strings.TrimPrefix(line, "VmHWM:"), "kB"))
            let value, err = strconv.Atoi(digits)
            return value, err
        }
    }
    return 0, error("no VmHWM")
}
func run() error {
    let args = os.Args()
    let mode = args[2]
    var config = http.DefaultClientConfig()
    config.Timeout = 3 * time.Second
    var settings = http.DefaultUploadConfig()
    var size = 100
    var length = 100
    var step = 3
    if mode == "zero" { size = 0; length = 0 }
    if mode == "short" { size = 5; length = 10 }
    if mode == "long" { size = 20; length = 10 }
    if mode == "limit" { settings.MaxBodyBytes = 100 }
    if mode == "overlimit" { settings.MaxBodyBytes = 99 }
    if mode == "negative" { length = -1 }
    if mode == "idle" || mode == "big" { size = 67108864; length = size; step = 65536 }
    if mode == "idle" { settings.IdleTimeout = 300 * time.Millisecond }
    if mode == "total" { settings.Timeout = 300 * time.Millisecond }
    let client = http.NewClient(config)?
    let ctx, cancel = context.WithCancel(context.Background())
    if mode == "precancel" { cancel() }
    let body = http.NewRequestBody(Source{at: 0, size: size, step: step, mode: mode}, length)?
    var headers = http.NewHeader()
    headers.Set("Content-Type", "application/octet-stream")?
    let response = client.Upload(ctx, "PUT", args[1], headers, body, settings)?
    cancel()
    println(response.StatusCode)
    println(strings.FromBytes(response.Body[:])?)
    if mode == "big" { println(peak()? < 32768) }
    return nil
}
func main() { let err = run(); if err != nil { println(errors.Message(err)) } }
"#;

#[derive(Clone, Copy, PartialEq)]
enum Peer {
    Accept,
    Reject,
    Stall,
    SlowReply,
    NoConnection,
}

fn checksum(length: usize) -> u64 {
    (0..length).fold(0u64, |sum, i| (sum * 31 + (i % 251) as u64) % 1_000_000_007)
}

fn serve(listener: TcpListener, peer: Peer) -> Result<Option<Vec<u8>>, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if peer == Peer::NoConnection && Instant::now() > deadline - Duration::from_secs(4)
                {
                    return Ok(None);
                }
                if Instant::now() > deadline {
                    return Err("no connection".into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(format!("accept: {error}")),
        }
    };
    if peer == Peer::NoConnection {
        return Err("client connected although it should have failed first".into());
    }
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            other => return Err(format!("request head: {other:?}")),
        }
    }
    let text = String::from_utf8_lossy(&head).to_string();
    let length: usize = text
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .ok_or("missing Content-Length")?
        .trim()
        .parse()
        .map_err(|_| "bad Content-Length")?;
    if !text.starts_with("PUT /upload HTTP/1.1\r\n")
        || !text.contains("Content-Type: application/octet-stream\r\n")
    {
        return Err(format!("unexpected request head: {text:?}"));
    }
    if peer == Peer::Stall {
        std::thread::sleep(Duration::from_secs(2));
        return Ok(Some(head));
    }
    if peer == Peer::Reject {
        let _ =
            stream.write_all(b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 4\r\n\r\nbig!");
    }
    let mut body = Vec::new();
    let mut chunk = vec![0; 1 << 16];
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    if peer == Peer::Reject {
        return Ok(Some(body));
    }
    if body.len() < length {
        return Ok(Some(body));
    }
    let sum = body.iter().fold(0u64, |sum, byte| {
        (sum * 31 + u64::from(*byte)) % 1_000_000_007
    });
    let reply = format!("{}:{sum}", body.len());
    if peer == Peer::SlowReply {
        std::thread::sleep(Duration::from_secs(1));
    }
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{reply}",
            reply.len()
        )
        .as_bytes(),
    );
    Ok(Some(body))
}

fn run_case(executable: &Path, mode: &str, peer: Peer) -> (String, Option<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/upload", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || serve(listener, peer));
    let output = bounded_process::run(
        Command::new(executable)
            .args([url.as_str(), mode])
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    let received = server
        .join()
        .unwrap()
        .unwrap_or_else(|error| panic!("{mode}: {error}: {}", stdout(&output)));
    assert!(
        output.status.success(),
        "{mode}: {} {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(output.stderr.is_empty(), "{mode}: {}", stderr(&output));
    (stdout(&output), received)
}

fn uploaded(consumed: usize, length: usize) -> String {
    format!("consumed {consumed}\n200\n{length}:{}\n", checksum(length))
}

#[test]
fn http_upload_sends_exact_declared_bytes_and_reports_source_failures() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("upload");
    build_file(UPLOAD_CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));

    for mode in ["plain", "eofwith", "limit"] {
        let (output, received) = run_case(&executable, mode, Peer::Accept);
        assert_eq!(output, uploaded(100, 100), "{mode}");
        assert_eq!(received.unwrap().len(), 100, "{mode}");
    }
    let (output, received) = run_case(&executable, "zero", Peer::Accept);
    assert_eq!(output, uploaded(0, 0));
    assert!(received.unwrap().is_empty());

    let (output, received) = run_case(&executable, "long", Peer::Accept);
    assert_eq!(output, uploaded(10, 10));
    assert_eq!(received.unwrap().len(), 10);

    for (mode, error, sent) in [
        (
            "short",
            "http.Client: request body shorter than declared length",
            5,
        ),
        ("error", "source failure", 3),
        ("stall", "http.Client: request body made no progress", 0),
        ("over", "http.Client: invalid read count", 0),
    ] {
        let (output, received) = run_case(&executable, mode, Peer::Accept);
        let consumed = if mode == "short" { 5 } else { sent };
        assert_eq!(output, format!("consumed {consumed}\n{error}\n"), "{mode}");
        assert_eq!(received.unwrap().len(), sent, "{mode}");
    }

    for (mode, error) in [
        ("overlimit", "http.Client: request body too large"),
        ("negative", "http.NewRequestBody: negative length"),
        ("precancel", "context canceled"),
    ] {
        let (output, received) = run_case(&executable, mode, Peer::NoConnection);
        assert_eq!(output, format!("consumed 0\n{error}\n"), "{mode}");
        assert!(received.is_none(), "{mode}");
    }

    let (output, _) = run_case(&executable, "plain", Peer::Reject);
    assert_eq!(output, "consumed 100\n413\nbig!\n");
}

#[test]
fn http_upload_idle_and_total_deadlines_bound_the_whole_exchange() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("upload");
    build_file(UPLOAD_CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));

    let started = Instant::now();
    let (output, _) = run_case(&executable, "idle", Peer::Stall);
    let consumed = output
        .strip_prefix("consumed ")
        .and_then(|rest| rest.split('\n').next())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap();
    assert!(consumed > 0 && consumed < 67108864, "{output}");
    assert!(
        output.ends_with("\nhttp.Client: upload idle timeout\n"),
        "{output}"
    );
    assert!(started.elapsed() < Duration::from_secs(10));

    let (output, _) = run_case(&executable, "total", Peer::SlowReply);
    assert_eq!(output, "consumed 100\ncontext deadline exceeded\n");
}

#[test]
fn http_upload_streams_large_sources_with_bounded_memory() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("upload");
    build_file(UPLOAD_CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let (output, received) = run_case(&executable, "big", Peer::Accept);
    assert_eq!(output, uploaded(67108864, 67108864) + "true\n");
    assert_eq!(received.unwrap().len(), 67108864);
}

#[test]
fn http_upload_panic_in_source_drops_body_and_connection() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("upload");
    build_file(UPLOAD_CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/upload", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || serve(listener, Peer::Accept));
    let output = bounded_process::run(
        Command::new(&executable)
            .args([url.as_str(), "panic"])
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    let received = server.join().unwrap().unwrap();
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "consumed 0\n");
    assert!(
        stderr(&output).contains("injected source panic"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("leak"), "{}", stderr(&output));
    assert!(received.unwrap().is_empty());
}

#[test]
fn http_upload_plain_and_async_progress_on_one_worker() {
    let source = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net"
import "std/net/http"
import "std/strings"
type Source struct { at int }
func (s mut Source) Read(buf mut []byte) (int, error) {
    if s.at == 6 { return 0, io.EOF }
    buf[0] = byte(97 + s.at)
    s.at += 1
    return 1, nil
}
type Conn struct { closed channel<bool> }
func (c mut Conn) drop() { c.closed.send(true) }
func (c mut Conn) Read(buf mut []byte) (int, error) { return 0, io.EOF }
func (c mut Conn) Write(data []byte) (int, error) { return data.len(), nil }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
async func serve(listener own net.Listener) error {
    let conn = listener.Accept()?
    var seen = ""
    var one = Array<byte>{0}
    for !strings.HasSuffix(seen, "\r\n\r\nabcdef") {
        let n = conn.Read(one[:])?
        if n == 1 { seen += strings.FromBytes(one[:])? }
    }
    conn.Write(strings.Bytes("HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nok")[:])?
    return nil
}
func send(target string) error {
    let client = http.NewClient(http.DefaultClientConfig())?
    let body = http.NewRequestBody(Source{at: 0}, 6)?
    let response = client.Upload(context.Background(), "POST", target, http.NewHeader(), body, http.DefaultUploadConfig())?
    if response.StatusCode != 201 || strings.FromBytes(response.Body[:])? != "ok" { return error("bad upload response") }
    return nil
}
async func sendAsync(target string) error {
    return send(target)
}
func invalidExchange() error {
    let closed = channel<bool>(1)
    let client = http.NewClient(http.DefaultClientConfig())?
    let body = http.NewRequestBody(Source{at: 0}, 6)?
    let negative = http.UploadConfig{MaxBodyBytes: -1, Timeout: 0, IdleTimeout: 0}
    let _, err = client.UploadExchange(context.Background(), Conn{closed: closed}, "POST", "https://example.test/", http.NewHeader(), body, negative)
    if err != error("http.Client: negative upload configuration") { return error("negative configuration accepted") }
    select {
        case closed.receive() {}
        default { return error("connection kept after validation failure") }
    }
    return nil
}
func run() error {
    waitIdle(0)
    for var round = 0; round < 2; round += 1 {
        let listener = net.Listen("tcp", "127.0.0.1:0")?
        let target = "http://" + listener.Addr() + "/"
        let server = go serve(listener)
        if round == 0 {
            send(target)?
        } else {
            let client = go sendAsync(target)
            client.wait()?
        }
        server.wait()?
    }
    return invalidExchange()
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"#
    .replace("import \"std/strings\"", "import \"std/strings\"\nimport \"std/time\"");
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("one_worker");
    controlled_runtime::build_with_workers(&source, &executable, 1);
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
fn http_upload_guide_example_sends_the_file() {
    let guide = include_str!("../../docs/http-uploads.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let source = blocks[0].replace(
        "http://localhost:8080/reports/today.csv",
        &format!("http://{address}/reports/today.csv"),
    );
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("guide");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let content = b"name,score\nAda,7\n".to_vec();
    std::fs::write(dir.path().join("report.csv"), &content).unwrap();
    let expected = content.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.ends_with(&expected) {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0, "connection closed early");
            request.extend_from_slice(&chunk[..n]);
        }
        let text = String::from_utf8_lossy(&request).to_string();
        assert!(
            text.starts_with("PUT /reports/today.csv HTTP/1.1\r\n"),
            "{text}"
        );
        assert!(text.contains("\r\nContent-Length: 17\r\n"), "{text}");
        assert!(text.contains("\r\nContent-Type: text/csv\r\n"), "{text}");
        stream
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });
    let output = bounded_process::run(
        Command::new(&executable)
            .current_dir(dir.path())
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "201\n");
}
