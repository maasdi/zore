use super::*;
use std::io::{BufRead, BufReader, Read};
use std::net::{Shutdown, TcpStream};
use std::process::Child;
use std::time::Duration;

const SERVER: &str = r#"package main
import "std/net"
import "std/net/http"
import "std/context"
import "std/io"
import "std/strings"
import "std/time"
type Factory struct { stop channel<bool> }
type Handler struct { stop channel<bool> }
func (f Factory) NewHandler() (http.Handler, error) { return Handler{stop: f.stop}, nil }
func (h Handler) Handle(w mut http.ResponseWriter, r mut http.Request) error {
    let path = r.Path()
    if path == "/stop" { h.stop.send(true); return w.WriteHeader(204) }
    if path == "/reject" {
        w.WriteHeader(403)?
        var data = Array<byte>{0}
        let _, err = r.Read(data[:])
        if err != error("http.Request.Read: response already started") { panic("read after commitment") }
        return nil
    }
    if path == "/reset" {
        w.WriteHeader(205)?
        let _, err = w.Write(strings.Bytes("forbidden")[:])
        if err == nil { panic("205 content") }
        return nil
    }
    if path == "/not-modified" { w.SetHeader("Content-Length", "42")?; return w.WriteHeader(304) }
    if path == "/head" { w.SetHeader("Content-Length", "42")?; return w.WriteHeader(200) }
    if path == "/error" { return error("private detail") }
    if path == "/panic" { panic("handler failure") }
    if path == "/unread" { return w.WriteHeader(200) }
    if path == "/overrun" {
        w.SetHeader("Content-Length", "1")?
        let _, err = w.Write(strings.Bytes("too long")[:])
        return err
    }
    if path == "/underrun" { w.SetHeader("Content-Length", "10")?; return w.WriteHeader(200) }
    if strings.HasPrefix(path, "/users/") {
        let id, _ = r.PathParam("id")
        let _, err = w.Write(strings.Bytes(id)[:])
        return err
    }
    var empty = Array<byte>{}
    let _, emptyErr = r.Read(empty[:])
    if emptyErr != nil { return emptyErr }
    let body = io.ReadAll(r)?
    let _, err = w.Write(body[:])
    return err
}
func main() {
    let stop = channel<bool>(1)
    var b = http.NewRouterBuilder()
    let paths = [string; 14]{"POST /echo", "POST /reject", "GET /reset", "GET /not-modified", "GET /head", "GET /error", "GET /panic", "POST /unread", "GET /overrun", "GET /underrun", "GET /users/{id}", "GET /users/me", "GET /stop", "GET /echo"}
    for path in paths { let err = b.Handle(path, Factory{stop: stop}); if err != nil { panic("route") } }
    let router, buildErr = b.Build()
    if buildErr != nil { panic("build") }
    var config = http.DefaultConfig()
    config.MaxBodyBytes = 32
    config.MaxHeaderBytes = 512
    config.ReadHeaderTimeout = 300 * time.Millisecond
    config.ReadBodyTimeout = 300 * time.Millisecond
    config.WriteTimeout = 2 * time.Second
    config.IdleTimeout = 300 * time.Millisecond
    config.AcceptPollInterval = 10 * time.Millisecond
    let server, configErr = http.NewServer(router, config)
    if configErr != nil { panic("config") }
    let listener, listenErr = net.Listen("tcp", "127.0.0.1:0")
    if listenErr != nil { panic("listen") }
    println(listener.Addr())
    let serving = go server.Serve(listener)
    let _, _ = stop.receive()
    let ctx, cancel = context.WithTimeout(context.Background(), 5 * time.Second)
    let err = server.Shutdown(ctx)
    cancel()
    if err != nil { panic("shutdown") }
    let again = server.Shutdown(context.Background())
    if again != nil { panic("second shutdown") }
    let serveErr = serving.wait()
    if serveErr != nil { panic("serve") }
}
"#;

struct ServerProcess {
    child: Child,
    address: String,
    _dir: TempDir,
}

