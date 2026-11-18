use super::*;
use std::time::Duration;

const PROGRAM: &str = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net"
import "std/net/http"
import "std/net/http/hpack"
import "std/os"
import "std/strconv"
import "std/strings"
import "std/time"

type Plain struct { conn net.Conn }
func (p mut Plain) Read(buf mut []byte) (int, error) { return p.conn.Read(buf) }
func (p mut Plain) Write(data []byte) (int, error) { return p.conn.Write(data) }
func (p mut Plain) SetReadDeadline(deadline time.Instant) error { return p.conn.SetReadDeadline(deadline) }
func (p mut Plain) SetWriteDeadline(deadline time.Instant) error { return p.conn.SetWriteDeadline(deadline) }

type Peer struct {
    conn net.Conn
    decoder hpack.Decoder
    encoder hpack.Encoder
    input Array<byte>
    log Array<string>
    updates int
    settings Array<byte>
}

func message(err error) string {
    if err == nil { return "nil" }
    return errors.Message(err)
}

func (p mut Peer) fill(count int) error {
    var buf = Array<byte>{}
    for buf.len() < 16384 { buf.push(0) }
    for p.input.len() < count {
        p.conn.SetReadDeadline(time.Now().Add(5 * time.Second))?
        let n, err = p.conn.Read(buf[:])
        for var i = 0; i < n; i += 1 { p.input.push(buf[i]) }
        if err != nil && p.input.len() < count { return err }
    }
    return nil
}

func (p mut Peer) take(count int) (Array<byte>, error) {
    p.fill(count)?
    var head = Array<byte>{}
    var rest = Array<byte>{}
    for i, value in p.input {
        if i < count { head.push(value) } else { rest.push(value) }
    }
    p.input = rest
    return head, nil
}

func (p mut Peer) frame() (int, int, int, Array<byte>, error) {
    let head, err = p.take(9)
    if err != nil { return 0, 0, 0, Array<byte>{}, err }
    let length = (int(head[0]) << 16) | (int(head[1]) << 8) | int(head[2])
    let id = ((int(head[5]) & 127) << 24) | (int(head[6]) << 16) | (int(head[7]) << 8) | int(head[8])
    let payload, payloadErr = p.take(length)
    if payloadErr != nil { return 0, 0, 0, Array<byte>{}, payloadErr }
    return int(head[3]), int(head[4]), id, payload, nil
}

func (p mut Peer) next() (int, int, int, Array<byte>, error) {
    for {
        let kind, flags, id, payload = p.frame()?
        if kind == 4 && flags == 1 { continue }
        if kind == 8 { p.updates += 1; continue }
        if kind == 3 {
            p.log.push("reset stream " + strconv.Itoa(id) + " code " + strconv.Itoa(int(payload[3])))
            continue
        }
        return kind, flags, id, payload, nil
    }
}

func number(value int) Array<byte> {
    return Array<byte>{byte((value >> 24) & 255), byte((value >> 16) & 255), byte((value >> 8) & 255), byte(value & 255)}
}

func (p mut Peer) quiet(wait time.Duration) error {
    var buf = Array<byte>{}
    for buf.len() < 16384 { buf.push(0) }
    let until = time.Now().Add(wait)
    for time.Now().Before(until) {
        p.conn.SetReadDeadline(until)?
        let n, _ = p.conn.Read(buf[:])
        for var i = 0; i < n; i += 1 { p.input.push(buf[i]) }
    }
    for p.input.len() >= 9 {
        let length = (int(p.input[0]) << 16) | (int(p.input[1]) << 8) | int(p.input[2])
        if p.input.len() < 9 + length { break }
        let kind, _, _, _ = p.frame()?
        if kind == 1 { return error("second stream opened early") }
    }
    return nil
}

func (p mut Peer) updatesWithin(wait time.Duration, id int) (int, error) {
    var buf = Array<byte>{}
    for buf.len() < 16384 { buf.push(0) }
    let until = time.Now().Add(wait)
    for time.Now().Before(until) {
        p.conn.SetReadDeadline(until)?
        let n, _ = p.conn.Read(buf[:])
        for var i = 0; i < n; i += 1 { p.input.push(buf[i]) }
    }
    var granted = 0
    for p.input.len() >= 9 {
        let length = (int(p.input[0]) << 16) | (int(p.input[1]) << 8) | int(p.input[2])
        if p.input.len() < 9 + length { break }
        let kind, _, stream, payload = p.frame()?
        if kind == 8 && stream == id { granted += (int(payload[0]) << 24) | (int(payload[1]) << 16) | (int(payload[2]) << 8) | int(payload[3]) }
    }
    return granted, nil
}

