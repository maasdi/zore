use super::*;
use std::io::Read;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const CLIENT: &str = r#"package main
import "std/context"
import "std/errors"
import "std/encoding/json"
import "std/net/http"
import "std/os"
import "std/time"
async func cancelSoon(done channel<bool>) { time.Sleep(30 * time.Millisecond); done.close() }
func send(client http.Client, ctx context.Context, args []string, headers http.Header, payload []byte) (http.ClientResponse, error) {
    if args[3] == "get" { return client.Get(ctx, args[1]) }
    return client.Do(ctx, args[2], args[1], headers, payload)
}
func request() error {
    let args = os.Args()
    var config = http.DefaultClientConfig()
    config.Timeout = 3 * time.Second
    if args[3] == "timeout" { config.Timeout = 300 * time.Millisecond }
    if args[3] == "body" { config.MaxBodyBytes = 3 }
    if args[3] == "headers" { config.MaxHeaderBytes = 160 }
    let client = http.NewClient(config)?
    var headers = http.NewHeader()
    headers.Add("X-Test", "yes")?
    var payload = Array<byte>{0, 255, 65}
    if args[3] == "json" { payload = json.Marshal("Ada")?; headers.Add("Content-Type", "application/json")? }
    var contextBudget = 4 * time.Second
    if args[3] == "context" { contextBudget = 30 * time.Millisecond }
    let timed, stopTimer = context.WithTimeout(context.Background(), contextBudget)
    let done = channel<bool>()
    let ctx, cancel = context.WithCancelOn(timed, done)
    if args[3] == "cancel" { go cancelSoon(done) }
    if args[3] == "precancel" { cancel() }
    let response, err = send(client, ctx, args[:], headers, payload[:])
    cancel()
    stopTimer()
    if err != nil { return err }
    println(response.StatusCode)
    if args[3] == "header" { for value in response.Headers.Values("X-Name") { println(value) } }
    if args[3] == "json" { var decoded = ""; json.Unmarshal(response.Body[:], decoded)?; println(decoded) } else { for b in response.Body { println(int(b)) } }
    return nil
}
func main() { let err = request(); if err != nil { println(errors.Message(err)) } }
"#;

fn collect_bounded(mut child: std::process::Child) -> (Output, bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            return (child.wait_with_output().unwrap(), false);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            return (child.wait_with_output().unwrap(), true);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn finish_bounded(child: std::process::Child) -> Output {
    let (output, timed_out) = collect_bounded(child);
    assert!(
        !timed_out,
        "HTTP client exceeded process deadline: {}",
        stderr(&output)
    );
    output
}

fn client_prints(source: &str, expected: &str) {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("client");
    build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let child = Command::new(executable)
        .env("ZORE_CHECK_LEAKS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = finish_bounded(child);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), expected);
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

fn configure_client_peer(stream: &TcpStream) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))
}

#[test]
fn http_client_peer_uses_blocking_io_after_nonblocking_accept() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    stream.set_nonblocking(true).unwrap();
    configure_client_peer(&stream).unwrap();
    let (started, reading) = std::sync::mpsc::channel();
    let (finished, result) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        started.send(()).unwrap();
        finished.send(stream.read(&mut [0])).unwrap();
    });
    reading.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_millis(30)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    drop(client);
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)),
        Ok(Ok(0))
    ));
    peer.join().unwrap();
}

#[derive(Debug)]
enum ClientPeerOutcome {
    NoConnection,
    ClosedDuringRequest,
    Request(Vec<u8>),
}

fn expected_client_error(mode: &str) -> Option<&str> {
    match mode {
        "cancel" | "precancel" => Some("context canceled\n"),
        "timeout" | "context" => Some("context deadline exceeded\n"),
        _ => None,
    }
}

fn connection_closed(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
    )
}