impl ServerProcess {
    fn start(source: &str) -> Self {
        let dir = TempDir::new().unwrap();
        let executable = dir.path().join("server");
        build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
        let mut child = Command::new(executable)
            .env("ZORE_CHECK_LEAKS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut address)
            .unwrap();
        assert!(address.contains(':'), "server did not announce its port");
        Self {
            child,
            address: address.trim().into(),
            _dir: dir,
        }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(&self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream
    }

    fn exchange(&self, request: &str) -> String {
        let mut stream = self.connect();
        stream.write_all(request.as_bytes()).unwrap();
        read_closed(&mut stream)
    }

    fn stop(self, expected_error: &str) {
        let response =
            self.exchange("GET /stop HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n");
        assert!(response.starts_with("HTTP/1.1 204"), "{response}");
        self.finish(expected_error);
    }

    fn finish(mut self, expected_error: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "{status}");
                break;
            }
            assert!(std::time::Instant::now() < deadline, "shutdown hung");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut errors = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut errors)
            .unwrap();
        if expected_error.is_empty() {
            assert!(errors.is_empty(), "{errors}");
        } else {
            assert!(errors.contains(expected_error), "{errors}");
        }
        assert!(!errors.contains("leak"), "{errors}");
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_closed(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    match stream.read_to_end(&mut bytes) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::ConnectionReset => {}
        Err(err) => panic!(
            "reading response: {err}; {}",
            String::from_utf8_lossy(&bytes)
        ),
    }
    String::from_utf8(bytes).unwrap()
}

fn status(response: &str, code: u16) {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {code} ")),
        "{response:?}"
    );
}

