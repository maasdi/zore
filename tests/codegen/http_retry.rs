use super::*;
use std::time::Duration;

const SCRIPTED_RETRIES: &str = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net/http"
import "std/net/url"
import "std/strings"
import "std/time"
type Conn struct { request string; reply Array<byte>; cursor int; drop bool; seen Mutex<map[string]int> }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) Write(data []byte) (int, error) {
    let text, _ = strings.FromBytes(data)
    c.request += text
    let head, _, found = strings.Cut(c.request, "\r\n\r\n")
    if !found { return data.len(), nil }
    let line, _, _ = strings.Cut(head, "\r\n")
    let method, rest, _ = strings.Cut(line, " ")
    let path, _, _ = strings.Cut(rest, " ")
    let key = method + " " + path
    let n = c.seen.withLock(func(seen mut map[string]int) int {
        let _, count = seen[key]
        seen[key] = count + 1
        return count + 1
    })
    c.request = ""
    c.cursor = 0
    c.drop = path == "/drop" && n < 3
    c.reply = strings.Bytes(answer(path, n))
    return data.len(), nil
}
func (c mut Conn) Read(data mut []byte) (int, error) {
    if c.drop { return 0, io.EOF }
    if c.cursor >= c.reply.len() { return 0, http.ClientPollTimeout }
    data[0] = c.reply[c.cursor]
    c.cursor += 1
    return 1, nil
}
func status(code string, extra string) string {
    return "HTTP/1.1 " + code + "\r\n" + extra + "Content-Length: 0\r\n\r\n"
}
func answer(path string, n int) string {
    let ok = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"
    if path == "/flaky" || path == "/drop" {
        if n < 3 { return status("503 Service Unavailable", "") }
        return ok
    }
    if path == "/down" { return status("503 Service Unavailable", "") }
    if path == "/gateway" { return status("502 Bad Gateway", "") }
    if path == "/missing" { return status("404 Not Found", "") }
    if path == "/error" { return status("500 Internal Server Error", "") }
    if path == "/limited" {
        if n == 1 { return status("429 Too Many Requests", "Retry-After: 0\r\n") }
        return ok
    }
    if path == "/later" { return status("429 Too Many Requests", "Retry-After: 100\r\n") }
    return ok
}
type Transport struct { seen Mutex<map[string]int> }
func (t Transport) Dial(ctx context.Context, target url.AbsoluteURL) (http.ClientConn, error) {
    return Conn{request: "", reply: Array<byte>{}, cursor: 0, drop: false, seen: t.seen}, nil
}
func check(ok bool, what string) { if !ok { panic(what) } }
func count(seen Mutex<map[string]int>, key string) int {
    return seen.withLock(func(m mut map[string]int) int {
        let _, n = m[key]
        let _, _ = m.remove(key)
        return n
    })
}
func failure(err error) string {
    if err == nil { return "nil" }
    return errors.Message(err)
}
func quick() http.RetryConfig {
    var config = http.DefaultRetryConfig()
    config.Backoff = 10 * time.Millisecond
    config.MaxBackoff = 40 * time.Millisecond
    return config
}
func call(client http.Client, method string, path string, config http.RetryConfig) (int, error) {
    let empty = Array<byte>{}
    let response = client.DoRetry(context.Background(), method, "https://a.test" + path, http.NewHeader(), empty[:], config)?
    return response.StatusCode, nil
}
async func cancelSoon(cancel own func()) {
    time.Sleep(50 * time.Millisecond)
    cancel()
}
func run() error {
    let defaults = http.DefaultRetryConfig()
    check(defaults.MaxAttempts == 3 && defaults.Backoff == 200 * time.Millisecond && defaults.MaxBackoff == 5 * time.Second && defaults.Timeout == 0 && !defaults.RetryUnsafe, "defaults")
    let seen = mutex(map[string]int{})
    let client = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{seen: seen})?

    let started = time.Now()
    check(call(client, "GET", "/flaky", quick())? == 200 && count(seen, "GET /flaky") == 3, "503 twice then success")
    check(time.Since(started) >= 30 * time.Millisecond, "backoff doubles between attempts")
    check(call(client, "GET", "/drop", quick())? == 200 && count(seen, "GET /drop") == 3, "dropped connections retried")
    check(call(client, "GET", "/down", quick())? == 503 && count(seen, "GET /down") == 3, "last response returned")
    check(call(client, "PUT", "/gateway", quick())? == 502 && count(seen, "PUT /gateway") == 3, "PUT retried")
    check(call(client, "GET", "/missing", quick())? == 404 && count(seen, "GET /missing") == 1, "404 not retried")
    check(call(client, "GET", "/error", quick())? == 500 && count(seen, "GET /error") == 1, "500 not retried")
    check(call(client, "GET", "/limited", quick())? == 200 && count(seen, "GET /limited") == 2, "Retry-After zero retried")
    check(call(client, "GET", "/later", quick())? == 429 && count(seen, "GET /later") == 1, "Retry-After above the cap returned")
    check(call(client, "POST", "/flaky", quick())? == 503 && count(seen, "POST /flaky") == 1, "POST not retried")
    var risky = quick()
    risky.RetryUnsafe = true
    check(call(client, "POST", "/flaky", risky)? == 200 && count(seen, "POST /flaky") == 3, "POST retried when allowed")
    var once = quick()
    once.MaxAttempts = 1
    check(call(client, "GET", "/down", once)? == 503 && count(seen, "GET /down") == 1, "single attempt")
    var five = quick()
    five.MaxAttempts = 5
    check(call(client, "GET", "/down", five)? == 503 && count(seen, "GET /down") == 5, "five attempts")
    let _, dropErr = call(client, "GET", "/drop", once)
    check(dropErr != nil && count(seen, "GET /drop") == 1, "last error returned")

    var budget = quick()
    budget.Backoff = 200 * time.Millisecond
    budget.MaxBackoff = 200 * time.Millisecond
    budget.Timeout = 50 * time.Millisecond
    let budgetStart = time.Now()
    check(call(client, "GET", "/down", budget)? == 503 && count(seen, "GET /down") == 1 && time.Since(budgetStart) < 150 * time.Millisecond, "no wait past the total budget")

    var slow = quick()
    slow.Backoff = 2 * time.Second
    slow.MaxBackoff = 2 * time.Second
    let ctx, cancel = context.WithCancel(context.Background())
    go cancelSoon(cancel)
    let cancelStart = time.Now()
    let empty = Array<byte>{}
    let _, cancelErr = client.DoRetry(ctx, "GET", "https://a.test/down", http.NewHeader(), empty[:], slow)
    check(failure(cancelErr) == "context canceled" && time.Since(cancelStart) < time.Second && count(seen, "GET /down") == 1, "cancel stops the wait")

    var negative = quick()
    negative.MaxAttempts = -1
    let _, negativeErr = call(client, "GET", "/flaky", negative)
    check(failure(negativeErr) == "http.Client: negative retry configuration", "negative")
    let _, invalidErr = call(client, "GET", "/bad path", quick())
    check(invalidErr != nil && count(seen, "GET /bad") == 0, "invalid request never sent")
    let plain = http.NewClient(http.DefaultClientConfig())?
    let _, tlsErr = plain.DoRetry(context.Background(), "GET", "https://a.test/", http.NewHeader(), empty[:], quick())
    check(failure(tlsErr) == "http.Client: HTTPS requires verified TLS transport", "https without transport")

    let pool = http.NewPool(http.NewClientWithTransport(http.DefaultClientConfig(), Transport{seen: seen})?, http.DefaultPoolConfig())?
    let pooled = pool.DoRetry(context.Background(), "GET", "https://a.test/flaky", http.NewHeader(), empty[:], quick())?
    check(pooled.StatusCode == 200 && count(seen, "GET /flaky") == 3 && pool.Stats().Active == 0, "pooled retries")
    pool.Close()
    let closedStart = time.Now()
    let _, closedErr = pool.DoRetry(context.Background(), "GET", "https://a.test/flaky", http.NewHeader(), empty[:], quick())
    check(failure(closedErr) == "http.Pool: closed" && count(seen, "GET /flaky") == 0 && time.Since(closedStart) < 10 * time.Millisecond, "closed pool fails before any wait")
    var capped = quick()
    capped.Backoff = 10 * time.Second
    capped.MaxBackoff = 30 * time.Millisecond
    let cappedStart = time.Now()
    check(call(client, "GET", "/flaky", capped)? == 200 && time.Since(cappedStart) < time.Second && count(seen, "GET /flaky") == 3, "first wait capped")
    return nil
}
func main() {
    let err = run()
    if err != nil { panic(errors.Message(err)) }
    println("ok")
}
"#;

#[test]
fn http_retries_repeat_only_safe_requests_within_their_budget() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("retries");
    build_file(SCRIPTED_RETRIES, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
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
fn http_retry_guide_example_tries_again_after_a_busy_answer() {
    use std::io::{Read, Write};
    let guide = include_str!("../../docs/http-retries.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let source = blocks[0].replace(
        "http://localhost:8080/health",
        &format!("http://{address}/health"),
    );
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("guide");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let server = std::thread::spawn(move || {
        for reply in [
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                request.push(byte[0]);
            }
            assert!(request.starts_with(b"GET /health HTTP/1.1\r\n"));
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "200\n");
}