func (p mut Peer) send(kind int, flags int, id int, payload []byte) error {
    var out = Array<byte>{byte((payload.len() >> 16) & 255), byte((payload.len() >> 8) & 255), byte(payload.len() & 255), byte(kind), byte(flags)}
    for value in number(id) { out.push(value) }
    for value in payload { out.push(value) }
    var sent = 0
    for sent < out.len() {
        p.conn.SetWriteDeadline(time.Now().Add(5 * time.Second))?
        sent += p.conn.Write(out[sent:])?
    }
    return nil
}

func (p mut Peer) hello() error {
    let preface = p.take(24)?
    let text = strings.FromBytes(preface[:])?
    if text != "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n" { return error("bad preface") }
    let kind, flags, id, payload = p.frame()?
    if kind != 4 || flags != 0 || id != 0 { return error("expected client SETTINGS") }
    var names = ""
    for var at = 0; at < payload.len(); at += 6 {
        let value = (int(payload[at + 2]) << 24) | (int(payload[at + 3]) << 16) | (int(payload[at + 4]) << 8) | int(payload[at + 5])
        names += strconv.Itoa(int(payload[at + 1])) + "=" + strconv.Itoa(value) + " "
    }
    p.log.push("client settings " + names)
    let settings = clone(p.settings)
    p.send(4, 0, 0, settings[:])?
    let empty = Array<byte>{}
    return p.send(4, 1, 0, empty[:])
}

func (p mut Peer) request() (int, bool, error) {
    let kind, flags, id, payload = p.next()?
    if kind != 1 || flags & 4 == 0 { return 0, false, error("expected HEADERS, got " + strconv.Itoa(kind)) }
    let fields = p.decoder.Decode(payload[:])?
    var line = "stream " + strconv.Itoa(id) + ":"
    for field in fields { line += " " + field.Name + "=" + field.Value }
    p.log.push(line)
    return id, flags & 1 != 0, nil
}

func (p mut Peer) body(id int) (int, error) {
    var total = 0
    for {
        let kind, flags, stream, payload = p.next()?
        if kind != 0 || stream != id { return total, error("expected DATA") }
        total += payload.len()
        if flags & 1 != 0 { return total, nil }
    }
}

func pair(name string, value string) hpack.Field {
    return hpack.Field{Name: name, Value: value, Sensitive: false}
}

func (p mut Peer) head(id int, flags int, fields []hpack.Field) error {
    let block = p.encoder.Encode(fields)
    return p.send(1, flags, id, block[:])
}

func (p mut Peer) answer(id int, status string, text string) error {
    let fields = Array<hpack.Field>{pair(":status", status), pair("content-type", "text/plain")}
    if text == "" {
        return p.head(id, 5, fields[:])
    }
    p.head(id, 4, fields[:])?
    let data = strings.Bytes(text)
    return p.send(0, 1, id, data[:])
}

func (p mut Peer) goodbye() error {
    for {
        let kind, flags, id, payload, err = p.next()
        if err != nil {
            _ = err
            p.log.push("closed")
            return nil
        }
        if kind == 7 {
            p.log.push("goaway code " + strconv.Itoa(int(payload[7])))
        } else if kind == 3 {
            p.log.push("reset stream " + strconv.Itoa(id) + " code " + strconv.Itoa(int(payload[3])))
        } else if kind == 6 && flags == 1 {
            p.log.push("ping ack")
        } else {
            p.log.push("frame " + strconv.Itoa(kind))
        }
    }
}