fn serve_client_peer(
    listener: TcpListener,
    bytes: &[u8],
    mode: &str,
    child_done: std::sync::mpsc::Receiver<()>,
) -> Result<ClientPeerOutcome, String> {
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if child_done.try_recv() != Err(std::sync::mpsc::TryRecvError::Empty) {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            return Ok(ClientPeerOutcome::NoConnection);
                        }
                        Err(error) => return Err(format!("final accept: {error}")),
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(format!("accept: {error}")),
        }
    };
    configure_client_peer(&stream).map_err(|error| format!("configure socket: {error}"))?;
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) => return Ok(ClientPeerOutcome::ClosedDuringRequest),
            Ok(_) => request.push(byte[0]),
            Err(error) if connection_closed(&error) => {
                return Ok(ClientPeerOutcome::ClosedDuringRequest);
            }
            Err(error) => return Err(format!("request headers: {error}")),
        }
        if request.len() >= 4096 {
            return Err("request headers too large".into());
        }
    }
    let expected_payload: &[u8] = match mode {
        "json" => b"\"Ada\"",
        "get" => b"",
        _ => &[0, 255, 65],
    };
    let mut payload = vec![0; expected_payload.len()];
    match stream.read_exact(&mut payload) {
        Ok(()) => {}
        Err(error)
            if error.kind() == std::io::ErrorKind::UnexpectedEof || connection_closed(&error) =>
        {
            return Ok(ClientPeerOutcome::ClosedDuringRequest);
        }
        Err(error) => return Err(format!("request payload: {error}")),
    }
    if payload != expected_payload {
        return Err(format!(
            "request payload: {payload:?}, expected {expected_payload:?}"
        ));
    }
    if mode == "delayed" {
        std::thread::sleep(Duration::from_millis(350));
    }
    if mode == "bulk" {
        let _ = stream.write_all(bytes);
    } else {
        for byte in bytes {
            if stream.write_all(&[*byte]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
    }
    if expected_client_error(mode).is_none() {
        let _ = stream.shutdown(Shutdown::Write);
    }
    match stream.read(&mut byte) {
        Ok(0) => {}
        Err(error) if connection_closed(&error) => {}
        other => return Err(format!("connection close: {other:?}")),
    }
    Ok(ClientPeerOutcome::Request(request))
}

fn client_peer(executable: &Path, response: &[u8], method: &str, mode: &str) -> (String, Vec<u8>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/a%2Fb?q=x%20y", listener.local_addr().unwrap());
    let child = Command::new(executable)
        .args([url.as_str(), method, mode])
        .env("ZORE_CHECK_LEAKS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let bytes = response.to_vec();
    let peer_mode = mode.to_owned();
    let (completed, child_done) = std::sync::mpsc::channel();
    let peer =
        std::thread::spawn(move || serve_client_peer(listener, &bytes, &peer_mode, child_done));
    let (output, timed_out) = collect_bounded(child);
    drop(completed);
    let diagnostic = format!(
        "{method} {mode} response={response:?}: status={} stdout={:?} stderr={:?}",
        output.status,
        stdout(&output),
        stderr(&output)
    );
    let outcome = peer
        .join()
        .unwrap_or_else(|_| panic!("{diagnostic}: peer thread panicked"))
        .unwrap_or_else(|error| panic!("{diagnostic}: {error}"));
    assert!(!timed_out, "{diagnostic}: exceeded process deadline");
    assert!(output.status.success(), "{diagnostic}");
    assert!(output.stderr.is_empty(), "{diagnostic}");
    if let Some(expected) = expected_client_error(mode) {
        assert_eq!(stdout(&output), expected, "{diagnostic}: {outcome:?}");
    }
    if mode == "precancel" {
        assert!(
            matches!(outcome, ClientPeerOutcome::NoConnection),
            "{diagnostic}: pre-canceled request dialed: {outcome:?}"
        );
    }
    let request = match outcome {
        ClientPeerOutcome::Request(request) => request,
        ClientPeerOutcome::NoConnection | ClientPeerOutcome::ClosedDuringRequest
            if expected_client_error(mode).is_some() =>
        {
            Vec::new()
        }
        early => panic!("{diagnostic}: expected complete request, got {early:?}"),
    };
    (stdout(&output), request)
}

#[test]
fn http_client_real_fragmented_binary_framing_and_hostile_peers() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("client");
    build_file(CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let successes: &[(&[u8], &str, &str)] = &[
        (b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n\0\xffA", "GET", "200\n0\n255\n65\n"),
        (b"HTTP/1.1 103 Early Hints\r\nLink: x\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;x=\"ok\"\r\n\0\r\n2\r\n\xffA\r\n0\r\nX-Checksum: yes\r\n\r\n", "POST", "200\n0\n255\n65\n"),
        (b"HTTP/1.1 404 Not Found\r\nContent-Encoding: gzip\r\n\r\n\0\xffA", "GET", "404\n0\n255\n65\n"),
        (b"HTTP/1.1 200 OK\r\nContent-Length: 9999999\r\n\r\n", "HEAD", "200\n"),
        (b"HTTP/1.1 204 No Content\r\n\r\n", "GET", "204\n"),
        (b"HTTP/1.1 304 Not Modified\r\nContent-Length: 9999999\r\n\r\n", "GET", "304\n"),
        (b"HTTP/1.1 205 Reset Content\r\nContent-Length: 0\r\n\r\n", "GET", "205\n"),
        (b"HTTP/1.1 302 Found\r\nLocation: https://elsewhere/\r\nContent-Length: 0\r\n\r\n", "GET", "302\n"),
        (b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\n\0\xffA", "GET", "200\n0\n255\n65\n"),
        (b"HTTP/1.0 200 OK\r\nServer: SimpleHTTP/0.6 Python/3.12.3\r\n\r\n\0\xffA", "GET", "200\n0\n255\n65\n"),
        (b"HTTP/1.1 200\r\nContent-Length: 3\r\n\r\n\0\xffA", "GET", "200\n0\n255\n65\n"),
        (b"HTTP/1.1 404 \r\nContent-Length: 0\r\n\r\n", "GET", "404\n"),
        (b"HTTP/1.1 200 Tr\xc3\xa8s bien\r\nContent-Length: 3\r\n\r\n\0\xffA", "GET", "200\n0\n255\n65\n"),
        (b"HTTP/1.1 200 Caf\xe9\r\nContent-Length: 3\r\n\r\n\0\xffA", "GET", "200\n0\n255\n65\n"),
    ];
    for (response, method, expected) in successes {
        let (output, request) = client_peer(&executable, response, method, "ok");
        assert_eq!(output, *expected, "{method} response={response:?}");
        let text = String::from_utf8(request).unwrap();
        assert!(
            text.starts_with(&format!(
                "{method} /a%2Fb?q=x%20y HTTP/1.1\r\nHost: 127.0.0.1:"
            )),
            "{text}"
        );
        assert!(
            text.contains("Content-Length: 3\r\nConnection: close\r\nX-Test: yes\r\n"),
            "{text}"
        );
    }
    assert_eq!(
        client_peer(
            &executable,
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n\0\xffA",
            "GET",
            "delayed"
        )
        .0,
        "200\n0\n255\n65\n",
        "delayed binary response"
    );
    assert_eq!(
        client_peer(
            &executable,
            b"HTTP/1.1 200 OK\r\nX-Name: Zo\xc3\xab \xe2\x9c\x93\r\nX-Name: Zo\xeb\r\nContent-Length: 0\r\n\r\n",
            "GET",
            "header"
        )
        .0,
        "200\nZo\u{eb} \u{2713}\nZo\u{eb}\n",
        "obs-text header values"
    );
    let (get_output, get_request) = client_peer(
        &executable,
        b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nA",
        "GET",
        "get",
    );
    assert_eq!(get_output, "200\n65\n");
    let get_text = String::from_utf8(get_request).unwrap();
    assert!(
        get_text.starts_with("GET /a%2Fb?q=x%20y HTTP/1.1\r\nHost: 127.0.0.1:")
            && get_text.contains("\r\nContent-Length: 0\r\n"),
        "{get_text}"
    );
    let mut many_chunks = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for _ in 0..7500 {
        many_chunks.extend_from_slice(b"1\r\nA\r\n");
    }
    many_chunks.extend_from_slice(b"0\r\n\r\n");
    assert_eq!(
        client_peer(&executable, &many_chunks, "GET", "bulk").0,
        format!("200\n{}", "65\n".repeat(7500)),
        "tiny chunks exceed the metadata budget only through framing"
    );
    let oversized_extension = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;x={}\r\nA\r\n0\r\n\r\n",
        "y".repeat(40000)
    );
    assert!(
        client_peer(&executable, oversized_extension.as_bytes(), "GET", "bulk")
            .0
            .contains("chunk line too large")
    );
    let padded_size = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{}1\r\nA\r\n0\r\n\r\n",
        "0".repeat(100)
    );
    assert!(
        client_peer(&executable, padded_size.as_bytes(), "GET", "bulk")
            .0
            .contains("chunk line too large")
    );
    let (json_output, json_request) = client_peer(&executable, b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 5\r\n\r\n\"Ada\"", "POST", "json");
    assert_eq!(json_output, "201\nAda\n");
    assert!(
        String::from_utf8(json_request)
            .unwrap()
            .contains("Content-Type: application/json\r\n")
    );
    let failures: &[(&[u8], &str)] = &[
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx",
            "ambiguous framing",
        ),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n",
            "ambiguous framing",
        ),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 1, 1\r\n\r\n",
            "invalid length",
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
            "unsupported transfer encoding",
        ),
        (
            b"HTTP/1.1 101 Switching Protocols\r\n\r\n",
            "protocol upgrade unsupported",
        ),
        (
            b"HTTP/1.1 100 Continue\r\nContent-Length: 0\r\n\r\n",
            "informational response forbids framing",
        ),
        (
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n",
            "204 forbids framing",
        ),
        (
            b"HTTP/1.1 205 Reset Content\r\nContent-Length: 1\r\n\r\nx",
            "205 forbids content",
        ),
        (b"HTTP/2.0 200 OK\r\n\r\n", "invalid status line"),
        (b"HTTP/1.2 200 OK\r\n\r\n", "invalid status line"),
        (b"HTTP/1.1  200 OK\r\n\r\n", "invalid status line"),
        (b"HTTP/1.1 20 OK\r\n\r\n", "invalid status line"),
        (b"HTTP/1.1 2000\r\n\r\n", "invalid status line"),
        (b"HTTP/1.1 2x0 OK\r\n\r\n", "invalid status code"),
        (b"HTTP/1.1 600 OK\r\n\r\n", "invalid status code"),
        (b"HTTP/1.1\r\n\r\n", "invalid status line"),
        (
            b"HTTP/1.0 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            "HTTP/1.0 response forbids Transfer-Encoding",
        ),
        (b"HTTP/1.1 200 OK\n\n", "invalid line byte"),
        (b"HTTP/1.1 200 O\x01K\r\n\r\n", "invalid line byte"),
        (
            b"HTTP/1.1 200 OK\r\nX-Bad: a\x7fb\r\n\r\n",
            "invalid line byte",
        ),
        (
            b"HTTP/1.1 200 OK\r\nX-Bad: a\0b\r\n\r\n",
            "invalid line byte",
        ),
        (
            b"HTTP/1.1 200 OK\r\nCaf\xc3\xa9: x\r\n\r\n",
            "invalid response header \"Caf\u{e9}\"",
        ),
        (
            b"HTTP/1.1 200 OK\r\n Folded: x\r\n\r\n",
            "invalid response header \" Folded\"",
        ),
        (
            b"HTTP/1.1 200 OK\r\nNo colon\r\n\r\n",
            "invalid response header line",
        ),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nx",
            "http.Client: unexpected EOF in body",
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nAB",
            "http.Client: unexpected EOF in body",
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nABCD\r\n",
            "http.Client: unexpected EOF in body",
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nq\r\n",
            "invalid chunk size",
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;x=\"unterminated\r\n",
            "invalid chunk extension",
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;caf\xc3\xa9\r\nA\r\n0\r\n\r\n",
            "invalid chunk extension",
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx!!",
            "expected chunk CRLF",
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 0\r\n\r\n",
            "forbidden trailer",
        ),
    ];
    for (response, expected) in failures {
        let (output, _) = client_peer(&executable, response, "GET", "ok");
        assert!(output.contains(expected), "expected {expected}: {output}");
    }
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nABCD".as_slice(),
        b"HTTP/1.1 200 OK\r\n\r\nABCD",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nABCD\r\n0\r\n\r\n",
    ] {
        assert!(
            client_peer(&executable, response, "GET", "body")
                .0
                .contains("body too large")
        );
    }
    let oversized = format!("HTTP/1.1 200 OK\r\nX-Large: {}\r\n\r\n", "x".repeat(200));
    assert!(
        client_peer(&executable, oversized.as_bytes(), "GET", "headers")
            .0
            .contains("metadata too large")
    );
    for oversized in [
        format!(
            "{}HTTP/1.1 204 No Content\r\n\r\n",
            "HTTP/1.1 103 Hints\r\nX-Hint: x\r\n\r\n".repeat(6)
        ),
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-Trailer: {}\r\n\r\n",
            "x".repeat(200)
        ),
    ] {
        assert!(
            client_peer(&executable, oversized.as_bytes(), "GET", "headers")
                .0
                .contains("metadata too large")
        );
    }
    for (mode, expected) in [
        ("cancel", "context canceled"),
        ("precancel", "context canceled"),
        ("timeout", "context deadline exceeded"),
        ("context", "context deadline exceeded"),
    ] {
        assert_eq!(
            client_peer(
                &executable,
                b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n",
                "GET",
                mode
            )
            .0,
            format!("{expected}\n"),
            "{mode}"
        );
    }
}

