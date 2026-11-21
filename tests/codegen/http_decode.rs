use super::*;
use std::time::Duration;

const SCRIPTED_DECODING: &str = r#"package main
import "std/compress/gzip"
import "std/context"
import "std/errors"
import "std/io"
import "std/net/http"
import "std/net/url"
import "std/strconv"
import "std/strings"
import "std/time"
type Sink struct { data Mutex<Array<byte>> }
func (s mut Sink) Write(data []byte) (int, error) {
    s.data.withLock(func(out mut Array<byte>) {
        for b in data { out.push(b) }
    })
    return data.len(), nil
}
func gz(text []byte) Array<byte> {
    let data = mutex(Array<byte>{})
    var writer, _ = gzip.NewWriter(Sink{data: data}, gzip.MaxBytes, gzip.MaxBytes)
    let _, _ = writer.Write(text)
    let _ = writer.Close()
    return data.withLock(func(out mut Array<byte>) Array<byte> { return clone(out) })
}
func gzText(text string) Array<byte> {
    let bytes = strings.Bytes(text)
    return gz(bytes[:])
}
func zeros(count int) Array<byte> {
    var out = Array<byte>{}
    for out.len() < count { out.push(0) }
    return out
}
type Conn struct { request string; reply Array<byte>; cursor int; log channel<string> }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) Write(data []byte) (int, error) {
    let text, _ = strings.FromBytes(data)
    c.request += text
    let head, _, found = strings.Cut(c.request, "\r\n\r\n")
    if !found { return data.len(), nil }
    c.log.send(head)
    let line, _, _ = strings.Cut(head, "\r\n")
    let method, rest, _ = strings.Cut(line, " ")
    let path, _, _ = strings.Cut(rest, " ")
    c.reply = answer(method, path)
    c.cursor = 0
    c.request = ""
    return data.len(), nil
}
func (c mut Conn) Read(data mut []byte) (int, error) {
    if c.cursor >= c.reply.len() { return 0, http.ClientPollTimeout }
    var n = 0
    for n < data.len() && c.cursor < c.reply.len() {
        data[n] = c.reply[c.cursor]
        c.cursor += 1
        n += 1
    }
    return n, nil
}
func respond(encoding string, body own Array<byte>, head bool) Array<byte> {
    var text = "HTTP/1.1 200 OK\r\n"
    if encoding != "" { text += encoding }
    text += "Content-Length: " + strconv.Itoa(body.len()) + "\r\n\r\n"
    var out = strings.Bytes(text)
    if !head {
        for b in body { out.push(b) }
    }
    return out
}
func answer(method string, path string) Array<byte> {
    let gzipped = "Content-Encoding: gzip\r\n"
    let head = method == "HEAD"
    if path == "/gzip" { return respond(gzipped, gzText("hello world"), head) }
    if path == "/xgzip" { return respond("Content-Encoding: X-GZIP\r\n", gzText("hello world"), head) }
    if path == "/plain" { return respond("", strings.Bytes("plain"), head) }
    if path == "/identity" { return respond("Content-Encoding: identity\r\n", strings.Bytes("same"), head) }
    if path == "/br" { return respond("Content-Encoding: br\r\n", strings.Bytes("xx"), head) }
    if path == "/double" { return respond("Content-Encoding: gzip, gzip\r\n", gzText("x"), head) }
    if path == "/twice" { return respond("Content-Encoding: gzip\r\nContent-Encoding: gzip\r\n", gzText("x"), head) }
    if path == "/partial" {
        var text = "HTTP/1.1 206 Partial Content\r\nContent-Encoding: gzip\r\nContent-Range: bytes 0-3/40\r\nContent-Length: 4\r\n\r\n"
        var out = strings.Bytes(text)
        let whole = gzText("hello world")
        for var i = 0; i < 4; i += 1 { out.push(whole[i]) }
        return out
    }
    if path == "/chunked" || path == "/chunkedbomb" || path == "/chunkedempty" {
        var packed = gzText("hello world")
        if path == "/chunkedbomb" {
            let big = zeros(1048576)
            packed = gz(big[:])
        }
        var out = strings.Bytes("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n")
        if path != "/chunkedempty" {
            for b in strings.Bytes(strconv.FormatInt(packed.len(), 16) + "\r\n") { out.push(b) }
            for b in packed { out.push(b) }
            for b in strings.Bytes("\r\n") { out.push(b) }
        }
        for b in strings.Bytes("0\r\n\r\n") { out.push(b) }
        return out
    }
    if path == "/moved" { return strings.Bytes("HTTP/1.1 302 Found\r\nLocation: /gzip\r\nContent-Length: 0\r\n\r\n") }
    if path == "/empty" { return respond(gzipped, Array<byte>{}, head) }
    if path == "/bomb" {
        let big = zeros(1048576)
        return respond(gzipped, gz(big[:]), head)
    }
    if path == "/concat" {
        var both = gzText("hello ")
        for b in gzText("world") { both.push(b) }
        return respond(gzipped, both, head)
    }
    if path == "/trailing" {
        var junk = gzText("hello")
        for b in strings.Bytes("xyz") { junk.push(b) }
        return respond(gzipped, junk, head)
    }
    if path == "/corrupt" {
        var bad = gzText("hello world")
        let at = bad.len() - 8
        bad[at] = bad[at] ^ 255
        return respond(gzipped, bad, head)
    }
    if path == "/truncated" {
        let whole = gzText("hello world")
        var cut = Array<byte>{}
        for var i = 0; i < whole.len() - 4; i += 1 { cut.push(whole[i]) }
        return respond(gzipped, cut, head)
    }
    return respond("", strings.Bytes("?"), head)
}
type Transport struct { log channel<string> }
func (t Transport) Dial(ctx context.Context, target url.AbsoluteURL) (http.ClientConn, error) {
    return Conn{request: "", reply: Array<byte>{}, cursor: 0, log: t.log}, nil
}
func check(ok bool, what string) { if !ok { panic(what) } }
func failure(err error) string {
    if err == nil { return "nil" }
    return errors.Message(err)
}
func drain(ch channel<string>) Array<string> {
    var items = Array<string>{}
    for {
        select {
            case let item, _ = ch.receive() { items.push(item) }
            default { return items }
        }
    }
}
func get(client http.Client, method string, path string, config http.DecodeConfig) (http.ClientResponse, error) {
    let empty = Array<byte>{}
    return client.DoDecoded(context.Background(), method, "https://a.test" + path, http.NewHeader(), empty[:], config)
}
func body(response http.ClientResponse) string {
    let text, _ = strings.FromBytes(response.Body[:])
    return text
}
func streamed(client http.Client, method string, path string, config http.DecodeConfig) (string, error) {
    let empty = Array<byte>{}
    var stream = client.OpenDecoded(context.Background(), method, "https://a.test" + path, http.NewHeader(), empty[:], http.DefaultStreamConfig(), config)?
    var buf = Array<byte>{0, 0, 0, 0, 0, 0, 0}
    var total = 0
    var text = ""
    for {
        let n, err = stream.Read(buf[:])
        total += n
        if text.len() < 64 {
            for var i = 0; i < n; i += 1 { text += string(rune(buf[i])) }
        }
        if err == io.EOF { break }
        if err != nil { return "", err }
    }
    let _, complete = stream.Trailers()
    var encoded = ""
    if stream.Headers().Values("Content-Encoding").len() > 0 { encoded = " encoded" }
    if total > 64 { text = strconv.Itoa(total) + " bytes" }
    return text + encoded + " complete=" + strconv.FormatBool(complete), nil
}
func expectStream(client http.Client, path string, config http.DecodeConfig, want string) {
    let got, err = streamed(client, "GET", path, config)
    var line = got
    if err != nil { line = "error " + failure(err) }
    check(line == want, "streamed " + path + ": " + line)
}
func run() error {
    let defaults = http.DefaultDecodeConfig()
    check(defaults.MaxBytes == 16777216 && defaults.MaxRatio == 200, "defaults")
    let log = channel<string>(64)
    let client = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{log: log})?

    let decoded = get(client, "GET", "/gzip", defaults)?
    check(body(decoded) == "hello world", "gzip decoded")
    check(decoded.Headers.Values("Content-Encoding").len() == 0 && decoded.Headers.Values("Content-Length").len() == 0, "encoding headers removed")
    let sent = drain(log)
    check(sent.len() == 1 && strings.Contains(sent[0], "\r\nAccept-Encoding: gzip"), "asks for gzip")
    check(body(get(client, "GET", "/xgzip", defaults)?) == "hello world", "x-gzip decoded")
    let plain = get(client, "GET", "/plain", defaults)?
    check(body(plain) == "plain" && plain.Headers.Values("Content-Length").len() == 1, "plain unchanged")
    let same = get(client, "GET", "/identity", defaults)?
    check(body(same) == "same" && same.Headers.Values("Content-Encoding").len() == 1, "identity unchanged")
    check(body(get(client, "GET", "/concat", defaults)?) == "hello world", "concatenated members")
    let empty = get(client, "GET", "/empty", defaults)?
    check(empty.Body.len() == 0 && empty.Headers.Values("Content-Encoding").len() == 1, "empty body kept")
    let head = get(client, "HEAD", "/gzip", defaults)?
    check(head.Body.len() == 0 && head.Headers.Values("Content-Encoding").len() == 1, "HEAD kept")
    for path in [string; 3]{"/br", "/double", "/twice"} {
        let _, err = get(client, "GET", path, defaults)
        check(failure(err) == "http.Client: unsupported content encoding", path + " " + failure(err))
    }
    let _, corrupt = get(client, "GET", "/corrupt", defaults)
    check(failure(corrupt) == "gzip: checksum mismatch", "corrupt " + failure(corrupt))
    let _, truncated = get(client, "GET", "/truncated", defaults)
    check(failure(truncated) == "gzip: truncated stream", "truncated " + failure(truncated))
    let _, trailing = get(client, "GET", "/trailing", defaults)
    check(failure(trailing) == "gzip: truncated stream", "trailing " + failure(trailing))
    let partial = get(client, "GET", "/partial", defaults)?
    check(partial.StatusCode == 206 && partial.Body.len() == 4 && partial.Headers.Values("Content-Encoding").len() == 1, "206 unchanged")
    let _, bomb = get(client, "GET", "/bomb", defaults)
    check(failure(bomb) == "http.Client: decoded body expands too much", "ratio " + failure(bomb))
    var small = defaults
    small.MaxRatio = 100000
    small.MaxBytes = 524288
    let _, large = get(client, "GET", "/bomb", small)
    check(failure(large) == "http.Client: decoded body too large", "size " + failure(large))
    var roomy = defaults
    roomy.MaxRatio = 100000
    roomy.MaxBytes = 1048576
    let exact = get(client, "GET", "/bomb", roomy)?
    check(exact.Body.len() == 1048576, "exact limit")
    let _ = drain(log)

    var asked = http.NewHeader()
    asked.Set("Accept-Encoding", "br")?
    let nothing = Array<byte>{}
    let _, owned = client.DoDecoded(context.Background(), "GET", "https://a.test/gzip", asked, nothing[:], defaults)
    check(failure(owned) == "http.Client: client owns Accept-Encoding" && drain(log).len() == 0, "caller Accept-Encoding refused")
    var negative = defaults
    negative.MaxRatio = -1
    let _, negativeErr = get(client, "GET", "/gzip", negative)
    check(failure(negativeErr) == "http.Client: negative decode configuration", "negative")
    var huge = defaults
    huge.MaxBytes = gzip.MaxBytes + 1
    let _, hugeErr = get(client, "GET", "/gzip", huge)
    check(failure(hugeErr) == "http.Client: decode limit too large" && drain(log).len() == 0, "limit above gzip")
    let raw = client.Do(context.Background(), "GET", "https://a.test/gzip", http.NewHeader(), nothing[:])?
    check(raw.Headers.Values("Content-Encoding").len() == 1 && raw.Body.len() > 11 && !strings.Contains(drain(log)[0], "Accept-Encoding"), "Do stays raw")

    var accept = http.NewHeader()
    accept.Set("Accept-Encoding", "gzip")?
    let followed, final = client.Follow(context.Background(), "GET", "https://a.test/moved", accept, nothing[:], http.DefaultRedirectConfig())?
    let unpacked = http.Decode(context.Background(), followed, defaults)?
    check(body(unpacked) == "hello world" && final == "https://a.test/gzip", "Decode after Follow")
    let ctx, cancel = context.WithCancel(context.Background())
    cancel()
    let packed = client.Do(context.Background(), "GET", "https://a.test/gzip", accept, nothing[:])?
    let _, cancelled = http.Decode(ctx, packed, defaults)
    check(failure(cancelled) == "context canceled", "Decode stops on cancel")
    let _ = drain(log)

    expectStream(client, "/gzip", defaults, "hello world complete=true")
    let streamSent = drain(log)
    check(streamSent.len() == 1 && strings.Contains(streamSent[0], "\r\nAccept-Encoding: gzip"), "stream asks for gzip")
    expectStream(client, "/xgzip", defaults, "hello world complete=true")
    expectStream(client, "/concat", defaults, "hello world complete=true")
    expectStream(client, "/plain", defaults, "plain complete=true")
    expectStream(client, "/identity", defaults, "same encoded complete=true")
    expectStream(client, "/empty", defaults, " encoded complete=true")
    let headStream, headErr = streamed(client, "HEAD", "/gzip", defaults)
    check(headErr == nil && headStream == " encoded complete=true", "streamed HEAD kept " + headStream)
    for path in [string; 3]{"/br", "/double", "/twice"} {
        expectStream(client, path, defaults, "error http.Client: unsupported content encoding")
    }
    expectStream(client, "/corrupt", defaults, "error gzip: checksum mismatch")
    expectStream(client, "/truncated", defaults, "error gzip: truncated stream")
    expectStream(client, "/trailing", defaults, "error gzip: truncated stream")
    expectStream(client, "/bomb", defaults, "error http.Client: decoded body expands too much")
    expectStream(client, "/bomb", small, "error http.Client: decoded body too large")
    expectStream(client, "/bomb", roomy, "1048576 bytes complete=true")
    var unbounded = defaults
    unbounded.MaxRatio = 4611686018427387903
    unbounded.MaxBytes = 2097152
    expectStream(client, "/bomb", unbounded, "1048576 bytes complete=true")
    expectStream(client, "/chunked", defaults, "hello world complete=true")
    expectStream(client, "/chunkedempty", defaults, " complete=true")
    expectStream(client, "/chunkedbomb", defaults, "1048576 bytes complete=true")
    expectStream(client, "/chunkedbomb", small, "error http.Client: decoded body too large")
    var tiny = small
    tiny.MaxBytes = 10
    var capped = client.OpenDecoded(context.Background(), "GET", "https://a.test/bomb", http.NewHeader(), nothing[:], http.DefaultStreamConfig(), tiny)?
    var seven = Array<byte>{0, 0, 0, 0, 0, 0, 0}
    var cappedTotal = 0
    var cappedErr error = nil
    for cappedErr == nil {
        let n, err = capped.Read(seven[:])
        cappedTotal += n
        cappedErr = err
    }
    check(cappedTotal == 10 && failure(cappedErr) == "http.Client: decoded body too large", "bytes before the limit are kept")
    let streamedPartial = client.OpenDecoded(context.Background(), "GET", "https://a.test/partial", http.NewHeader(), nothing[:], http.DefaultStreamConfig(), defaults)?
    check(streamedPartial.StatusCode() == 206 && streamedPartial.Headers().Values("Content-Encoding").len() == 1, "streamed 206 unchanged")
    let _ = drain(log)
    let _, streamOwned = client.OpenDecoded(context.Background(), "GET", "https://a.test/gzip", asked, nothing[:], http.DefaultStreamConfig(), defaults)
    check(failure(streamOwned) == "http.Client: client owns Accept-Encoding" && drain(log).len() == 0, "streamed caller Accept-Encoding refused")
    let _, streamNegative = client.OpenDecoded(context.Background(), "GET", "https://a.test/gzip", http.NewHeader(), nothing[:], http.DefaultStreamConfig(), negative)
    check(failure(streamNegative) == "http.Client: negative decode configuration" && drain(log).len() == 0, "streamed negative")
    let wrapped = client.Open(context.Background(), "GET", "https://a.test/gzip", accept, nothing[:], http.DefaultStreamConfig())?
    var unwrapped = http.DecodeStream(wrapped, defaults)?
    var piece = Array<byte>{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}
    let unwrappedN, _ = unwrapped.Read(piece[:])
    check(unwrappedN == 11, "DecodeStream on an opened stream")
    let _ = drain(log)

    let pool = http.NewPool(http.NewClientWithTransport(http.DefaultClientConfig(), Transport{log: log})?, http.DefaultPoolConfig())?
    let pooled = pool.DoDecoded(context.Background(), "GET", "https://a.test/gzip", http.NewHeader(), nothing[:], defaults)?
    check(body(pooled) == "hello world" && pool.Stats().Idle == 1, "pooled decode")
    var pooledStream = pool.OpenDecoded(context.Background(), "GET", "https://a.test/gzip", http.NewHeader(), nothing[:], http.DefaultStreamConfig(), defaults)?
    var pooledBuf = Array<byte>{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}
    let pooledN, _ = pooledStream.Read(pooledBuf[:])
    let _, pooledEnd = pooledStream.Read(pooledBuf[:])
    _ = pooledEnd
    check(pooledN == 11 && pooledEnd == io.EOF, "pooled streamed decode")
    pool.Close()
    return nil
}
func main() {
    let err = run()
    if err != nil { panic(errors.Message(err)) }
    println("ok")
}
"#;