func script(p mut Peer, mode string) error {
    p.hello()?
    if mode == "basic" {
        let first, _ = p.request()?
        p.answer(first, "200", "hello")?
        let second, ended = p.request()?
        if ended { return error("POST without body") }
        p.log.push("body " + strconv.Itoa(p.body(second)?))
        let created = Array<hpack.Field>{pair(":status", "201"), pair("content-length", "0")}
        p.head(second, 5, created[:])?
        let third, _ = p.request()?
        let sized = Array<hpack.Field>{pair(":status", "200"), pair("content-length", "5")}
        p.head(third, 5, sized[:])?
    } else if mode == "concurrent" || mode == "limit" || mode == "waiters" {
        if mode == "concurrent" {
            let a, _ = p.request()?
            let b, _ = p.request()?
            let c, _ = p.request()?
            let ok = Array<hpack.Field>{pair(":status", "200")}
            p.head(c, 4, ok[:])?
            p.head(a, 4, ok[:])?
            p.head(b, 4, ok[:])?
            let x = strings.Bytes("x")
            let y = strings.Bytes("y")
            p.send(0, 0, a, x[:])?
            p.send(0, 0, b, x[:])?
            p.send(0, 1, c, y[:])?
            p.send(0, 1, b, y[:])?
            p.send(0, 1, a, y[:])?
        } else {
            var count = 3
            if mode == "waiters" { count = 2 }
            for var i = 0; i < count; i += 1 {
                let id, _ = p.request()?
                var hold = 100 * time.Millisecond
                if mode == "waiters" && i == 0 { hold = 1500 * time.Millisecond }
                p.quiet(hold)?
                p.answer(id, "200", "one at a time")?
            }
        }
    } else if mode == "upload" {
        let id, _ = p.request()?
        var total = 0
        for total < 65535 {
            let kind, _, _, payload = p.next()?
            if kind != 0 { return error("expected DATA") }
            total += payload.len()
        }
        p.conn.SetReadDeadline(time.Now().Add(100 * time.Millisecond))?
        var probe = [byte; 1]{0}
        let n, _ = p.conn.Read(probe[:])
        p.log.push("stalled at " + strconv.Itoa(total) + " extra " + strconv.Itoa(n))
        let grant = number(200000)
        p.send(8, 0, 0, grant[:])?
        p.send(8, 0, id, grant[:])?
        total += p.body(id)?
        p.log.push("uploaded " + strconv.Itoa(total))
        p.answer(id, "200", "stored")?
    } else if mode == "download" {
        let id, _ = p.request()?
        let ok = Array<hpack.Field>{pair(":status", "200"), pair("content-length", "300000")}
        p.head(id, 4, ok[:])?
        var chunk = Array<byte>{}
        for chunk.len() < 15000 { chunk.push(byte(chunk.len() % 251)) }
        for var i = 0; i < 20; i += 1 {
            var flags = 0
            if i == 19 { flags = 1 }
            p.send(0, flags, id, chunk[:])?
        }
    } else if mode == "refused" {
        let a, _ = p.request()?
        let b, _ = p.request()?
        let refused = number(7)
        p.send(3, 0, a, refused[:])?
        p.answer(b, "200", "served")?
    } else if mode == "goaway" {
        let a, _ = p.request()?
        let _, _ = p.request()?
        var payload = number(a)
        for value in number(0) { payload.push(value) }
        p.send(7, 0, 0, payload[:])?
        p.answer(a, "200", "last one")?
    } else if mode == "push" {
        let id, _ = p.request()?
        var payload = number(2)
        payload.push(130)
        p.send(5, 4, id, payload[:])?
    } else if mode == "bigframe" {
        let _, _ = p.request()?
        let head = Array<byte>{0, 64, 1, 0, 0, 0, 0, 0, 1}
        p.conn.Write(head[:])?
    } else if mode == "malformed" {
        let cases = Array<Array<hpack.Field>>{
            Array<hpack.Field>{pair(":status", "200"), pair("X-Upper", "1")},
            Array<hpack.Field>{pair("content-type", "text/plain")},
            Array<hpack.Field>{pair(":status", "200"), pair("connection", "close")},
            Array<hpack.Field>{pair(":status", "200"), pair(":path", "/")},
            Array<hpack.Field>{pair(":status", "20")},
        }
        for fields in cases {
            let id, _ = p.request()?
            p.head(id, 5, fields[:])?
        }
        let short, _ = p.request()?
        let sized = Array<hpack.Field>{pair(":status", "200"), pair("content-length", "9")}
        p.head(short, 4, sized[:])?
        let four = strings.Bytes("four")
        p.send(0, 1, short, four[:])?
        let early, _ = p.request()?
        p.send(0, 1, early, four[:])?
        let good, _ = p.request()?
        p.answer(good, "200", "still alive")?
    } else if mode == "hpack" {
        let id, _ = p.request()?
        let block = Array<byte>{128}
        p.send(1, 5, id, block[:])?
    } else if mode == "continuation" {
        let id, _ = p.request()?
        let ping = Array<byte>{1, 2, 3, 4, 5, 6, 7, 8}
        p.send(6, 0, 0, ping[:])?
        let early = Array<hpack.Field>{pair(":status", "103"), pair("link", "</style.css>")}
        p.head(id, 4, early[:])?
        let fields = Array<hpack.Field>{pair(":status", "200"), pair("x-long", strings.Repeat("v", 40)), pair("x-last", "end")}
        let block = p.encoder.Encode(fields[:])
        var first = Array<byte>{3, 0, 0, 0, 0, 16}
        for var i = 0; i < 10; i += 1 { first.push(block[i]) }
        for var i = 0; i < 3; i += 1 { first.push(0) }
        p.send(1, 40, id, first[:])?
        p.send(9, 0, id, block[10:20])?
        p.send(9, 4, id, block[20:])?
        var padded = Array<byte>{2}
        for value in strings.Bytes("padded") { padded.push(value) }
        padded.push(0)
        padded.push(0)
        p.send(0, 8, id, padded[:])?
        let trailers = Array<hpack.Field>{pair("x-checksum", "abc")}
        p.head(id, 5, trailers[:])?
    } else if mode == "cancel" {
        let a, _ = p.request()?
        let b, _ = p.request()?
        p.log.push("answering only stream " + strconv.Itoa(b) + " of " + strconv.Itoa(a))
        p.answer(b, "200", "fast")?
    } else if mode == "bodylimit" {
        let id, _ = p.request()?
        let ok = Array<hpack.Field>{pair(":status", "200")}
        p.head(id, 4, ok[:])?
        var chunk = Array<byte>{}
        for chunk.len() < 1001 { chunk.push(65) }
        p.send(0, 0, id, chunk[:])?
        let next, _ = p.request()?
        p.answer(next, "200", "small")?
    } else if mode == "eof" {
        let _, _ = p.request()?
        p.conn.CloseWrite()?
    } else if mode == "flood" {
        let id, _ = p.request()?
        let ok = Array<hpack.Field>{pair(":status", "200")}
        p.head(id, 4, ok[:])?
        let empty = Array<byte>{}
        for var i = 0; i < 70; i += 1 { p.send(0, 0, id, empty[:])? }
    } else if mode == "strict" {
        let signed, _ = p.request()?
        let plus = Array<hpack.Field>{pair(":status", "200"), pair("content-length", "+5")}
        p.head(signed, 4, plus[:])?
        let five = strings.Bytes("hello")
        p.send(0, 1, signed, five[:])?
        let head, _ = p.request()?
        let ok = Array<hpack.Field>{pair(":status", "200")}
        p.head(head, 4, ok[:])?
        p.send(0, 1, head, five[:])?
        let trailed, _ = p.request()?
        p.head(trailed, 4, ok[:])?
        p.send(0, 0, trailed, five[:])?
        let bad = Array<hpack.Field>{pair("x-t", "bad\u007fvalue")}
        p.head(trailed, 5, bad[:])?
        let good, _ = p.request()?
        p.answer(good, "200", "fine")?
    } else if mode == "lateflood" {
        let id, _ = p.request()?
        p.answer(id, "200", "done")?
        let _, _ = p.request()?
        let empty = Array<byte>{}
        for var i = 0; i < 70; i += 1 { p.send(0, 0, id, empty[:])? }
    } else if mode == "goawayclose" {
        let a, _ = p.request()?
        var payload = number(a)
        for value in number(0) { payload.push(value) }
        p.send(7, 0, 0, payload[:])?
        time.Sleep(100 * time.Millisecond)
        p.answer(a, "200", "draining")?
    } else if mode == "stream" {
        let id, _ = p.request()?
        let sized = Array<hpack.Field>{pair(":status", "200"), pair("content-length", "300000")}
        p.head(id, 4, sized[:])?
        var chunk = Array<byte>{}
        for chunk.len() < 16384 { chunk.push(byte(chunk.len() % 251)) }
        var window = 262144
        var sent = 0
        var paused = false
        for sent < 300000 {
            if window == 0 {
                if !paused {
                    paused = true
                    p.log.push("paused at " + strconv.Itoa(sent))
                    let early = p.updatesWithin(150 * time.Millisecond, id)?
                    p.log.push("updates before reading " + strconv.Itoa(early))
                    window += early
                    continue
                }
                let kind, _, stream, payload = p.frame()?
                if kind == 8 && stream == id {
                    window += (int(payload[0]) << 24) | (int(payload[1]) << 16) | (int(payload[2]) << 8) | int(payload[3])
                }
                continue
            }
            var n = chunk.len()
            if n > window { n = window }
            if n > 300000 - sent { n = 300000 - sent }
            var flags = 0
            if sent + n == 300000 { flags = 1 }
            p.send(0, flags, id, chunk[:n])?
            window -= n
            sent += n
        }
        p.log.push("sent " + strconv.Itoa(sent))
        let trailed, _ = p.request()?
        let ok = Array<hpack.Field>{pair(":status", "200")}
        p.head(trailed, 4, ok[:])?
        let abc = strings.Bytes("abc")
        p.send(0, 0, trailed, abc[:])?
        let sum = Array<hpack.Field>{pair("x-sum", "6")}
        p.head(trailed, 5, sum[:])?
        let early, _ = p.request()?
        p.head(early, 4, ok[:])?
        var thousand = Array<byte>{}
        for thousand.len() < 1000 { thousand.push(66) }
        p.send(0, 0, early, thousand[:])?
        let reset, _ = p.request()?
        p.head(reset, 4, ok[:])?
        let partial = strings.Bytes("partial")
        p.send(0, 0, reset, partial[:])?
        time.Sleep(50 * time.Millisecond)
        let code = number(2)
        p.send(3, 0, reset, code[:])?
        let quiet, _ = p.request()?
        p.head(quiet, 4, ok[:])?
        let long, _ = p.request()?
        let five = Array<hpack.Field>{pair(":status", "200"), pair("content-length", "5")}
        p.head(long, 4, five[:])?
        let ten = strings.Bytes("0123456789")
        p.send(0, 1, long, ten[:])?
        let head, _ = p.request()?
        p.head(head, 5, five[:])?
        let huge, _ = p.request()?
        let hundred = Array<hpack.Field>{pair(":status", "200"), pair("content-length", "100")}
        p.head(huge, 4, hundred[:])?
        let cross, _ = p.request()?
        p.head(cross, 4, ok[:])?
        let twelve = strings.Bytes("abcdefghijkl")
        p.send(0, 1, cross, twelve[:])?
        let last, _ = p.request()?
        p.answer(last, "200", "still fine")?
    } else if mode == "zerowindow" {
        let _, _ = p.request()?
        let zero = number(0)
        p.send(8, 0, 0, zero[:])?
    }
    return p.goodbye()
}