#[test]
fn http_client_absolute_urls_and_validation_precede_transport() {
    client_prints(
        r#"package main
import "std/context"
import "std/net/http"
import "std/net/url"
func main() {
    let valid = [string; 9]{"HTTP://Example.com", "https://[::1]:8443/a%2Fb?q=x", "http://127.0.0.1?x=1", "http://[::ffff:192.0.2.1]/", "https://a.example./a//../b", "http://localhost:65535/", "http://my_db:5432/health", "http://host/a?b=c#frag ment", "http://host#top"}
    for text in valid {
        let parsed, err = url.ParseAbsolute(text)
        if err != nil { panic("valid URL") }
        println(parsed.Scheme + " " + parsed.Authority + " " + parsed.Address + " " + parsed.RequestTarget)
    }
    let invalid = [string; 23]{"/relative", "ftp://host/", "httpss://host/", "HTTP:/host/", "http://user@host/", "http://host/a b", "http://host/%zz", "http://host/%a", "http://host/é", "http://host/\\x", "http:///", "http://host:", "http://host:0", "http://host:65536", "http://host:-1", "http://::1/", "http://[1:2:3]/", "http://[::1%25lo]/", "http://[1::2::3]/", "http://999.1.2.3/", "http://01.2.3.4/", "http://-host/", "http://host..test/"}
    for text in invalid { let _, err = url.ParseAbsolute(text); if err == nil { panic("invalid URL") } }
    let client, err = http.NewClient(http.DefaultClientConfig())
    if err != nil { panic("client") }
    let empty = Array<byte>{}
    let _, httpsErr = client.Do(context.Background(), "GET", "https://localhost/", http.NewHeader(), empty[:])
    println(httpsErr != nil)
    let _, getErr = http.Get(context.Background(), "https://localhost/")
    if getErr != error("http.Client: HTTPS requires verified TLS transport") { panic("package Get") }
    let _, methodGetErr = client.Get(context.Background(), "/relative")
    if methodGetErr != error("url: expected absolute http or https URL") { panic("client Get") }
    var limits = http.DefaultClientConfig()
    limits.MaxRequestBodyBytes = 2
    limits.MaxBodyBytes = 1
    let limited, limitsErr = http.NewClient(limits)
    if limitsErr != nil { panic("limits") }
    let three = Array<byte>{1, 2, 3}
    let _, uploadErr = limited.Do(context.Background(), "POST", "http://localhost:1/", http.NewHeader(), three[:])
    if uploadErr != error("http.Client: request body too large") { panic("request body limit") }
    let controls = [string; 9]{"Host", "Content-Length", "Transfer-Encoding", "Connection", "Trailer", "Upgrade", "Expect", "Proxy-Connection", "Bad Name"}
    for name in controls {
        var headers = http.NewHeader()
        let added = headers.Add(name, "x")
        if added != nil { continue }
        let _, failure = client.Do(context.Background(), "GET", "http://localhost:1/", headers, empty[:])
        if failure != error("http.Client: client owns transport header") { panic("transport header") }
    }
    var injected = http.NewHeader()
    println(injected.Add("X-Test", "safe\r\nInjected: yes") != nil)
    for method in [string; 4]{"", "GET\r\nX:", "CONNECT", "TRACE"} {
        let _, failure = client.Do(context.Background(), method, "http://localhost:1/", http.NewHeader(), empty[:])
        if failure == nil { panic("invalid method") }
    }
    var negative = http.DefaultClientConfig()
    negative.Timeout = -1
    let invalidClient, badConfig = http.NewClient(negative)
    println(badConfig != nil)
    let _, emptyClientErr = invalidClient.Do(context.Background(), "GET", "http://localhost/", http.NewHeader(), empty[:])
    if emptyClientErr != error("http.Client: empty client") { panic("empty client") }
}
"#,
        "http Example.com Example.com:80 /\nhttps [::1]:8443 [::1]:8443 /a%2Fb?q=x\nhttp 127.0.0.1 127.0.0.1:80 /?x=1\nhttp [::ffff:192.0.2.1] [::ffff:192.0.2.1]:80 /\nhttps a.example. a.example.:443 /a//../b\nhttp localhost:65535 localhost:65535 /\nhttp my_db:5432 my_db:5432 /health\nhttp host host:80 /a?b=c\nhttp host host:80 /\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn http_client_partial_io_owned_transport_and_error_cleanup() {
    client_prints(FAULTED_CLIENT, "closed exactly once\n");
}

const FAULTED_CLIENT: &str = r#"package main
import "std/context"
import "std/io"
import "std/net/http"
import "std/net/url"
import "std/strings"
import "std/time"
type Conn struct { sent int; cursor int; mode int; drops channel<int>; done channel<bool>; ctx context.Context }
type Cancellation struct { cancel func() }
func (c mut Cancellation) drop() { (c.cancel)() }
func (c mut Conn) drop() { c.drops.send(c.sent) }
func (c mut Conn) SetReadDeadline(deadline int) error {
    if c.mode == 12 { panic("stopped caller set read deadline") }
    return nil
}
func (c mut Conn) SetWriteDeadline(deadline int) error {
    if c.mode == 12 { panic("stopped caller set write deadline") }
    if c.mode == 4 { return error("deadline failure") }
    return nil
}
func (c mut Conn) Write(data []byte) (int, error) {
    if c.mode == 12 { panic("stopped caller wrote") }
    if c.mode == 9 { panic("injected client panic") }
    let head = strings.Bytes("POST / HTTP/1.1\r\nHost: example.test\r\nContent-Length: 3\r\nConnection: close\r\n\r\n")
    let tail = [byte; 3]{0, 255, 65}
    var n = 2
    if n > data.len() { n = data.len() }
    if c.mode == 2 { return 0, nil }
    if c.mode == 3 { return data.len() + 1, nil }
    for var i = 0; i < n; i += 1 {
        let at = c.sent + i
        if at < head.len() { if data[i] != head[at] { panic("header write offset") } } else { if data[i] != tail[at - head.len()] { panic("binary write offset") } }
    }
    c.sent += n
    if c.mode == 1 { return n, error("write failure") }
    if c.sent == 2 { return n, http.ClientPollTimeout }
    return n, nil
}
func (c mut Conn) Read(data mut []byte) (int, error) {
    if c.mode == 12 { panic("stopped caller read") }
    if c.mode == 11 { c.done.close(); let _, _ = c.ctx.Done().receive(); return 0, http.ClientPollTimeout }
    if c.mode == 5 { return 0, nil }
    if c.mode == 6 { return data.len() + 1, nil }
    var response = strings.Bytes("HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n")
    response.push(0); response.push(255); response.push(65)
    if c.cursor == response.len() { return 0, io.EOF }
    data[0] = response[c.cursor]
    c.cursor += 1
    if c.mode == 8 && c.cursor == 1 { return 1, http.ClientPollTimeout }
    if c.cursor == response.len() {
        if c.mode == 7 { return 1, error("read failure") }
        return 1, io.EOF
    }
    return 1, nil
}
type Transport struct { drops channel<int>; mode int; calls channel<bool>; done channel<bool>; ctx context.Context }
func (t Transport) Dial(ctx context.Context, target url.AbsoluteURL) (http.ClientConn, error) {
    if t.mode == 12 { panic("stopped caller dialed") }
    if target.Scheme != "https" || target.Authority != "example.test" || target.Hostname != "example.test" || target.Address != "example.test:443" || target.RequestTarget != "/" || ctx.Err() != nil { panic("dial contract") }
    t.calls.send(true)
    if t.mode == 10 {
        let _, _ = ctx.Done().receive()
        return Conn{sent: 0, cursor: 0, mode: t.mode, drops: t.drops, done: t.done, ctx: t.ctx}, ctx.Err()
    }
    return Conn{sent: 0, cursor: 0, mode: t.mode, drops: t.drops, done: t.done, ctx: t.ctx}, nil
}
func (t mut Transport) drop() { t.calls.send(false) }
func exercise(mode int, drops channel<int>, calls channel<bool>) {
    var settings = http.DefaultClientConfig()
    if mode == 10 { settings.Timeout = 30 * time.Millisecond }
    let done = channel<bool>()
    let ctx, cancel = context.WithCancelOn(context.Background(), done)
    let cancellation = Cancellation{cancel: cancel}
    let client, configErr = http.NewClientWithTransport(settings, Transport{drops: drops, calls: calls, mode: mode, done: done, ctx: ctx})
    if configErr != nil { panic("config") }
    let payload = Array<byte>{0, 255, 65}
    let response, err = client.Do(ctx, "POST", "https://example.test/", http.NewHeader(), payload[:])
    if mode == 11 && err != error("context canceled") { panic("active read cancellation") }
    if mode == 0 || mode == 8 {
        if err != nil { panic("response") }
        if response.StatusCode != 200 || response.Body.len() != 3 || response.Body[0] != 0 || response.Body[1] != 255 || response.Body[2] != 65 { panic("binary read") }
    } else {
        if err == nil || response.StatusCode != 0 || response.Body.len() != 0 { panic("partial response escaped") }
    }
}
func stoppedCalls(ctx context.Context, expected error, drops channel<int>, calls channel<bool>) {
    _ = expected
    let client, clientErr = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{drops: drops, calls: calls, mode: 12, done: channel<bool>(), ctx: ctx})
    if clientErr != nil { panic("stopped caller client") }
    let payload = Array<byte>{0, 255, 65}
    for var mode = 0; mode < 3; mode += 1 {
        var target = "https://example.test/"
        var method = "POST"
        var expectedErr = expected
        if mode == 1 { target = "/relative"; _ = expectedErr; expectedErr = error("url: expected absolute http or https URL") }
        if mode == 2 { method = ""; _ = expectedErr; expectedErr = error("http.Client: unsupported or invalid method") }
        let response, err = client.Do(ctx, method, target, http.NewHeader(), payload[:])
        if err != expectedErr || response.StatusCode != 0 || response.Body.len() != 0 { panic("stopped caller Do") }
        let exchanged, exchangeErr = client.Exchange(ctx, Conn{sent: 0, cursor: 0, mode: 12, drops: drops, done: channel<bool>(), ctx: ctx}, method, target, http.NewHeader(), payload[:])
        if exchangeErr != expectedErr || exchanged.StatusCode != 0 || exchanged.Body.len() != 0 { panic("stopped caller Exchange") }
        let count, _ = drops.receive()
        if count != 0 { panic("stopped caller connection drop") }
    }
    let plain, plainErr = http.NewClient(http.DefaultClientConfig())
    if plainErr != nil { panic("stopped caller plain client") }
    let _, httpsErr = plain.Do(ctx, "GET", "https://example.test/", http.NewHeader(), payload[:])
    if httpsErr != error("http.Client: HTTPS requires verified TLS transport") { panic("stopped caller HTTPS validation") }
}
func verifyStoppedContext(ctx context.Context, expected error) {
    _ = expected
    let drops = channel<int>(10)
    let calls = channel<bool>(20)
    stoppedCalls(ctx, expected, drops, calls)
    let dialed, _ = calls.receive()
    if dialed { panic("stopped caller dial event") }
    select { case calls.receive() { panic("stopped caller extra transport event") }; default {} }
    select { case drops.receive() { panic("stopped caller duplicate connection drop") }; default {} }
}
func main() {
    let drops = channel<int>(10)
    let calls = channel<bool>(20)
    for var mode = 0; mode < 9; mode += 1 {
        exercise(mode, drops, calls)
        let count, _ = drops.receive()
        if mode == 1 { if count != 2 { panic("partial error") } }
        if mode == 2 || mode == 3 || mode == 4 { if count != 0 { panic("invalid write accepted") } }
        let dialed, _ = calls.receive()
        let retained, _ = calls.receive()
        if !dialed || retained { panic("transport drop") }
    }
    exercise(10, drops, calls)
    let failedCount, _ = drops.receive()
    if failedCount != 0 { panic("canceled dial wrote") }
    let failedDial, _ = calls.receive()
    let failedRetained, _ = calls.receive()
    if !failedDial || failedRetained { panic("canceled dial resources") }
    exercise(11, drops, calls)
    let activeCount, _ = drops.receive()
    let expectedRequest = strings.Bytes("POST / HTTP/1.1\r\nHost: example.test\r\nContent-Length: 3\r\nConnection: close\r\n\r\n")
    if activeCount != expectedRequest.len() + 3 { panic("active read request") }
    let activeDial, _ = calls.receive()
    let activeRetained, _ = calls.receive()
    if !activeDial || activeRetained { panic("active read resources") }
    select { case drops.receive() { panic("duplicate active connection drop") }; default {} }
    let client, configErr = http.NewClient(http.DefaultClientConfig())
    if configErr != nil { panic("client") }
    let empty = Array<byte>{}
    let _, invalid = client.Exchange(context.Background(), Conn{sent: 0, cursor: 0, mode: 0, drops: drops, done: channel<bool>(), ctx: context.Background()}, "GET", "/relative", http.NewHeader(), empty[:])
    if invalid == nil { panic("URL") }
    let count, _ = drops.receive()
    if count != 0 { panic("write before validation") }
    var negative = http.DefaultClientConfig()
    negative.Timeout = -1
    let _, configFailure = http.NewClientWithTransport(negative, Transport{drops: drops, calls: calls, mode: 0, done: channel<bool>(), ctx: context.Background()})
    if configFailure == nil { panic("configuration") }
    let dialed, _ = calls.receive()
    if dialed { panic("dialed invalid client") }
    let canceled, cancel = context.WithCancel(context.Background())
    cancel()
    if canceled.Err() != error("context canceled") { panic("caller not canceled") }
    for var attempt = 0; attempt < 8; attempt += 1 { verifyStoppedContext(canceled, error("context canceled")) }
    let expired, stopExpiry = context.WithTimeout(context.Background(), -time.Second)
    let expiryCleanup = Cancellation{cancel: stopExpiry}
    verifyStoppedContext(expired, error("context deadline exceeded"))
    println("closed exactly once")
}
"#;