#[test]
fn http_decoding_unpacks_gzip_within_size_and_ratio_limits() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("decoding");
    build_file(SCRIPTED_DECODING, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(60),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

const QUARTERLY_REPORT_GZIP: [u8; 36] = [
    31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 43, 44, 77, 44, 42, 73, 45, 202, 169, 84, 40, 74, 45, 200,
    47, 42, 1, 0, 142, 104, 127, 36, 16, 0, 0, 0,
];

#[test]
fn http_decoding_guide_example_unpacks_a_gzip_answer() {
    use std::io::{Read, Write};
    let guide = include_str!("../../docs/http-decoding.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let source = blocks[0].replace(
        "http://localhost:8080/report",
        &format!("http://{address}/report"),
    );
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("guide");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let server = std::thread::spawn(move || {
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
        let text = String::from_utf8_lossy(&request).into_owned();
        assert!(text.contains("\r\nAccept-Encoding: gzip\r\n"), "{text}");
        let mut reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
            QUARTERLY_REPORT_GZIP.len()
        )
        .into_bytes();
        reply.extend_from_slice(&QUARTERLY_REPORT_GZIP);
        stream.write_all(&reply).unwrap();
    });
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "quarterly report\n");
}

#[test]
fn http_streamed_decoding_memory_does_not_grow_with_the_body() {
    let source = r#"package main
import "std/compress/flate"
import "std/compress/gzip"
import "std/context"
import "std/io"
import "std/net/http"
import "std/net/url"
import "std/strconv"
import "std/strings"
import "std/time"
type Sink struct { data Mutex<Array<byte>> }
func (s mut Sink) Write(data []byte) (int, error) {
    s.data.withLock(func(out mut Array<byte>) {
        for b in data { out.push(b) }
    })
    return data.len(), nil
}
type Conn struct { reply Array<byte>; cursor int; sent bool }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) Write(data []byte) (int, error) { c.sent = true; return data.len(), nil }
func (c mut Conn) Read(data mut []byte) (int, error) {
    if !c.sent || c.cursor >= c.reply.len() { return 0, http.ClientPollTimeout }
    var n = 0
    for n < data.len() && c.cursor < c.reply.len() {
        data[n] = c.reply[c.cursor]
        c.cursor += 1
        n += 1
    }
    return n, nil
}
type Transport struct { reply Array<byte> }
func (t Transport) Dial(ctx context.Context, target url.AbsoluteURL) (http.ClientConn, error) {
    return Conn{reply: clone(t.reply), cursor: 0, sent: false}, nil
}
func packed(size int) Array<byte> {
    var data = Array<byte>{}
    var seed = 7
    for data.len() < size {
        seed = (seed * 1103515245 + 12345) % 2147483648
        data.push(byte(seed % 64 + 32))
    }
    let out = mutex(Array<byte>{})
    var writer, _ = gzip.NewWriterLevel(Sink{data: out}, gzip.MaxBytes, gzip.MaxBytes, flate.NoCompression)
    let _, _ = writer.Write(data[:])
    let _ = writer.Close()
    let body = out.withLock(func(o mut Array<byte>) Array<byte> { return clone(o) })
    var reply = strings.Bytes("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: " + strconv.Itoa(body.len()) + "\r\n\r\n")
    for b in body { reply.push(b) }
    return reply
}
func main() {
    waitIdle(0)
    let size = 4194304
    let client, _ = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{reply: packed(size)})
    var config = http.DefaultDecodeConfig()
    config.MaxBytes = 8388608
    let empty = Array<byte>{}
    var stream, err = client.OpenDecoded(context.Background(), "GET", "https://a.test/big", http.NewHeader(), empty[:], http.DefaultStreamConfig(), config)
    if err != nil { panic("open") }
    let before = allocatedBytes()
    var buf = Array<byte>{}
    for buf.len() < 4096 { buf.push(0) }
    var total = 0
    var peak = 0
    for {
        let n, readErr = stream.Read(buf[:])
        total += n
        let used = allocatedBytes() - before
        if used > peak { peak = used }
        if readErr == io.EOF { break }
        if readErr != nil { panic("read") }
    }
    if total != size { panic("size") }
    if peak > 1048576 { panic("memory grew with the body: " + strconv.Itoa(peak)) }
    println("ok")
}
"#;
    let output = run_controlled_one_worker(source);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}