async func serve(listener own net.Listener, mode string) Array<string> {
    let decoder, _ = hpack.NewDecoder(4096, 65536)
    var settings = Array<byte>{}
    if mode == "limit" || mode == "waiters" {
        settings = Array<byte>{0, 3, 0, 0, 0, 1}
    }
    var log = Array<string>{}
    let conn, acceptErr = listener.Accept()
    if acceptErr != nil {
        log.push("accept " + message(acceptErr))
        return log
    }
    var peer = Peer{conn: conn, decoder: decoder, encoder: hpack.NewEncoder(), input: Array<byte>{}, log: Array<string>{}, updates: 0, settings: settings}
    if mode == "nosettings" {
        let ping = Array<byte>{0, 0, 0, 0, 0, 0, 0, 0}
        let sendErr = peer.send(6, 0, 0, ping[:])
        _ = sendErr
        let endErr = peer.goodbye()
        _ = endErr
        if sendErr != nil || endErr != nil { peer.log.push("script failed") }
    } else {
        let err = script(peer, mode)
        if err != nil { peer.log.push("script " + message(err)) }
    }
    peer.log.push("window updates " + strconv.Itoa(peer.updates))
    for line in peer.log { log.push(line) }
    return log
}

func show(conn http.HTTP2Conn, ctx context.Context, method string, path string, body []byte) string {
    var headers = http.NewHeader()
    let _ = headers.Add("X-Trace", "1")
    let response, err = conn.Do(ctx, method, "https://example.test" + path, headers, body)
    if err != nil { return path + " error " + message(err) }
    let text, _ = strings.FromBytes(response.Body[:])
    var line = path + " " + strconv.Itoa(response.StatusCode) + " " + strconv.Itoa(response.Body.len())
    if text.len() <= 40 { line += " " + text }
    for entry in response.Headers.Entries() { line += " " + entry.Name + "=" + entry.Value }
    return line
}

