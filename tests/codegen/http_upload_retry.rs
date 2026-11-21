use super::*;
use std::io::Read;
use std::time::Duration;

const PROGRAM: &str = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net/http"
import "std/net/url"
import "std/strconv"
import "std/strings"
import "std/time"
type State struct { dials int; closed int; requests int; made int; dropped int; bodies int; active int; overlap bool; reads int; wires Array<string> }
func state() Mutex<State> { return mutex(State{dials: 0, closed: 0, requests: 0, made: 0, dropped: 0, bodies: 0, active: 0, overlap: false, reads: 0, wires: Array<string>{}}) }
type Source struct { left int; mode string; seen Mutex<State> }
func (s mut Source) drop() { if s.mode == "panic" || s.mode == "factorypanic" { println("body dropped") }; s.seen.withLock(func(v mut State) { v.bodies += 1; v.active -= 1 }) }
func (s mut Source) Read(buf mut []byte) (int, error) {
    s.seen.withLock(func(v mut State) { v.reads += 1 })
    if s.mode == "panic" { panic("injected source panic") }
    if s.mode == "sourcetimeout" { return 0, error("context deadline exceeded") }
    if s.mode == "stall" { return 0, nil }
    if s.mode == "invalid" { return buf.len() + 1, nil }
    if s.left == 0 { return 0, io.EOF }
    var n = buf.len()
    if n > 3 { n = 3 }
    if n > s.left { n = s.left }
    for var i = 0; i < n; i += 1 { buf[i] = 120 }
    s.left -= n
    if s.mode == "source" { return n, error("source failure") }
    if s.mode == "known_eof" && s.left == 0 { return n, io.EOF }
    return n, nil
}
func factoryFailure() error { return error("factory failure") }
type Factory struct { mode string; size int; seen Mutex<State> }
func (f mut Factory) drop() { if f.mode == "panic" || f.mode == "factorypanic" { println("factory dropped") }; f.seen.withLock(func(v mut State) { v.dropped += 1 }) }
func (f mut Factory) NewBody(ctx context.Context) (http.RequestBody, error) {
    f.seen.withLock(func(v mut State) {
        v.made += 1
        if v.active != 0 { v.overlap = true }
        v.active += 1
    })
    if f.mode == "emptyfactory" {
        f.seen.withLock(func(v mut State) { v.active -= 1 })
        factoryFailure()?
    }
    var size = f.size
    if f.mode == "short" { size = 2 }
    let source = Source{left: size, mode: f.mode, seen: f.seen}
    if f.mode == "factorypanic" { panic("injected factory panic") }
    if f.mode == "slow" { ctx.Done().receive(); return http.NewUnknownLengthBody(source), ctx.Err() }
    if f.mode == "factory" { return http.NewUnknownLengthBody(source), error("factory failure") }
    if f.mode == "known" || f.mode == "known_eof" || f.mode == "short" { return http.NewRequestBody(source, f.size) }
    return http.NewUnknownLengthBody(source), nil
}
type Conn struct { request string; reply Array<byte>; cursor int; seen Mutex<State>; mode string; dial int }
func (c mut Conn) drop() { if c.mode == "panic" { println("connection dropped") }; c.seen.withLock(func(v mut State) { v.closed += 1 }) }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) Read(buf mut []byte) (int, error) {
    if c.mode == "timeout" && c.dial == 1 { return 0, http.ClientPollTimeout }
    if c.cursor == c.reply.len() { return 0, http.ClientPollTimeout }
    var n = buf.len()
    if n > c.reply.len() - c.cursor { n = c.reply.len() - c.cursor }
    for var i = 0; i < n; i += 1 { buf[i] = c.reply[c.cursor + i] }
    c.cursor += n
    return n, nil
}
func (c mut Conn) Write(data []byte) (int, error) {
    if c.dial == 1 && c.request == "" {
        if c.mode == "zero" { return 0, error("wire failure") }
        if c.mode == "partial" { return 1, error("wire failure") }
        if c.mode == "badwrite" { return data.len() + 1, nil }
    }
    if c.mode == "bodywire" && strings.Contains(c.request, "\r\n\r\n") { return 0, error("wire failure") }
    let text, _ = strings.FromBytes(data)
    c.request += text
    let head, body, found = strings.Cut(c.request, "\r\n\r\n")
    if !found { return data.len(), nil }
    var complete = false
    if strings.Contains(head, "Transfer-Encoding: chunked") {
        complete = strings.HasSuffix(body, "0\r\n\r\n")
    } else {
        for line in strings.Split(head, "\r\n") {
            if strings.HasPrefix(line, "Content-Length: ") {
                let size, _ = strconv.Atoi(strings.TrimPrefix(line, "Content-Length: "))
                complete = body.len() == size
            }
        }
    }
    if !complete { return data.len(), nil }
    let wire = c.request
    let count = c.seen.withLock(func(v mut State) int { v.requests += 1; v.wires.push(wire); return v.requests })
    c.request = ""
    c.cursor = 0
    var status = "200 OK"
    var extra = ""
    if c.mode == "flaky" && count < 3 || c.mode == "down" { status = "503 Service Unavailable" }
    if c.mode == "later" { status = "429 Too Many Requests"; extra = "Retry-After: 100\r\n" }
    c.reply = strings.Bytes("HTTP/1.1 " + status + "\r\n" + extra + "Content-Length: 2\r\n\r\nok")
    return data.len(), nil
}
type Dead struct {}
func (d mut Dead) Read(buf mut []byte) (int, error) { return 0, io.EOF }
func (d mut Dead) Write(data []byte) (int, error) { return 0, error("closed") }
func (d mut Dead) SetReadDeadline(deadline time.Instant) error { return nil }
func (d mut Dead) SetWriteDeadline(deadline time.Instant) error { return nil }
type Transport struct { seen Mutex<State>; mode string }
func (t Transport) Dial(ctx context.Context, target url.AbsoluteURL) (http.ClientConn, error) {
    let dial = t.seen.withLock(func(v mut State) int { v.dials += 1; return v.dials })
    if t.mode == "dial" && dial == 1 { return Dead{}, error("wire failure") }
    return Conn{request: "", reply: Array<byte>{}, cursor: 0, seen: t.seen, mode: t.mode, dial: dial}, nil
}
func check(ok bool, label string) { if !ok { panic(label) } }
func quick() http.RetryConfig {
    var config = http.DefaultRetryConfig()
    config.Backoff = time.Millisecond
    config.MaxBackoff = 2 * time.Millisecond
    return config
}
func counts(seen Mutex<State>, made int, bodies int, dials int, requests int) {
    let good = seen.withLock(func(v mut State) bool {
        return v.made == made && v.dropped == 1 && v.bodies == bodies && v.dials == dials && v.requests == requests && v.active == 0 && !v.overlap
    })
    check(good, "factory/body/connection accounting")
}
func attempt(method string, transport string, reader string, risky bool, made int, dials int, requests int, status int, failure error) error {
    _ = failure
    let seen = state()
    var clientConfig = http.DefaultClientConfig()
    if transport == "timeout" { clientConfig.Timeout = 100 * time.Millisecond }
    let client = http.NewClientWithTransport(clientConfig, Transport{seen: seen, mode: transport})?
    var retry = quick()
    retry.RetryUnsafe = risky
    let response, err = client.UploadRetry(context.Background(), method, "https://example.test/", http.NewHeader(), Factory{mode: reader, size: 3, seen: seen}, http.DefaultUploadConfig(), retry)
    let matched = err == failure
    check(response.StatusCode == status && matched, "unexpected retry outcome: " + transport + " / " + reader)
    var bodies = made
    if reader == "emptyfactory" { bodies = 0 }
    counts(seen, made, bodies, dials, requests)
    let closed = seen.withLock(func(v mut State) int { return v.closed })
    var expectedClosed = dials
    if transport == "dial" { expectedClosed -= 1 }
    check(closed == expectedClosed, "owned connection cleanup")
    return nil
}
func pool() error {
    let seen = state()
    let client = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{seen: seen, mode: "flaky"})?
    let pool = http.NewPool(client, http.DefaultPoolConfig())?
    let response = pool.UploadRetry(context.Background(), "PUT", "https://example.test/", http.NewHeader(), Factory{mode: "known_eof", size: 3, seen: seen}, http.DefaultUploadConfig(), quick())?
    check(response.StatusCode == 200, "pooled retry response")
    counts(seen, 3, 3, 1, 3)
    check(pool.Stats().Idle == 1 && pool.Stats().Active == 0, "pooled retry did not reuse")
    seen.withLock(func(v mut State) { v.active += 1 })
    let unknown = http.NewUnknownLengthBody(Source{left: 3, mode: "normal", seen: seen})
    pool.Upload(context.Background(), "PUT", "https://example.test/", http.NewHeader(), unknown, http.DefaultUploadConfig())?
    let wireGood = seen.withLock(func(v mut State) bool {
        return strings.Contains(v.wires[0], "Content-Length: 3\r\n") && strings.Contains(v.wires[3], "Transfer-Encoding: chunked\r\n") && !strings.Contains(v.wires[3], "Connection: close")
    })
    check(wireGood && pool.Stats().Idle == 1, "pooled framing")
    seen.withLock(func(v mut State) { v.active += 1 })
    let broken = http.NewUnknownLengthBody(Source{left: 3, mode: "source", seen: seen})
    let _, err = pool.Upload(context.Background(), "PUT", "https://example.test/", http.NewHeader(), broken, http.DefaultUploadConfig())
    check(err == error("source failure") && pool.Stats().Idle == 0 && pool.Stats().Active == 0, "partial upload reused connection")
    seen.withLock(func(v mut State) { v.active += 1 })
    let next = http.NewRequestBody(Source{left: 3, mode: "normal", seen: seen}, 3)?
    pool.Upload(context.Background(), "PUT", "https://example.test/", http.NewHeader(), next, http.DefaultUploadConfig())?
    pool.Close()
    let settled = seen.withLock(func(v mut State) bool { return v.dials == 2 && v.closed == 2 && v.active == 0 && v.bodies == 6 })
    check(settled, "pool retirement and cleanup")
    return nil
}
func validation() error {
    for mode in Array<string>{"url", "method", "header", "upload", "retry", "canceled", "closed", "tls"} {
        let seen = state()
        var clients = Array<http.Client>{}
        if mode == "tls" { clients.push(http.NewClient(http.DefaultClientConfig())?) } else {
            clients.push(http.NewClientWithTransport(http.DefaultClientConfig(), Transport{seen: seen, mode: "ok"})?)
        }
        let _, client = clients.pop()
        let ctx, cancel = context.WithCancel(context.Background())
        if mode == "canceled" { cancel() }
        var target = "https://example.test/"
        var method = "PUT"
        var headers = http.NewHeader()
        var upload = http.DefaultUploadConfig()
        var retry = quick()
        if mode == "url" { target = "/bad" }
        if mode == "method" { method = "TRACE" }
        if mode == "header" { headers.Set("Content-Length", "3")? }
        if mode == "upload" { upload.MaxBodyBytes = -1 }
        if mode == "retry" { retry.MaxAttempts = -1 }
        if mode == "closed" {
            let pool = http.NewPool(client, http.DefaultPoolConfig())?
            pool.Close()
            let _, err = pool.UploadRetry(ctx, method, target, headers, Factory{mode: "normal", size: 3, seen: seen}, upload, retry)
            check(err == error("http.Pool: closed"), "closed pool")
        } else {
            let _, err = client.UploadRetry(ctx, method, target, headers, Factory{mode: "normal", size: 3, seen: seen}, upload, retry)
            check(err != nil, "invalid request: " + mode)
        }
        cancel()
        counts(seen, 0, 0, 0, 0)
    }
    return nil
}
func factoryTimeout() error {
    let seen = state()
    let client = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{seen: seen, mode: "ok"})?
    var settings = http.DefaultUploadConfig()
    settings.Timeout = 20 * time.Millisecond
    let _, err = client.UploadRetry(context.Background(), "PUT", "https://example.test/", http.NewHeader(), Factory{mode: "slow", size: 3, seen: seen}, settings, quick())
    check(err == error("context deadline exceeded"), "factory did not receive attempt deadline")
    counts(seen, 1, 1, 0, 0)
    return nil
}
func limits() error {
    for mode in Array<string>{"normal", "known"} {
        let seen = state()
        let client = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{seen: seen, mode: "flaky"})?
        var settings = http.DefaultUploadConfig()
        settings.MaxBodyBytes = 2
        let _, err = client.UploadRetry(context.Background(), "PUT", "https://example.test/", http.NewHeader(), Factory{mode: mode, size: 3, seen: seen}, settings, quick())
        check(err == error("http.Client: request body too large"), "body limit")
        var dials = 1
        if mode == "known" { dials = 0 }
        counts(seen, 1, 1, dials, 0)
    }
    return nil
}
func run() error {
    attempt("PUT", "flaky", "normal", false, 3, 3, 3, 200, nil)?
    attempt("PUT", "flaky", "known_eof", false, 3, 3, 3, 200, nil)?
    attempt("POST", "flaky", "normal", false, 1, 1, 1, 503, nil)?
    attempt("POST", "flaky", "normal", true, 3, 3, 3, 200, nil)?
    attempt("POST", "zero", "normal", false, 2, 2, 1, 200, nil)?
    attempt("POST", "dial", "normal", false, 2, 2, 1, 200, nil)?
    attempt("POST", "partial", "normal", false, 1, 1, 0, 0, error("wire failure"))?
    attempt("POST", "partial", "normal", true, 2, 2, 1, 200, nil)?
    attempt("POST", "badwrite", "normal", false, 1, 1, 0, 0, error("http.Client: invalid write count"))?
    attempt("PUT", "flaky", "source", false, 1, 1, 0, 0, error("source failure"))?
    attempt("PUT", "bodywire", "source", false, 1, 1, 0, 0, error("wire failure"))?
    attempt("PUT", "flaky", "factory", false, 1, 0, 0, 0, error("factory failure"))?
    attempt("PUT", "flaky", "emptyfactory", false, 1, 0, 0, 0, error("factory failure"))?
    attempt("PUT", "flaky", "stall", false, 1, 1, 0, 0, error("http.Client: request body made no progress"))?
    attempt("PUT", "flaky", "invalid", false, 1, 1, 0, 0, error("http.Client: invalid read count"))?
    attempt("PUT", "flaky", "short", false, 1, 1, 0, 0, error("http.Client: request body shorter than declared length"))?
    attempt("PUT", "later", "normal", false, 1, 1, 1, 429, nil)?
    attempt("PUT", "timeout", "normal", false, 2, 2, 2, 200, nil)?
    attempt("POST", "timeout", "normal", false, 1, 1, 1, 0, error("context deadline exceeded"))?
    attempt("PUT", "flaky", "sourcetimeout", false, 1, 1, 0, 0, error("context deadline exceeded"))?
    factoryTimeout()?
    validation()?
    limits()?
    return pool()
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"#;

#[test]
fn http_upload_retry_requires_explicit_fresh_bodies_and_reuses_only_clean_connections() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("upload_retry");
    build_file(PROGRAM, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
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
fn http_pool_uploads_reuse_retire_and_cancel_waiters_on_one_worker() {
    let source = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net"
import "std/net/http"
import "std/strings"
import "std/time"
type Source struct { at int; block bool; fail bool; entered channel<bool>; release channel<bool>; dropped channel<int> }
func (s mut Source) drop() { s.dropped.send(s.at) }
func (s mut Source) Read(buf mut []byte) (int, error) {
    if s.block { s.block = false; s.entered.send(true); s.release.receive() }
    if s.at == 3 { return 0, io.EOF }
    var n = 3 - s.at
    if n > buf.len() { n = buf.len() }
    for var i = 0; i < n; i += 1 { buf[i] = 120 }
    s.at += n
    if s.fail { return n, error("source failure") }
    return n, nil
}
func head(conn net.Conn) (string, error) {
    var text = ""
    var one = Array<byte>{0}
    conn.SetReadDeadline(time.Now().Add(5 * time.Second))?
    for !strings.HasSuffix(text, "\r\n\r\n") {
        let n = conn.Read(one[:])?
        if n != 1 { return "", error("header stalled") }
        text += strings.FromBytes(one[:])?
    }
    if strings.Contains(text, "Connection: close") { return "", error("closing upload header") }
    return text, nil
}
func take(conn net.Conn, size int) (string, error) {
    var text = ""
    var one = Array<byte>{0}
    for text.len() < size {
        let n, err = conn.Read(one[:])
        _ = err
        if n == 1 { text += strings.FromBytes(one[:])? }
        if err != nil { return text, err }
        if n == 0 { return text, error("body stalled") }
    }
    return text, nil
}
func answer(conn net.Conn) error {
    let bytes = strings.Bytes("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
    var at = 0
    for at < bytes.len() { at += conn.Write(bytes[at:])? }
    return nil
}
async func server(listener own net.Listener) error {
    let first = listener.Accept()?
    let known = head(first)?
    if !strings.Contains(known, "Content-Length: 3\r\n") { return error("known framing") }
    if take(first, 3)? != "xxx" { return error("first body") }
    answer(first)?
    let unknown = head(first)?
    if !strings.Contains(unknown, "Transfer-Encoding: chunked\r\n") { return error("unknown framing") }
    if take(first, 13)? != "3\r\nxxx\r\n0\r\n\r\n" { return error("chunk body") }
    answer(first)?
    let broken = head(first)?
    if !strings.Contains(broken, "Content-Length: 10\r\n") { return error("partial framing") }
    let prefix, err = take(first, 10)
    let failed = err != nil
    if prefix != "xxx" || !failed { return error("partial connection was retained") }
    let second = listener.Accept()?
    head(second)?
    if take(second, 3)? != "xxx" { return error("replacement body") }
    return answer(second)
}
async func send(pool http.Pool, ctx context.Context, target string, body own http.RequestBody) error {
    let response = pool.Upload(ctx, "PUT", target, http.NewHeader(), body, http.DefaultUploadConfig())?
    if response.StatusCode != 200 { return error("upload response") }
    return nil
}
func body(block bool, fail bool, entered channel<bool>, release channel<bool>, dropped channel<int>, length int) (http.RequestBody, error) {
    return http.NewRequestBody(Source{at: 0, block: block, fail: fail, entered: entered, release: release, dropped: dropped}, length)
}
func requireDrop(dropped channel<int>, count int) error {
    let actual, _ = dropped.receive()
    if actual != count { return error("reader drop accounting") }
    return nil
}
func run() error {
    waitIdle(0)
    let listener = net.Listen("tcp", "127.0.0.1:0")?
    let target = "http://" + listener.Addr() + "/"
    let peer = go server(listener)
    var limits = http.DefaultPoolConfig()
    limits.MaxConnections = 1
    limits.MaxConnectionsPerHost = 1
    limits.MaxWaiters = 1
    let pool = http.NewPool(http.NewClient(http.DefaultClientConfig())?, limits)?
    let entered = channel<bool>(1)
    let release = channel<bool>(1)
    let firstDrop = channel<int>(1)
    let firstBody = body(true, false, entered, release, firstDrop, 3)?
    let first = go send(pool, context.Background(), target, firstBody)
    entered.receive()
    let canceledDrop = channel<int>(1)
    let queuedBody = body(false, false, entered, release, canceledDrop, 3)?
    let ctx, cancel = context.WithCancel(context.Background())
    let queued = go send(pool, ctx, target, queuedBody)
    let until = time.Now().Add(5 * time.Second)
    for pool.Stats().Waiting != 1 {
        if !time.Now().Before(until) { return error("upload did not wait for lease") }
        time.Sleep(time.Millisecond)
    }
    let rejectedDrop = channel<int>(1)
    let rejected = body(false, false, entered, release, rejectedDrop, 3)?
    let _, fullErr = pool.Upload(context.Background(), "PUT", target, http.NewHeader(), rejected, http.DefaultUploadConfig())
    if fullErr != error("http.Pool: too many waiters") { return error("waiter bound") }
    requireDrop(rejectedDrop, 0)?
    cancel()
    let canceled = queued.wait()
    if canceled != error("context canceled") { return error("waiter cancellation") }
    requireDrop(canceledDrop, 0)?
    release.send(true)
    first.wait()?
    requireDrop(firstDrop, 3)?
    if pool.Stats().Idle != 1 || pool.Stats().Active != 0 || pool.Stats().Waiting != 0 { return error("pool did not settle") }
    let unknownDrop = channel<int>(1)
    let unknown = http.NewUnknownLengthBody(Source{at: 0, block: false, fail: false, entered: entered, release: release, dropped: unknownDrop})
    pool.Upload(context.Background(), "PUT", target, http.NewHeader(), unknown, http.DefaultUploadConfig())?
    requireDrop(unknownDrop, 3)?
    let brokenDrop = channel<int>(1)
    let broken = body(false, true, entered, release, brokenDrop, 10)?
    let _, failure = pool.Upload(context.Background(), "PUT", target, http.NewHeader(), broken, http.DefaultUploadConfig())
    if failure != error("source failure") || pool.Stats().Idle != 0 || pool.Stats().Active != 0 { return error("failed connection reused") }
    requireDrop(brokenDrop, 3)?
    let lastDrop = channel<int>(1)
    let last = body(false, false, entered, release, lastDrop, 3)?
    pool.Upload(context.Background(), "PUT", target, http.NewHeader(), last, http.DefaultUploadConfig())?
    requireDrop(lastDrop, 3)?
    pool.Close()
    peer.wait()?
    return nil
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"#;
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("pooled_upload");
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
fn http2_upload_retry_replays_refusals_and_preserves_stream_and_factory_ownership() {
    let source = format!(
        "{}{}",
        super::http2::peer_source(),
        r#"
type Counts struct { made int; bodies int; dropped int; active int; overlap bool }
type Source struct { at int; fail bool; seen Mutex<Counts> }
func (s mut Source) drop() { s.seen.withLock(func(v mut Counts) { v.bodies += 1; v.active -= 1 }) }
func (s mut Source) Read(buf mut []byte) (int, error) {
    if s.at == 3 { return 0, io.EOF }
    var n = 3 - s.at
    if n > buf.len() { n = buf.len() }
    for var i = 0; i < n; i += 1 { buf[i] = 120 }
    s.at += n
    if s.fail { return n, error("source failure") }
    return n, nil
}
type Factory struct { seen Mutex<Counts>; fail bool }
func (f mut Factory) drop() { f.seen.withLock(func(v mut Counts) { v.dropped += 1 }) }
func (f mut Factory) NewBody(ctx context.Context) (http.RequestBody, error) {
    f.seen.withLock(func(v mut Counts) { v.made += 1; if v.active != 0 { v.overlap = true }; v.active += 1 })
    return http.NewUnknownLengthBody(Source{at: 0, fail: f.fail, seen: f.seen}), nil
}
async func serveUpload(listener own net.Listener, mode string) error {
    let raw = listener.Accept()?
    var settings = Array<byte>{0, 4, 0, 0, 0, 0}
    if mode == "source" { settings = Array<byte>{} }
    var p = Peer{conn: raw, decoder: hpack.NewDecoder(4096, 32768)?, encoder: hpack.NewEncoder(), input: Array<byte>{}, log: Array<string>{}, updates: 0, settings: settings}
    p.hello()?
    let first, ended = p.request()?
    if ended { return error("upload ended at headers") }
    if mode == "goaway" {
        p.send(7, 0, 0, Array<byte>{0, 0, 0, 0, 0, 0, 0, 0}[:])?
        return p.goodbye()
    }
    if mode == "source" {
        var total = 0
        for {
            let kind, flags, id, data = p.frame()?
            if kind == 4 || kind == 8 { continue }
            if id != first { return error("source failure replayed") }
            if kind == 3 {
                if int(data[3]) != 8 || total != 3 { return error("source failure reset") }
                break
            }
            if kind != 0 || flags & 1 != 0 { return error("source error ended successfully") }
            total += data.len()
        }
    } else {
        p.send(3, 0, first, number(7)[:])?
        if mode != "once" {
            let retry, _ = p.request()?
            if retry != first + 2 || !strings.Contains(p.log[p.log.len() - 1], ":path=/upload") { return error("retry stream missing") }
            p.send(8, 0, retry, number(3)[:])?
            if p.body(retry)? != 3 { return error("replayed body differs") }
            p.answer(retry, "200", "ok")?
        }
    }
    let sibling, _ = p.request()?
    if !strings.Contains(p.log[p.log.len() - 1], ":path=/sibling") { return error("unexpected replay") }
    p.answer(sibling, "200", "sibling")?
    return p.goodbye()
}
async func replay(conn http.HTTP2Conn, factory own http.RequestBodyFactory, retry http.RetryConfig) (http.ClientResponse, error) {
    return conn.UploadRetry(context.Background(), "POST", "https://example.test/upload", http.NewHeader(), factory, http.DefaultUploadConfig(), retry)
}
func run() error {
    let mode = os.Args()[1]
    let listener = net.Listen("tcp", "127.0.0.1:0")?
    let address = listener.Addr()
    let peer = go serveUpload(listener, mode)
    var config = http.DefaultClientConfig()
    config.Timeout = 2 * time.Second
    let client = http.NewClient(config)?
    let conn = client.NewHTTP2Conn(Plain{conn: net.Dial("tcp", address)?}, http.DefaultHTTP2Config())?
    let seen = mutex(Counts{made: 0, bodies: 0, dropped: 0, active: 0, overlap: false})
    var retry = http.DefaultRetryConfig()
    retry.Backoff = time.Millisecond
    retry.MaxBackoff = 2 * time.Millisecond
    var responses = Array<http.ClientResponse>{}
    var failure error = nil
    if mode == "once" {
        seen.withLock(func(v mut Counts) { v.active += 1 })
        let body = http.NewUnknownLengthBody(Source{at: 0, fail: false, seen: seen})
        let response, err = conn.Upload(context.Background(), "POST", "https://example.test/upload", http.NewHeader(), body, http.DefaultUploadConfig())
        responses.push(response)
        _ = failure
        failure = err
    } else {
        let task = go replay(conn, Factory{seen: seen, fail: mode == "source"}, retry)
        let response, err = task.wait()
        responses.push(response)
        _ = failure
        failure = err
    }
    let _, response = responses.pop()
    if mode == "refused" {
        if failure != nil || response.StatusCode != 200 || strings.FromBytes(response.Body[:])? != "ok" { return error("refusal retry outcome") }
    } else if mode == "source" {
        if failure != error("source failure") { return error("source error replayed or lost") }
    } else if failure != error("http2: request not processed") { return error("unprocessed outcome") }
    let counts = seen.withLock(func(v mut Counts) bool {
        var made = 1
        var dropped = 1
        var bodies = 1
        if mode == "refused" { made = 2; bodies = 2 }
        if mode == "once" { made = 0; dropped = 0 }
        return v.made == made && v.dropped == dropped && v.bodies == bodies && v.active == 0 && !v.overlap
    })
    if !counts { return error("factory/body ownership") }
    if mode != "goaway" {
        let empty = Array<byte>{}
        let sibling = conn.Do(context.Background(), "GET", "https://example.test/sibling", http.NewHeader(), empty[:])?
        if strings.FromBytes(sibling.Body[:])? != "sibling" { return error("sibling failed") }
    }
    conn.Close()
    peer.wait()?
    println("ok")
    return nil
}
func main() { let err = run(); if err != nil { println("failed " + message(err)) } }
"#
    );
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("h2_replay");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    for mode in ["refused", "goaway", "once", "source"] {
        let output = bounded_process::run(
            Command::new(&executable)
                .arg(mode)
                .env("ZORE_CHECK_LEAKS", "1"),
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(output.status.success(), "{mode}: {}", stderr(&output));
        assert!(output.stderr.is_empty(), "{mode}: {}", stderr(&output));
        assert_eq!(stdout(&output), "ok\n", "{mode}");
    }
}

#[test]
fn http_upload_retry_panics_drop_the_source_connection_and_owned_factory_once() {
    let dir = TempDir::new().unwrap();
    for (mode, expected, panic) in [
        (
            "panic",
            "connection dropped\nbody dropped\nfactory dropped\n",
            "injected source panic",
        ),
        (
            "factorypanic",
            "body dropped\nfactory dropped\n",
            "injected factory panic",
        ),
    ] {
        let main = format!(
            r#"func main() {{
    let seen = state()
    let client, err = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{{seen: seen, mode: "panic"}})
    if err != nil {{ panic(errors.Message(err)) }}
    let _, _ = client.UploadRetry(context.Background(), "PUT", "https://example.test/", http.NewHeader(), Factory{{mode: "{mode}", size: 3, seen: seen}}, http.DefaultUploadConfig(), quick())
}}"#
        );
        let source = PROGRAM.replace("func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println(\"ok\") }", &main);
        let executable = dir.executable(mode);
        build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
        let output = bounded_process::run(
            Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(!output.status.success());
        assert_eq!(stdout(&output), expected);
        assert!(stderr(&output).contains(panic), "{}", stderr(&output));
        assert!(!stderr(&output).contains("leak"), "{}", stderr(&output));
    }
}

#[test]
fn http_upload_retry_guide_reopens_a_file_and_reuses_the_connection() {
    let guide = include_str!("../../docs/http-upload-retries.md");
    let source = guide
        .split("```ore\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let target = format!("http://{}/object", listener.local_addr().unwrap());
    let source = source.replace("http://localhost:8080/object", &target);
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("replay_guide");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let payload = b"body-from-file\n".to_vec();
    std::fs::write(directory.path().join("payload.bin"), &payload).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        for status in ["503 Service Unavailable", "200 OK"] {
            let mut head = Vec::new();
            let mut byte = [0];
            while !head.ends_with(b"\r\n\r\n") {
                conn.read_exact(&mut byte).unwrap();
                head.push(byte[0]);
            }
            let text = String::from_utf8(head).unwrap();
            assert!(text.starts_with("PUT /object HTTP/1.1\r\n"));
            assert!(text.contains(&format!("\r\nContent-Length: {}\r\n", payload.len())));
            assert!(!text.contains("Connection: close"));
            let mut body = vec![0; payload.len()];
            conn.read_exact(&mut body).unwrap();
            assert_eq!(body, payload);
            conn.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 2\r\n\r\nok").as_bytes())
                .unwrap();
        }
    });
    let output = bounded_process::run(
        Command::new(&executable)
            .current_dir(directory.path())
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    peer.join().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "200\n");
}