#[test]
fn http_client_panic_drops_owned_connection_and_transport() {
    let source = FAULTED_CLIENT
        .replace(
            "c.drops.send(c.sent)",
            "c.drops.send(c.sent); println(\"connection closed\")",
        )
        .replace(
            "t.calls.send(false)",
            "t.calls.send(false); println(\"transport closed\")",
        )
        .replace("var mode = 0; mode < 9", "var mode = 9; mode < 10");
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("client");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let child = Command::new(executable)
        .env("ZORE_CHECK_LEAKS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = finish_bounded(child);
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "connection closed\ntransport closed\n");
    assert!(stderr(&output).contains("injected client panic"));
    assert!(!stderr(&output).contains("leak"), "{}", stderr(&output));
}

#[test]
fn http_client_async_calls_suspend_and_cancel_against_real_peer() {
    let source = CLIENT
        .replace("func request()", "async func request()")
        .replace(
            "let err = request()",
            "let task = go request(); let err = task.wait()",
        );
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("client");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    assert_eq!(
        client_peer(
            &executable,
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n\0\xffA",
            "GET",
            "ok"
        )
        .0,
        "200\n0\n255\n65\n"
    );
    assert_eq!(
        client_peer(
            &executable,
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n",
            "GET",
            "cancel"
        )
        .0,
        "context canceled\n"
    );
}