async func fetch(conn http.HTTP2Conn, path string, wait time.Duration, done channel<string>) {
    let ctx, cancel = context.WithTimeout(context.Background(), wait)
    let empty = Array<byte>{}
    done.send(show(conn, ctx, "GET", path, empty[:]))
    cancel()
}

func settle(conn http.HTTP2Conn, count int) {
    let until = time.Now().Add(2 * time.Second)
    for time.Now().Before(until) {
        let stats = conn.Stats()
        if stats.Active + stats.Waiting >= count || stats.Closed { return }
        time.Sleep(time.Millisecond)
    }
}

func drain(stream mut http.ResponseStream) int {
    var buf = Array<byte>{}
    for buf.len() < 7000 { buf.push(0) }
    var total = 0
    for {
        let n, err = stream.Read(buf[:])
        total += n
        if err != nil {
            if err != io.EOF { println("drain " + message(err)) }
            return total
        }
    }
}

func together(conn http.HTTP2Conn, paths []string, waits []time.Duration) {
    let done = channel<string>(8)
    for i, path in paths {
        let before = conn.Stats().Active + conn.Stats().Waiting
        go fetch(conn, path, waits[i], done)
        settle(conn, before + 1)
    }
    var lines = Array<string>{}
    for var i = 0; i < paths.len(); i += 1 {
        let line, _ = done.receive()
        lines.push(line)
    }
    for path in paths {
        for line in lines {
            if strings.HasPrefix(line, path + " ") { println(line) }
        }
    }
}

