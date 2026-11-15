use super::*;
use std::time::Duration;

const SCRIPTED_REDIRECTS: &str = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net/http"
import "std/net/url"
import "std/strconv"
import "std/strings"
import "std/time"
type Conn struct { origin string; request string; reply Array<byte>; cursor int; log channel<string>; drops channel<string>; ready time.Instant }
func (c mut Conn) drop() { c.drops.send(c.origin) }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) Write(data []byte) (int, error) {
    let text, _ = strings.FromBytes(data)
    c.request += text
    let head, body, found = strings.Cut(c.request, "\r\n\r\n")
    if !found { return data.len(), nil }
    var length = 0
    for line in strings.Split(head, "\r\n") {
        let name, value, _ = strings.Cut(line, ": ")
        if name == "Content-Length" { length, _ = strconv.Atoi(value) }
    }
    if body.len() < length { return data.len(), nil }
    c.log.send(c.origin + " " + c.request)
    let line, _, _ = strings.Cut(head, "\r\n")
    c.reply = strings.Bytes(answer(c.origin, line))
    c.cursor = 0
    c.request = ""
    c.ready = time.Now()
    if strings.Contains(line, "/slow") { c.ready = time.Now().Add(200 * time.Millisecond) }
    return data.len(), nil
}
func (c mut Conn) Read(data mut []byte) (int, error) {
    if c.cursor >= c.reply.len() || time.Now().Before(c.ready) { return 0, http.ClientPollTimeout }
    data[0] = c.reply[c.cursor]
    c.cursor += 1
    return 1, nil
}
func moved(status string, location string) string {
    return "HTTP/1.1 " + status + "\r\nLocation: " + location + "\r\nContent-Length: 0\r\n\r\n"
}
func ok(text string) string {
    return "HTTP/1.1 200 OK\r\nContent-Length: " + strconv.Itoa(text.len()) + "\r\n\r\n" + text
}
func answer(origin string, line string) string {
    let method, rest, _ = strings.Cut(line, " ")
    let path, _, _ = strings.Cut(rest, " ")
    if path == "/start" { return moved("302 Found", "/next") }
    if path == "/next" { return moved("301 Moved Permanently", "https://b.test/final?x#part") }
    if path == "/back" { return moved("307 Temporary Redirect", "https://a.test/echo") }
    if path == "/away" { return moved("308 Permanent Redirect", "https://b.test/back") }
    if path == "/see" { return moved("303 See Other", "/echo") }
    if path == "/temp" { return moved("307 Temporary Redirect", "/echo") }
    if path == "/found" { return moved("302 Found", "/echo") }
    if path == "/perm" { return moved("301 Moved Permanently", "/echo") }
    if path == "/loop1" { return moved("302 Found", "/loop2") }
    if path == "/loop2" { return moved("302 Found", "/loop1") }
    if strings.HasPrefix(path, "/hop") {
        let n, _ = strconv.Atoi(path[4:])
        if n < 3 { return moved("302 Found", "/hop" + strconv.Itoa(n + 1)) }
        return ok("hops")
    }
    if path == "/down" { return moved("302 Found", "http://a.test/echo") }
    if path == "/ftp" { return moved("302 Found", "ftp://a.test/file") }
    if path == "/broken" { return moved("302 Found", "http://[bad/") }
    if path == "/twice" { return "HTTP/1.1 302 Found\r\nLocation: /echo\r\nLocation: /echo\r\nContent-Length: 0\r\n\r\n" }
    if path == "/empty" { return "HTTP/1.1 302 Found\r\nLocation: \r\nContent-Length: 0\r\n\r\n" }
    if path == "/bigmove" { return "HTTP/1.1 302 Found\r\nLocation: /echo\r\nContent-Length: 4096\r\n\r\n" + strings.Repeat("x", 4096) }
    if path == "/bodymove" { return "HTTP/1.1 302 Found\r\nLocation: /echo\r\nContent-Length: 5\r\n\r\nmoved" }
    if path == "/bare" { return "HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n" }
    if path == "/choices" { return moved("300 Multiple Choices", "/echo") }
    if path == "/slow" { return moved("302 Found", "/slow2") }
    if path == "/slow2" { return moved("302 Found", "/slow") }
    return ok(method + " " + origin + path)
}
type Transport struct { log channel<string>; drops channel<string> }
func (t Transport) Dial(ctx context.Context, target url.AbsoluteURL) (http.ClientConn, error) {
    return Conn{origin: target.Scheme + "://" + target.Hostname, request: "", reply: Array<byte>{}, cursor: 0, log: t.log, drops: t.drops, ready: time.Now()}, nil
}
func check(ok bool, what string) { if !ok { panic(what) } }
func drain(ch channel<string>) Array<string> {
    var items = Array<string>{}
    for {
        select {
            case let item, _ = ch.receive() { items.push(item) }
            default { return items }
        }
    }
}
func text(response http.ClientResponse) string {
    let body, _ = strings.FromBytes(response.Body[:])
    return body
}
func newClient(log channel<string>, drops channel<string>, timeout time.Duration) (http.Client, error) {
    var config = http.DefaultClientConfig()
    config.Timeout = timeout
    return http.NewClientWithTransport(config, Transport{log: log, drops: drops})
}
func follow(client http.Client, method string, target string, body string, config http.RedirectConfig) (http.ClientResponse, string, error) {
    var headers = http.NewHeader()
    headers.Set("Authorization", "Bearer secret")?
    headers.Set("Cookie", "id=1")?
    headers.Set("Proxy-Authorization", "Basic p")?
    headers.Set("X-Keep", "yes")?
    headers.Set("Content-Type", "text/plain")?
    let data = strings.Bytes(body)
    return client.Follow(context.Background(), method, target, headers, data[:], config)
}
func failure(err error) string {
    if err == nil { return "nil" }
    return errors.Message(err)
}
func run() error {
    let log = channel<string>(64)
    let drops = channel<string>(64)
    let client = newClient(log, drops, 3 * time.Second)?
    let defaults = http.DefaultRedirectConfig()
    check(defaults.MaxRedirects == 10 && !defaults.AllowHTTPSDowngrade, "defaults")

    let response, final = follow(client, "GET", "https://a.test/start", "", defaults)?
    check(response.StatusCode == 200 && text(response) == "GET https://b.test/final?x", "chain body")
    check(final == "https://b.test/final?x", "final url " + final)
    let chain = drain(log)
    check(chain.len() == 3, "three requests")
    check(strings.Contains(chain[1], "\r\nAuthorization: Bearer secret\r\n") && strings.Contains(chain[1], "\r\nCookie: id=1\r\n"), "same origin keeps credentials")
    check(!strings.Contains(chain[2], "Authorization") && !strings.Contains(chain[2], "Cookie") && strings.Contains(chain[2], "\r\nX-Keep: yes\r\n"), "cross origin strips credentials")
    check(strings.HasPrefix(chain[2], "https://b.test GET /final?x HTTP/1.1\r\nHost: b.test\r\n"), "fragment dropped and host updated")
    check(drain(drops).len() == 3, "every hop closed its connection")

    let back, backURL = follow(client, "PUT", "https://a.test/away", "data", defaults)?
    check(text(back) == "PUT https://a.test/echo" && backURL == "https://a.test/echo", "308 and 307 keep method")
    let returned = drain(log)
    check(strings.HasSuffix(returned[2], "\r\n\r\ndata") && !strings.Contains(returned[2], "Authorization"), "body kept, credentials stay stripped after returning")

    for pair in [string; 5]{"POST /see GET", "PUT /see GET", "POST /found GET", "POST /perm GET", "DELETE /found DELETE"} {
        let parts = strings.Split(pair, " ")
        let rewritten, _ = follow(client, parts[0], "https://a.test" + parts[1], "data", defaults)?
        check(text(rewritten) == parts[2] + " https://a.test/echo", "rewrite " + pair)
        let hops = drain(log)
        if parts[2] == "GET" {
            check(strings.Contains(hops[1], "\r\nContent-Length: 0\r\n") && !strings.Contains(hops[1], "Content-Type") && strings.HasSuffix(hops[1], "\r\n\r\n"), "body dropped " + pair)
        } else {
            check(strings.HasSuffix(hops[1], "\r\n\r\ndata") && strings.Contains(hops[1], "Content-Type"), "body kept " + pair)
        }
    }
    let temp, _ = follow(client, "POST", "https://a.test/temp", "data", defaults)?
    check(text(temp) == "POST https://a.test/echo", "307 keeps POST")
    check(strings.HasSuffix(drain(log)[1], "\r\n\r\ndata"), "307 keeps body")
    let head, _ = follow(client, "HEAD", "https://a.test/see", "", defaults)?
    check(head.StatusCode == 200 && drain(log)[1].len() > 0, "303 keeps HEAD")

    let _, _, loop = follow(client, "GET", "https://a.test/loop1", "", defaults)
    check(failure(loop) == "http.Client: redirect loop", "loop " + failure(loop))
    var two = defaults
    two.MaxRedirects = 2
    let _, _, many = follow(client, "GET", "https://a.test/hop0", "", two)
    check(failure(many) == "http.Client: too many redirects", "too many")
    var three = defaults
    three.MaxRedirects = 3
    let hops, _ = follow(client, "GET", "https://a.test/hop0", "", three)?
    check(text(hops) == "hops", "exact redirect limit")
    let _ = drain(log)
    let _, _, down = follow(client, "GET", "https://a.test/down", "", defaults)
    check(failure(down) == "http.Client: redirect from HTTPS to HTTP", "downgrade refused")
    var downgrade = defaults
    downgrade.AllowHTTPSDowngrade = true
    let allowed, allowedURL = follow(client, "GET", "https://a.test/down", "", downgrade)?
    check(text(allowed) == "GET http://a.test/echo" && allowedURL == "http://a.test/echo", "downgrade allowed")
    check(!strings.Contains(drain(log)[2], "Authorization"), "scheme change strips credentials")
    for path in [string; 3]{"/ftp", "/broken", "/twice"} {
        let _, _, bad = follow(client, "GET", "https://a.test" + path, "", defaults)
        var expected = "http.Client: invalid redirect location"
        if path == "/twice" { expected = "http.Client: ambiguous redirect location" }
        check(failure(bad) == expected, path + " " + failure(bad))
    }
    let _ = drain(log)
    let _, _, normalized = follow(client, "GET", "https://A.TEST:443/loop1", "", defaults)
    check(failure(normalized) == "http.Client: redirect loop" && drain(log).len() == 2, "loop from a differently written start")
    for path in [string; 3]{"/bare", "/choices", "/empty"} {
        let kept, keptURL = follow(client, "GET", "https://a.test" + path, "", defaults)?
        check(kept.StatusCode >= 300 && keptURL == "https://a.test" + path, "not followed " + path)
    }
    var negative = defaults
    negative.MaxRedirects = -1
    let _, _, negativeErr = follow(client, "GET", "https://a.test/start", "", negative)
    check(failure(negativeErr) == "http.Client: negative redirect configuration", "negative")
    let _ = drain(log)
    check(drain(log).len() == 0, "negative never sends")

    let slow = newClient(log, drops, 300 * time.Millisecond)?
    let _, _, budget = follow(slow, "GET", "https://a.test/slow", "", defaults)
    check(failure(budget) == "context deadline exceeded", "one budget " + failure(budget))
    let _ = drain(log)

    let empty = Array<byte>{}
    var stream, streamURL = client.OpenFollow(context.Background(), "GET", "https://a.test/start", http.NewHeader(), empty[:], http.DefaultStreamConfig(), defaults)?
    var sink = Array<byte>{}
    var buf = [byte; 4]{0, 0, 0, 0}
    for {
        let n, err = stream.Read(buf[:])
        if err == io.EOF { break }
        if err != nil { return err }
        for var i = 0; i < n; i += 1 { sink.push(buf[i]) }
    }
    check(strings.FromBytes(sink[:])? == "GET https://b.test/final?x" && streamURL == "https://b.test/final?x", "streamed chain")
    stream.Close()?
    var totalLimit = http.DefaultStreamConfig()
    totalLimit.Timeout = 300 * time.Millisecond
    let _, _, streamBudget = client.OpenFollow(context.Background(), "GET", "https://a.test/slow", http.NewHeader(), empty[:], totalLimit, defaults)
    check(failure(streamBudget) == "context deadline exceeded", "stream budget " + failure(streamBudget))
    let _ = drain(log)
    let _ = drain(drops)

    var small = http.DefaultStreamConfig()
    small.MaxBodyBytes = 64
    var big, _ = client.OpenFollow(context.Background(), "GET", "https://a.test/bigmove", http.NewHeader(), empty[:], small, defaults)?
    check(big.StatusCode() == 200, "large redirect body ignored")
    big.Close()?
    let _, _, tooBig = client.OpenFollow(context.Background(), "GET", "https://a.test/bigmove", http.NewHeader(), empty[:], small, two)
    _ = tooBig
    let _ = drain(log)
    let _ = drain(drops)

    var single = http.DefaultPoolConfig()
    single.MaxConnectionsPerHost = 1
    let singleDrops = channel<string>(64)
    let narrow = http.NewPool(newClient(log, singleDrops, 3 * time.Second)?, single)?
    var moved, movedURL = narrow.OpenFollow(context.Background(), "GET", "https://a.test/bodymove", http.NewHeader(), empty[:], http.DefaultStreamConfig(), defaults)?
    check(moved.StatusCode() == 200 && movedURL == "https://a.test/echo", "redirect body on a single connection")
    check(narrow.Stats().Active == 1 && drain(singleDrops).len() == 1, "unread redirect retired its connection")
    moved.Close()?
    check(narrow.Stats().Active == 0 && drain(singleDrops).len() == 1, "final stream closed early")
    narrow.Close()
    let _ = drain(log)

    let poolDrops = channel<string>(64)
    let pool = http.NewPool(newClient(log, poolDrops, 3 * time.Second)?, http.DefaultPoolConfig())?
    let pooled, pooledURL = pool.Follow(context.Background(), "GET", "https://a.test/start", http.NewHeader(), empty[:], defaults)?
    check(text(pooled) == "GET https://b.test/final?x" && pooledURL == "https://b.test/final?x", "pooled chain")
    check(pool.Stats().Idle == 2 && drain(poolDrops).len() == 0, "pooled hops reuse connections")
    var pooledStream, _ = pool.OpenFollow(context.Background(), "GET", "https://a.test/start", http.NewHeader(), empty[:], http.DefaultStreamConfig(), defaults)?
    check(pooledStream.StatusCode() == 200 && pool.Stats().Active == 1, "pooled stream")
    pooledStream.Close()?
    check(pool.Stats().Active == 0 && pool.Stats().Idle == 1, "pooled stream chain released")
    pool.Close()
    let _ = drain(log)
    return nil
}
func main() {
    let err = run()
    if err != nil { panic(errors.Message(err)) }
    println("ok")
}
"#;

#[test]
fn http_redirects_follow_rewrite_strip_and_bound_every_chain() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("redirects");
    build_file(SCRIPTED_REDIRECTS, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
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
fn http_redirect_guide_example_follows_to_the_final_answer() {
    use std::io::{Read, Write};
    let guide = include_str!("../../docs/http-redirects.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let source = blocks[0].replace(
        "http://localhost:8080/latest",
        &format!("http://{address}/latest"),
    );
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("guide");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let server = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for reply in [
            "HTTP/1.1 302 Found\r\nLocation: /files/v2\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nv2",
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
            seen.push(String::from_utf8_lossy(&request).into_owned());
            stream.write_all(reply.as_bytes()).unwrap();
        }
        seen
    });
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    let seen = server.join().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), format!("200\nhttp://{address}/files/v2\n"));
    assert!(
        seen[0].starts_with("GET /latest HTTP/1.1\r\n"),
        "{}",
        seen[0]
    );
    assert!(
        seen[1].starts_with("GET /files/v2 HTTP/1.1\r\n"),
        "{}",
        seen[1]
    );
}