#[test]
fn http_framing_routing_and_shutdown_on_loopback() {
    let server = ServerProcess::start(SERVER);
    let echo = server.exchange(
        "POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    );
    status(&echo, 200);
    assert!(
        echo.contains("Date: ") && echo.ends_with("5\r\nhello\r\n0\r\n\r\n"),
        "{echo}"
    );
    let chunked = server.exchange("POST /echo HTTP/1.1\r\nHost: test\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2;name=\"a;b\"\r\nhe\r\n3\r\nllo\r\n0\r\n\r\n");
    status(&chunked, 200);
    assert!(chunked.ends_with("5\r\nhello\r\n0\r\n\r\n"), "{chunked}");
    let cases = [
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\na",
            400,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n",
            400,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: +1\r\n\r\n",
            400,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 9223372036854775808\r\n\r\n",
            400,
        ),
        ("GET /echo HTTP/1.1\r\nHost: x\r\nHost: y\r\n\r\n", 400),
        ("GET /echo HTTP/1.1\r\n\r\n", 400),
        ("GET /echo HTTP/1.1\r\nHost : x\r\n\r\n", 400),
        ("GET /echo HTTP/1.1\r\nHost: x:bad\r\n\r\n", 400),
        ("GET /echo HTTP/1.1\r\nHost: [::1\r\n\r\n", 400),
        ("GET /echo HTTP/1.0\r\nHost: x\r\n\r\n", 505),
        ("GET /%2f HTTP/1.1\r\nHost: x\r\n\r\n", 400),
        ("GET /../echo HTTP/1.1\r\nHost: x\r\n\r\n", 400),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 33\r\nExpect: 100-continue\r\n\r\n",
            413,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nExpect: other\r\n\r\n",
            417,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: gzip\r\n\r\n",
            400,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n21\r\n",
            413,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\nQ\r\n",
            400,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nTrailer: no\r\n\r\n",
            400,
        ),
        (
            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 1\r\n\r\n",
            408,
        ),
    ];
    for (request, expected) in cases {
        status(&server.exchange(request), expected);
    }
    let oversized = format!(
        "GET /echo HTTP/1.1\r\nHost: x\r\nX: {}\r\n\r\n",
        "a".repeat(512)
    );
    status(&server.exchange(&oversized), 431);
    let mut truncated = server.connect();
    truncated
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\na")
        .unwrap();
    truncated.shutdown(Shutdown::Write).unwrap();
    status(&read_closed(&mut truncated), 400);
    for (path, expected) in [
        ("/missing", 404),
        ("/reset", 205),
        ("/not-modified", 304),
        ("/error", 500),
    ] {
        let response = server.exchange(&format!(
            "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
        ));
        status(&response, expected);
        assert!(!response.contains("private detail") && !response.contains("forbidden"));
        if expected == 205 {
            assert!(response.contains("Content-Length: 0\r\n"));
        }
        if expected == 304 {
            assert!(response.contains("Content-Length: 42\r\n"));
        }
        if expected == 205 || expected == 304 {
            assert!(response.ends_with("\r\n\r\n") && !response.contains("chunked"));
        }
    }
    let head = server.exchange("HEAD /head HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    status(&head, 200);
    assert!(
        head.contains("Content-Length: 42")
            && head.ends_with("\r\n\r\n")
            && !head.contains("chunked")
    );
    let options = server.exchange("OPTIONS /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    status(&options, 204);
    assert!(
        options.contains("Allow: GET, HEAD, OPTIONS, POST\r\n"),
        "{options}"
    );
    status(
        &server.exchange("PUT /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"),
        405,
    );
    let parameter =
        server.exchange("GET /users/123 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert!(parameter.contains("\r\n123\r\n"), "{parameter}");
    let literal = server.exchange("GET /users/me HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert!(
        !literal.contains("\r\nme\r\n"),
        "literal route lost precedence: {literal}"
    );
    let reject = server.exchange(
        "POST /reject HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\nExpect: 100-continue\r\n\r\n",
    );
    status(&reject, 403);
    assert!(!reject.contains("100 Continue"));
    let mut expect = server.connect();
    expect.write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n").unwrap();
    let mut interim = [0; 25];
    expect.read_exact(&mut interim).unwrap();
    assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
    expect.write_all(b"abc").unwrap();
    let final_response = read_closed(&mut expect);
    status(&final_response, 200);
    assert!(!final_response.contains("100 Continue"));
    let pipelined = server.exchange("GET /echo HTTP/1.1\r\nHost: x\r\n\r\nGET /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert_eq!(pipelined.matches("HTTP/1.1 200").count(), 2, "{pipelined}");
    let unread = server.exchange("POST /unread HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabcGET /echo HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(unread.matches("HTTP/1.1").count(), 1, "{unread}");
    for path in ["/overrun", "/underrun"] {
        let response = server.exchange(&format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n"));
        status(&response, 200);
        assert!(!response.contains("500") && !response.contains("too long"));
    }
    let mut silent = server.connect();
    assert!(read_closed(&mut silent).is_empty());
    let mut idle = server.connect();
    idle.write_all(b"GET /echo HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    status(&read_closed(&mut idle), 200);
    let mut shutdown_idle = server.connect();
    server.stop("");
    assert!(read_closed(&mut shutdown_idle).is_empty());
}

#[test]
fn http_url_headers_dates_and_ambiguous_routes() {
    prints(
        r#"package main
import "std/net/url"
import "std/net/http"
import "std/time"
type F struct { n int }
type H struct { n int }
func (f F) NewHandler() (http.Handler, error) { return H{n: f.n}, nil }
func (h H) Handle(w mut http.ResponseWriter, r mut http.Request) error { return nil }
func failZero() error { return error("zero") }
func zeroValues() (http.Request, http.ResponseWriter, http.Server, http.RouterBuilder, error) {
    failZero()?
    panic("unreachable")
}
func checkZeros() {
    var r, w, server, builder, err = zeroValues()
    if err == nil { panic("missing error") }
    if r.Method() != "" || r.Path() != "" || w.Committed() { panic("zero metadata") }
    if r.Context().Err() != nil { panic("zero context") }
    var data = Array<byte>{0}
    let _, readErr = r.Read(data[:])
    if readErr == nil { panic("zero read") }
    let _, writeErr = w.Write(data[:])
    if writeErr == nil { panic("zero write") }
    let shutdownErr = server.Shutdown(r.Context())
    if shutdownErr == nil { panic("zero shutdown") }
    let routeErr = builder.Handle("GET /", F{n: 0})
    if routeErr == nil { panic("zero builder") }
    let _, buildErr = builder.Build()
    if buildErr == nil { panic("zero build") }
}
func main() {
    checkZeros()
    println(url.PathEscape("a /+é"))
    println(url.QueryEscape("a +"))
    let values, err = url.ParseQuery("a=1&a=2&bare&x=a+b&semi=;&")
    println(err == nil); println(values.Encode())
    let entries = values.Entries()
    println(entries.len())
    let invalid, bad = url.ParseQuery("ok=1&bad=%FF")
    println(bad != nil); println(invalid.Entries().len())
    let paths = [string; 6]{"/a%2fb", "/%5c", "/%00", "/%2e", "/%FF", "/?q=%zz"}
    for path in paths { let _, err = url.ParseRequestTarget(path); println(err != nil) }
    let target, targetErr = url.ParseRequestTarget("/a//b/?x=%FF")
    println(targetErr == nil); println(target.Path); println(target.RawQuery)
    var header = http.NewHeader()
    _ = header.Add("X-A", "first")
    _ = header.Add("x-a", "second")
    println(header.Values("X-a").len())
    let badHeader = header.Set("X-A", "bad\r\ninjected")
    println(badHeader != nil)
    let first, _ = header.Get("x-a")
    println(first)
    _ = header.Add("K", "ascii")
    let _, unicodeMatch = header.Get("K")
    println(unicodeMatch)
    let date, dateErr = time.FormatHTTPDate(0)
    println(dateErr == nil); println(date)
    let leap, _ = time.FormatHTTPDate(951782400)
    println(leap)
    let firstDate, _ = time.FormatHTTPDate(-11644473600)
    println(firstDate)
    let lastDate, _ = time.FormatHTTPDate(253402300799)
    println(lastDate)
    let _, outside = time.FormatHTTPDate(253402300800)
    println(outside != nil); println(time.UnixSeconds() > 0)
    var builder = http.NewRouterBuilder()
    _ = builder.Handle("GET /a/{x}", F{n: 0})
    _ = builder.Handle("GET /{y}/b", F{n: 0})
    let _, ambiguous = builder.Build()
    println(ambiguous != nil)
}
"#,
        "a%20%2F%2B%C3%A9\na+%2B\ntrue\na=1&a=2&bare=&x=a+b&semi=%3B\n5\ntrue\n0\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n/a//b/\nx=%FF\n2\ntrue\nfirst\nfalse\ntrue\nThu, 01 Jan 1970 00:00:00 GMT\nTue, 29 Feb 2000 00:00:00 GMT\nMon, 01 Jan 1601 00:00:00 GMT\nFri, 31 Dec 9999 23:59:59 GMT\ntrue\ntrue\ntrue\n",
    );
}

fn read_headers(stream: &mut TcpStream) -> String {
    let mut output = Vec::new();
    while !output.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        output.push(byte[0]);
        assert!(output.len() < 4096);
    }
    String::from_utf8(output).unwrap()
}

#[test]
fn http_capacity_and_handler_panic_release_the_connection_permit() {
    let source = SERVER.replace(
        "config.MaxBodyBytes = 32",
        "config.MaxConnections = 1\n    config.MaxBodyBytes = 32",
    );
    let server = ServerProcess::start(&source);
    let mut held = server.connect();
    held.write_all(b"GET /echo HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    status(&read_headers(&mut held), 200);
    let mut excess = server.connect();
    assert!(read_closed(&mut excess).is_empty());
    assert!(read_closed(&mut held).is_empty());
    let panic_response = server.exchange("GET /panic HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(panic_response.is_empty());
    let response = server.exchange("GET /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    status(&response, 200);
    server.stop("handler failure");
}

#[test]
fn http_factory_poison_preserves_acceptance_and_shutdown() {
    let source = SERVER
        .replace("type Factory struct { stop channel<bool> }", "type Factory struct { stop channel<bool>; poison bool }")
        .replace("{ return Handler{stop: f.stop}, nil }", "{ if f.poison { panic(\"factory failure\") }; return Handler{stop: f.stop}, nil }")
        .replace("Factory{stop: stop}", "Factory{stop: stop, poison: false}")
        .replace("let router, buildErr = b.Build()", "_ = b.Handle(\"GET /factory-panic\", Factory{stop: stop, poison: true})\n    let router, buildErr = b.Build()")
        .replace("let _, _ = stop.receive()", "let input = os.Stdin()\n    var signalByte = Array<byte>{0}\n    let _, _ = input.Read(signalByte[:])")
        .replace("import \"std/net\"", "import \"std/net\"\nimport \"std/os\"");
    let mut server = ServerProcess::start(&source);
    assert!(
        server
            .exchange("GET /factory-panic HTTP/1.1\r\nHost: x\r\n\r\n")
            .is_empty()
    );
    status(
        &server.exchange("GET /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"),
        503,
    );
    server.child.stdin.take().unwrap().write_all(b"\n").unwrap();
    server.finish("factory failure");
}

#[test]
fn rest_example_model_codec_handles_unicode_and_rejects_non_json_escapes() {
    let codec = include_str!("../../examples/rest-api/json.ore");
    let source = format!(
        r#"{codec}
func main() {{
    let good = strings.Bytes("{{\"name\":\"Ada \\uD83D\\uDE00\"}}")
    let name, err = decodeName(good[:])
    println(err == nil)
    println(quoteJSON(name))
    let inputs = [string; 7]{{"{{\"name\":\"\\x41\"}}", "{{\"name\":\"\\uD800\"}}", "{{\"name\":\"\\uDC00\"}}", "{{\"name\":\"\"}}", "{{\"name\":\"a\",\"name\":\"b\"}}", "{{\"other\":\"a\"}}", "{{\"name\":\"a\"}} trailing"}}
    for input in inputs {{ let _, err = decodeName(strings.Bytes(input)[:]); println(err != nil) }}
}}
"#
    );
    prints(
        &source,
        "true\n\"Ada 😀\"\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn http_shutdown_expiry_cancels_cooperatively_and_later_shutdown_waits() {
    let source = r#"package main
import "std/net"
import "std/net/http"
import "std/context"
import "std/time"
type Factory struct { entered channel<bool>; release channel<bool> }
type H struct { entered channel<bool>; release channel<bool> }
func (f Factory) NewHandler() (http.Handler, error) { return H{entered: f.entered, release: f.release}, nil }
func (h H) Handle(w mut http.ResponseWriter, r mut http.Request) error {
    h.entered.send(true)
    let _, _ = h.release.receive()
    let _, _ = r.Context().Done().receive()
    if r.Context().Err() == nil { panic("missing cancellation") }
    return nil
}
func main() {
    let entered = channel<bool>(1)
    let release = channel<bool>()
    var b = http.NewRouterBuilder()
    _ = b.Handle("GET /hold", Factory{entered: entered, release: release})
    let router, _ = b.Build()
    var config = http.DefaultConfig()
    config.AcceptPollInterval = time.Millisecond
    let server, _ = http.NewServer(router, config)
    let listener, _ = net.Listen("tcp", "127.0.0.1:0")
    println(listener.Addr())
    let serving = go server.Serve(listener)
    let _, _ = entered.receive()
    let ctx, cancel = context.WithTimeout(context.Background(), 20 * time.Millisecond)
    let first = server.Shutdown(ctx)
    cancel()
    if first != error("context deadline exceeded") { panic("shutdown should expire") }
    release.close()
    let second = server.Shutdown(context.Background())
    if second != nil { panic("second shutdown") }
    let err = serving.wait()
    if err != nil { panic("serve") }
}
"#;
    let server = ServerProcess::start(source);
    assert!(
        server
            .exchange("GET /hold HTTP/1.1\r\nHost: x\r\n\r\n")
            .is_empty()
    );
    server.finish("");
}

pub(super) fn check_rest_example() {
    let main = include_str!("../../examples/rest-api/main.ore")
        .replace("127.0.0.1:8080", "127.0.0.1:0")
        .replace(
            "println(\"Listening on http://\" + listener.Addr() + \"; press Enter to shut down\")",
            "println(listener.Addr())",
        );
    let codec = include_str!("../../examples/rest-api/json.ore")
        .replace("package main\n", "")
        .replace("import \"std/strings\"\n", "");
    let mut server = ServerProcess::start(&format!("{main}\n{codec}"));
    let body = r#"{"name":"Ada"}"#;
    let created = server.exchange(&format!("POST /users HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()));
    status(&created, 201);
    assert!(
        created.contains("Location: /users/1\r\n") && created.ends_with(r#"{"id":1,"name":"Ada"}"#),
        "{created}"
    );
    let users = server.exchange("GET /users HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    status(&users, 200);
    assert!(users.ends_with(r#"[{"id":1,"name":"Ada"}]"#), "{users}");
    for (path, expected) in [("/users/1", 200), ("/users/2", 404), ("/users/no", 400)] {
        status(
            &server.exchange(&format!(
                "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
            )),
            expected,
        );
    }
    status(
        &server.exchange(
            "POST /users HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
        415,
    );
    status(&server.exchange("POST /users HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"), 400);
    server.child.stdin.take().unwrap().write_all(b"\n").unwrap();
    server.finish("");
}

fn http_sources() -> String {
    let mut imports = std::collections::BTreeSet::new();
    let mut bodies = String::new();
    for source in [
        include_str!("../../std/net/http/header.ore"),
        include_str!("../../std/net/http/router.ore"),
        include_str!("../../std/net/http/exchange.ore"),
        include_str!("../../std/net/http/server.ore"),
    ] {
        for line in source.lines() {
            if line.starts_with("import ") {
                imports.insert(line);
            } else if !line.starts_with("package ") {
                bodies.push_str(line);
                bodies.push('\n');
            }
        }
    }
    format!(
        "package main\n{}\n{bodies}",
        imports.into_iter().collect::<Vec<_>>().join("\n")
    )
}

#[test]
fn http_faulted_transport_preserves_counts_commitment_and_expect_ordering() {
    let source = http_sources()
        .replace(
            "let deadlineErr = conn[0].SetWriteDeadline(pollDeadline(deadline, interval))",
            "let deadlineErr error = nil",
        )
        .replace(
            "let n, err = conn[0].Write(data[position:])",
            "let n, err = transportWrite(data[position:], server)",
        );
    let source = format!(
        r#"{source}
func transportWrite(data []byte, server Server) (int, error) {{
    if server.config.MaxBodyBytes == 2 {{ return 0, nil }}
    let text, _ = strings.FromBytes(data)
    if server.config.MaxBodyBytes == 4 && strings.HasPrefix(text, "HTTP/1.1") {{ return data.len(), nil }}
    let call = server.state.withLock(func(s mut serverState) int {{ s.connections += 1; return s.connections }})
    if call == 1 {{ return 2, error("net.Write: timed out") }}
    if server.config.MaxBodyBytes != 3 && text != "cdef" {{ panic("repeated or lost bytes") }}
    return 1, error("injected failure")
}}
func testServer(mode int) Server {{
    let builder = NewRouterBuilder()
    let router, _ = builder.Build()
    var config = DefaultConfig()
    config.MaxBodyBytes = mode
    let server, _ = NewServer(router, config)
    return server
}}
func main() {{
    let conn, _ = net.Dial("invalid", "")
    let sockets = [net.Conn; 1]{{conn}}
    let data = strings.Bytes("abcdef")
    let deadline = time.Now() + time.Second
    let count, err = writeBytes(sockets[:], testServer(1), data[:], deadline, time.Millisecond)
    println(count)
    println(err == error("injected failure"))
    let zero, zeroErr = writeBytes(sockets[:], testServer(2), data[:], deadline, time.Millisecond)
    println(zero)
    println(zeroErr == error("http: write made no progress"))
    let server = testServer(3)
    var r = newRequest(sockets[:], server, Array<byte>{{}}, deadline)
    r.expect = true
    r.done = false
    r.remaining = 1
    r.writeDeadline = deadline
    var w = newWriter(sockets[:], server, r.control, deadline, false, false)
    var empty = Array<byte>{{}}
    let _, emptyErr = r.Read(empty[:])
    println(emptyErr == nil)
    println(server.state.withLock(func(s mut serverState) int {{ return s.connections }}))
    let headerErr = w.WriteHeader(403)
    println(headerErr == error("injected failure"))
    println(w.Committed())
    var buffer = Array<byte>{{0}}
    let _, readErr = r.Read(buffer[:])
    println(readErr == error("http.Request.Read: response already started"))
    println(server.state.withLock(func(s mut serverState) int {{ return s.connections }}))
    let again = w.WriteHeader(500)
    println(again == error("http: response already started"))
    let bodyServer = testServer(4)
    var bodyWriter = newWriter(sockets[:], bodyServer, mutex(exchangeControl{{committed: false, continued: false, failure: nil}}), deadline, false, false)
    _ = bodyWriter.SetHeader("Content-Length", "6")
    let written, bodyErr = bodyWriter.Write(data[:])
    println(written)
    println(bodyErr == error("injected failure"))
}}
"#
    );
    prints(
        &source,
        "3\ntrue\n0\ntrue\ntrue\n0\ntrue\ntrue\ntrue\n2\ntrue\n3\ntrue\n",
    );
}

#[test]
fn http_first_byte_preserves_initial_deadline_and_starts_subsequent_budget() {
    let mut source = http_sources();
    for (start, end) in [
        ("func (r mut Request) nextByte", "func (r mut Request) line"),
        (
            "func (r mut Request) beginHeaders",
            "func (r mut Request) parseHeaders",
        ),
    ] {
        let a = source.find(start).unwrap();
        let b = a + source[a..].find(end).unwrap();
        let replacement = source[a..b].replace("time.Now()", "r.server.readClock()");
        source.replace_range(a..b, &replacement);
    }
    let source = format!(
        r#"{source}
func (s Server) readClock() int {{ return s.state.withLock(func(state mut serverState) int {{ return state.connections }}) }}
func (s Server) setClock(value int) {{ s.state.withLock(func(state mut serverState) {{ state.connections = value }}) }}
func main() {{
    let builder = NewRouterBuilder()
    let router, _ = builder.Build()
    var config = DefaultConfig()
    config.ReadHeaderTimeout = 5
    let server, _ = NewServer(router, config)
    let conn, _ = net.Dial("invalid", "")
    let sockets = [net.Conn; 1]{{conn}}
    server.setClock(14)
    var first = newRequest(sockets[:], server, strings.Bytes("GET / HTTP/1.1\r\n"), deadlineAfter(10, config.ReadHeaderTimeout))
    let firstErr = first.beginHeaders(true)
    println(firstErr == nil)
    println(first.deadline)
    server.setClock(15)
    let _, expired = first.line()
    println(expired == error("http: read timed out"))
    server.setClock(14)
    var next = newRequest(sockets[:], server, strings.Bytes("GET / HTTP/1.1\r\n"), 100)
    let nextErr = next.beginHeaders(false)
    println(nextErr == nil)
    println(next.deadline)
    server.setClock(18)
    let line, lineErr = next.line()
    println(lineErr == nil)
    println(line)
}}
"#
    );
    prints(&source, "true\n15\ntrue\ntrue\n19\ntrue\nGET / HTTP/1.1\n");
}

#[test]
fn http_queued_factory_poison_unwinds_both_guards_without_poisoning_accounting() {
    let source = http_sources()
        .replace(
            "let deadlineErr = conn[0].SetWriteDeadline(pollDeadline(deadline, interval))",
            "let deadlineErr error = nil",
        )
        .replace(
            "let n, err = conn[0].Write(data[position:])",
            "let n = data.len() - position\n        let err error = nil",
        );
    let source = format!(
        r#"{source}
type raceFactory struct {{ entered channel<bool>; release channel<bool> }}
func (f raceFactory) NewHandler() (Handler, error) {{
    f.entered.send(true)
    let _, _ = f.release.receive()
    panic("factory race failure")
}}
async func dispatchRace(s Server) {{
    let guard = completion{{server: s}}
    let _, _, err = s.router.dispatch("GET", "/race")
    _ = err
}}
func main() {{
    let entered = channel<bool>(1)
    let release = channel<bool>()
    var builder = NewRouterBuilder()
    _ = builder.Handle("GET /race", raceFactory{{entered: entered, release: release}})
    let router, _ = builder.Build()
    let server, _ = NewServer(router, DefaultConfig())
    server.state.withLock(func(state mut serverState) {{ state.connections = 2 }})
    go dispatchRace(server)
    let _, _ = entered.receive()
    go dispatchRace(server)
    waitBlocked(2)
    release.close()
    for !server.complete() {{ time.Sleep(time.Millisecond) }}
    println(server.router.state.isPoisoned())
    println(server.state.isPoisoned())
    let handler, _, err = router.dispatch("GET", "/race")
    println(err == nil)
    let conn, _ = net.Dial("invalid", "")
    let sockets = [net.Conn; 1]{{conn}}
    let deadline = time.Now() + time.Second
    var request = newRequest(sockets[:], server, Array<byte>{{}}, deadline)
    var writer = newWriter(sockets[:], server, request.control, deadline, false, false)
    let handleErr = handler.Handle(writer, request)
    println(handleErr == nil)
    println(writer.status)
    println(server.complete())
}}
"#
    );
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("race");
    controlled_runtime::build(&source, &executable);
    let output = Command::new(executable)
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "true\nfalse\ntrue\ntrue\n503\ntrue\n");
    let errors = stderr(&output);
    assert!(
        errors.contains("factory race failure") && errors.contains("poison"),
        "{errors}"
    );
    assert!(!errors.contains("leak"), "{errors}");
}