func run() error {
    let mode = os.Args()[1]
    let listener = net.Listen("tcp4", "127.0.0.1:0")?
    let address = listener.Addr()
    listener.SetDeadline(time.Now().Add(5 * time.Second))?
    let server = go serve(listener, mode)
    var config = http.DefaultClientConfig()
    config.Timeout = 5 * time.Second
    config.MaxRequestBodyBytes = 300000
    if mode == "bodylimit" { config.MaxBodyBytes = 1000 }
    let client = http.NewClient(config)?
    var limits = http.DefaultHTTP2Config()
    if mode == "waiters" {
        limits.MaxStreams = 1
        limits.MaxWaiters = 1
    }
    let conn = client.NewHTTP2Conn(Plain{conn: net.Dial("tcp", address)?}, limits)?
    let ctx = context.Background()
    let empty = Array<byte>{}
    let long = 4 * time.Second
    if mode == "basic" {
        println(show(conn, ctx, "GET", "/a?x=1", empty[:]))
        let ping = strings.Bytes("ping")
        println(show(conn, ctx, "POST", "/b", ping[:]))
        println(show(conn, ctx, "HEAD", "/c", empty[:]))
        let stats = conn.Stats()
        println("stats " + strconv.Itoa(stats.Active) + " " + strconv.Itoa(stats.Waiting))
    } else if mode == "waiters" {
        let done = channel<string>(2)
        go fetch(conn, "/1", long, done)
        settle(conn, 1)
        go fetch(conn, "/2", long, done)
        settle(conn, 2)
        println(show(conn, ctx, "GET", "/3", empty[:]))
        let first, _ = done.receive()
        let second, _ = done.receive()
        if strings.HasPrefix(first, "/1 ") { println(first); println(second) } else { println(second); println(first) }
    } else if mode == "concurrent" || mode == "limit" {
        let paths = Array<string>{"/1", "/2", "/3"}
        let waits = Array<time.Duration>{long, long, long}
        together(conn, paths[:], waits[:])
    } else if mode == "upload" {
        var data = Array<byte>{}
        for data.len() < 200000 { data.push(byte(data.len() % 7)) }
        println(show(conn, ctx, "PUT", "/big", data[:]))
    } else if mode == "download" {
        println(show(conn, ctx, "GET", "/large", empty[:]))
    } else if mode == "refused" || mode == "goaway" {
        let paths = Array<string>{"/first", "/second"}
        let waits = Array<time.Duration>{long, long}
        together(conn, paths[:], waits[:])
        if mode == "goaway" {
            println(show(conn, ctx, "GET", "/after", empty[:]))
        }
    } else if mode == "malformed" {
        for var i = 0; i < 8; i += 1 {
            println(show(conn, ctx, "GET", "/m" + strconv.Itoa(i), empty[:]))
        }
    } else if mode == "cancel" {
        let paths = Array<string>{"/slow", "/fast"}
        let waits = Array<time.Duration>{200 * time.Millisecond, long}
        together(conn, paths[:], waits[:])
    } else if mode == "bodylimit" {
        println(show(conn, ctx, "GET", "/huge", empty[:]))
        println(show(conn, ctx, "GET", "/small", empty[:]))
    } else if mode == "strict" {
        println(show(conn, ctx, "GET", "/signed", empty[:]))
        println(show(conn, ctx, "HEAD", "/head", empty[:]))
        println(show(conn, ctx, "GET", "/trailer", empty[:]))
        println(show(conn, ctx, "GET", "/good", empty[:]))
    } else if mode == "lateflood" {
        println(show(conn, ctx, "GET", "/done", empty[:]))
        time.Sleep(20 * time.Millisecond)
        println(show(conn, ctx, "GET", "/later", empty[:]))
    } else if mode == "goawayclose" {
        let done = channel<string>(1)
        go fetch(conn, "/draining", long, done)
        let until = time.Now().Add(2 * time.Second)
        for !conn.Stats().Closed && time.Now().Before(until) { time.Sleep(time.Millisecond) }
        println("closed " + strconv.FormatBool(conn.Stats().Closed))
        conn.Close()
        println(show(conn, ctx, "GET", "/refused", empty[:]))
        let line, _ = done.receive()
        println(line)
    } else if mode == "stream" {
        var config = http.DefaultStreamConfig()
        var first = conn.Open(ctx, "GET", "https://example.test/big", http.NewHeader(), empty[:], config)?
        time.Sleep(1000 * time.Millisecond)
        println("streamed " + strconv.Itoa(first.StatusCode()) + " " + strconv.Itoa(drain(first)))
        var second = conn.Open(ctx, "GET", "https://example.test/trailed", http.NewHeader(), empty[:], config)?
        let size = drain(second)
        let trailers, complete = second.Trailers()
        let sum, _ = trailers.Get("x-sum")
        println("trailed " + strconv.Itoa(size) + " x-sum=" + sum + " complete=" + strconv.FormatBool(complete))
        var third = conn.Open(ctx, "GET", "https://example.test/early", http.NewHeader(), empty[:], config)?
        var some = Array<byte>{0, 0, 0, 0, 0, 0, 0, 0, 0, 0}
        let got, _ = third.Read(some[:])
        println("early read " + strconv.Itoa(got))
        third.Close()?
        var fourth = conn.Open(ctx, "GET", "https://example.test/reset", http.NewHeader(), empty[:], config)?
        var buf = Array<byte>{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}
        var text = ""
        var failure = ""
        for failure == "" {
            let n, err = fourth.Read(buf[:])
            for var i = 0; i < n; i += 1 { text += string(rune(buf[i])) }
            if err != nil { failure = message(err) }
        }
        println("reset read " + text + " then " + failure)
        var idle = http.DefaultStreamConfig()
        idle.IdleTimeout = 200 * time.Millisecond
        var fifth = conn.Open(ctx, "GET", "https://example.test/quiet", http.NewHeader(), empty[:], idle)?
        let _, quietErr = fifth.Read(buf[:])
        println("quiet " + message(quietErr))
        var sixth = conn.Open(ctx, "GET", "https://example.test/long", http.NewHeader(), empty[:], config)?
        let longN, longErr = sixth.Read(buf[:])
        println("long read " + strconv.Itoa(longN) + " then " + message(longErr))
        let headStream = conn.Open(ctx, "HEAD", "https://example.test/head", http.NewHeader(), empty[:], config)?
        let _, headComplete = headStream.Trailers()
        println("head " + strconv.Itoa(headStream.StatusCode()) + " complete=" + strconv.FormatBool(headComplete))
        var small = http.DefaultStreamConfig()
        small.MaxBodyBytes = 10
        let _, hugeErr = conn.Open(ctx, "GET", "https://example.test/huge", http.NewHeader(), empty[:], small)
        println("huge " + message(hugeErr))
        var crossing = conn.Open(ctx, "GET", "https://example.test/cross", http.NewHeader(), empty[:], small)?
        let crossN, crossErr = crossing.Read(buf[:])
        let afterN, afterErr = crossing.Read(buf[:])
        println("cross read " + strconv.Itoa(crossN) + " " + message(crossErr) + " then " + strconv.Itoa(afterN) + " " + message(afterErr))
        println(show(conn, ctx, "GET", "/last", empty[:]))
    } else if mode == "validation" {
        println(show(conn, ctx, "CONNECT", "/x", empty[:]))
        var data = Array<byte>{}
        for data.len() < 300001 { data.push(0) }
        println(show(conn, ctx, "PUT", "/x", data[:]))
        var headers = http.NewHeader()
        let _ = headers.Add("TE", "trailers")
        let _, teErr = conn.Do(ctx, "GET", "https://example.test/", headers, empty[:])
        println(message(teErr))
        var large = http.NewHeader()
        let _ = large.Add("X-Large", strings.Repeat("a", 40000))
        let _, largeErr = conn.Do(ctx, "GET", "https://example.test/", large, empty[:])
        println(message(largeErr))
        let _, urlErr = conn.Do(ctx, "GET", "/relative", http.NewHeader(), empty[:])
        println(message(urlErr))
        let _, configErr = client.NewHTTP2Conn(Plain{conn: net.Dial("tcp", address)?}, http.HTTP2Config{MaxStreams: 0 - 1, StreamWindow: 0, MaxWaiters: 0})
        println(message(configErr))
        let _, rangeErr = client.NewHTTP2Conn(Plain{conn: net.Dial("tcp", address)?}, http.HTTP2Config{MaxStreams: 0, StreamWindow: 100, MaxWaiters: 0})
        println(message(rangeErr))
        var huge = http.DefaultClientConfig()
        huge.MaxHeaderBytes = 16777217
        let wide = http.NewClient(huge)?
        let _, wideErr = wide.NewHTTP2Conn(Plain{conn: net.Dial("tcp", address)?}, http.DefaultHTTP2Config())
        println(message(wideErr))
    } else if mode == "continuation" {
        println(show(conn, ctx, "GET", "/only", empty[:]))
    } else {
        println(show(conn, ctx, "GET", "/only", empty[:]))
        println(show(conn, ctx, "GET", "/again", empty[:]))
    }
    conn.Close()
    println(show(conn, ctx, "GET", "/closed", empty[:]))
    for !conn.Stats().Closed { time.Sleep(time.Millisecond) }
    println("--- server")
    for line in server.wait() { println(line) }
    return nil
}

func main() { let err = run(); if err != nil { println("failed " + message(err)) } }
"#;

fn run(executable: &Path, mode: &str) -> String {
    let output = bounded_process::run(
        Command::new(executable)
            .arg(mode)
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(60),
    )
    .unwrap();
    assert!(output.status.success(), "{mode}: {}", stderr(&output));
    assert!(output.stderr.is_empty(), "{mode}: {}", stderr(&output));
    stdout(&output)
}

#[test]
fn http2_connection_multiplexes_requests_over_a_scripted_peer() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("http2");
    build_file(PROGRAM, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let expected = include_str!("fixtures/http2_scripted.txt").replace("\r\n", "\n");
    let mut cases = 0;
    for case in expected.split("=== ").skip(1) {
        let (mode, output) = case.split_once('\n').unwrap();
        assert_eq!(run(&executable, mode), output, "{mode}");
        cases += 1;
    }
    assert_eq!(cases, 24);
}
