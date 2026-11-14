use super::*;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POOL_CLIENT: &str = r#"package main
import "std/context"
import "std/errors"
import "std/net/http"
import "std/os"
import "std/strconv"
import "std/strings"
import "std/time"
func message(err error) string {
    if err == nil { return "nil" }
    return errors.Message(err)
}
func stats(pool http.Pool) {
    let s = pool.Stats()
    println("stats " + strconv.Itoa(s.Active) + " " + strconv.Itoa(s.Idle) + " " + strconv.Itoa(s.Waiting))
}
func call(pool http.Pool, ctx context.Context, method string, target string) {
    let response, err = pool.Do(ctx, method, target, http.NewHeader(), Array<byte>{}[:])
    if err != nil {
        println(message(err))
        return
    }
    let text, _ = strings.FromBytes(response.Body[:])
    println(strconv.Itoa(response.StatusCode) + " " + text)
}
async func waiter(pool http.Pool, target string, done channel<string>) {
    let response, err = pool.Do(context.Background(), "GET", target, http.NewHeader(), Array<byte>{}[:])
    if err != nil {
        done.send(message(err))
        return
    }
    let text, _ = strings.FromBytes(response.Body[:])
    done.send(strconv.Itoa(response.StatusCode) + " " + text)
}
func drain(stream mut http.ResponseStream) {
    var text = ""
    var buf = [byte; 2]{0, 0}
    for {
        let n, err = stream.Read(buf[:])
        if err != nil {
            println(text + " " + message(err))
            return
        }
        let piece, _ = strings.FromBytes(buf[:n])
        text += piece
    }
}
func waiting(pool http.Pool, count int) {
    for pool.Stats().Waiting != count { time.Sleep(5 * time.Millisecond) }
}
func run() error {
    let args = os.Args()
    let target = args[1]
    let mode = args[2]
    var settings = http.DefaultPoolConfig()
    if mode == "saturate" || mode == "cancel" || mode == "shutdown" { settings.MaxConnectionsPerHost = 1 }
    if mode == "cancel" { settings.MaxWaiters = 1 }
    if mode == "expire" { settings.IdleTimeout = 200 * time.Millisecond }
    if mode == "global" { settings.MaxConnections = 1 }
    if mode == "idlecap" { settings.MaxIdlePerHost = 1 }
    let client = http.NewClient(http.DefaultClientConfig())?
    let pool = http.NewPool(client, settings)?
    let empty = Array<byte>{}
    let background = context.Background()
    if mode == "head" {
        call(pool, background, "HEAD", target)
        call(pool, background, "HEAD", target)
    } else if mode == "stream" || mode == "early" {
        var stream = pool.Open(background, "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig())?
        if mode == "early" {
            var one = [byte; 1]{0}
            let n, err = stream.Read(one[:])
            println(strconv.Itoa(n) + " " + message(err))
            stream.Close()?
        } else {
            drain(stream)
        }
        stats(pool)
        call(pool, background, "GET", target)
    } else if mode == "saturate" || mode == "shutdown" {
        var first = pool.Open(background, "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig())?
        let done = channel<string>(1)
        go waiter(pool, target, done)
        waiting(pool, 1)
        stats(pool)
        if mode == "shutdown" {
            pool.Close()
            let result, _ = done.receive()
            println(result)
            call(pool, background, "GET", target)
            drain(first)
        } else {
            drain(first)
            let result, _ = done.receive()
            println(result)
        }
    } else if mode == "cancel" {
        var first = pool.Open(background, "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig())?
        let short, stop = context.WithTimeout(background, 200 * time.Millisecond)
        call(pool, short, "GET", target)
        stop()
        stats(pool)
        let done = channel<string>(1)
        go waiter(pool, target, done)
        waiting(pool, 1)
        call(pool, background, "GET", target)
        drain(first)
        let result, _ = done.receive()
        println(result)
    } else if mode == "expire" {
        call(pool, background, "GET", target)
        time.Sleep(400 * time.Millisecond)
        stats(pool)
        call(pool, background, "GET", target)
    } else if mode == "stale" {
        call(pool, background, "GET", target)
        time.Sleep(400 * time.Millisecond)
        call(pool, background, "GET", target)
        call(pool, background, "GET", target)
    } else if mode == "global" {
        call(pool, background, "GET", target)
        call(pool, background, "GET", strings.Replace(target, "127.0.0.1", "localhost", 1))
    } else if mode == "idlecap" {
        var first = pool.Open(background, "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig())?
        var second = pool.Open(background, "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig())?
        drain(first)
        drain(second)
    } else {
        call(pool, background, "GET", target)
        call(pool, background, "GET", target)
    }
    stats(pool)
    pool.Close()
    stats(pool)
    return nil
}
func main() { let err = run(); if err != nil { println(message(err)) } }
"#;

#[derive(Clone)]
struct Reply {
    bytes: Vec<u8>,
    close_after: Option<u64>,
}

fn reply(text: &str) -> Reply {
    Reply {
        bytes: text.as_bytes().to_vec(),
        close_after: None,
    }
}

fn closing(text: &str, delay: u64) -> Reply {
    Reply {
        bytes: text.as_bytes().to_vec(),
        close_after: Some(delay),
    }
}

#[derive(Default)]
struct Log {
    connections: usize,
    requests: Vec<(usize, String)>,
}

fn connection(
    mut stream: TcpStream,
    id: usize,
    replies: Arc<Mutex<Vec<Reply>>>,
    log: Arc<Mutex<Log>>,
) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    loop {
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => request.push(byte[0]),
                _ => return,
            }
        }
        let text = String::from_utf8_lossy(&request).into_owned();
        let next = {
            let mut queue = replies.lock().unwrap();
            if queue.is_empty() {
                None
            } else {
                Some(queue.remove(0))
            }
        };
        log.lock().unwrap().requests.push((id, text));
        let Some(next) = next else {
            return;
        };
        if stream.write_all(&next.bytes).is_err() {
            return;
        }
        if let Some(delay) = next.close_after {
            std::thread::sleep(Duration::from_millis(delay));
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
    }
}

struct Peer {
    url: String,
    log: Arc<Mutex<Log>>,
    stop: Arc<AtomicBool>,
    acceptor: std::thread::JoinHandle<()>,
}

fn start(replies: Vec<Reply>) -> Peer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!(
        "http://127.0.0.1:{}/item",
        listener.local_addr().unwrap().port()
    );
    let replies = Arc::new(Mutex::new(replies));
    let log = Arc::new(Mutex::new(Log::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let acceptor = {
        let (log, stop) = (log.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let id = {
                            let mut log = log.lock().unwrap();
                            log.connections += 1;
                            log.connections
                        };
                        let (replies, log) = (replies.clone(), log.clone());
                        workers.push(std::thread::spawn(move || {
                            connection(stream, id, replies, log)
                        }));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        })
    };
    Peer {
        url,
        log,
        stop,
        acceptor,
    }
}

impl Peer {
    fn finish(self) -> Log {
        self.stop.store(true, Ordering::SeqCst);
        self.acceptor.join().unwrap();
        Arc::try_unwrap(self.log)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap()
    }
}

fn run_case(executable: &Path, mode: &str, replies: Vec<Reply>) -> (String, Log) {
    let peer = start(replies);
    let started = Instant::now();
    let output = bounded_process::run(
        Command::new(executable)
            .args([peer.url.as_str(), mode])
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    let log = peer.finish();
    assert!(started.elapsed() < Duration::from_secs(20), "{mode}");
    assert!(
        output.status.success(),
        "{mode}: {} {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(output.stderr.is_empty(), "{mode}: {}", stderr(&output));
    (stdout(&output), log)
}

fn connections(log: &Log) -> Vec<usize> {
    log.requests.iter().map(|(id, _)| *id).collect()
}

const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";

fn build() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("pool");
    build_file(POOL_CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    (dir, executable)
}

#[test]
fn http_pool_reuses_only_cleanly_framed_keep_alive_responses() {
    let (_dir, executable) = build();
    let chunked = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhe\r\n3\r\nllo\r\n0\r\nX-Done: 1\r\n\r\n";
    let reused = [
        ("pair", vec![reply(OK), reply(OK)], "200 hello\n200 hello\n"),
        (
            "pair",
            vec![reply(chunked), reply(chunked)],
            "200 hello\n200 hello\n",
        ),
        (
            "pair",
            vec![reply("HTTP/1.1 204 No Content\r\n\r\n"), reply(OK)],
            "204 \n200 hello\n",
        ),
        (
            "head",
            vec![
                reply("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n"),
                reply("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n"),
            ],
            "200 \n200 \n",
        ),
        (
            "pair",
            vec![
                reply(
                    "HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: 5\r\n\r\nhello",
                ),
                reply(OK),
            ],
            "200 hello\n200 hello\n",
        ),
    ];
    for (mode, replies, expected) in reused {
        let (output, log) = run_case(&executable, mode, replies);
        assert_eq!(
            output,
            format!("{expected}stats 0 1 0\nstats 0 0 0\n"),
            "{mode}"
        );
        assert_eq!(connections(&log), vec![1, 1], "{expected}");
        for (_, request) in &log.requests {
            assert!(
                !request.to_ascii_lowercase().contains("connection:"),
                "{request}"
            );
        }
    }

    let retired = [
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 5\r\n\r\nhello",
        "HTTP/1.1 200 OK\r\nConnection: Keep-Alive, Close\r\nContent-Length: 5\r\n\r\nhello",
        "HTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\nhello",
        "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhelloEXTRA",
    ];
    for first in retired {
        let (output, log) = run_case(&executable, "pair", vec![reply(first), reply(OK)]);
        assert_eq!(
            output, "200 hello\n200 hello\nstats 0 1 0\nstats 0 0 0\n",
            "{first}"
        );
        assert_eq!(connections(&log), vec![1, 2], "{first}");
    }

    let (output, log) = run_case(
        &executable,
        "pair",
        vec![closing("HTTP/1.1 200 OK\r\n\r\nhello", 0), reply(OK)],
    );
    assert_eq!(output, "200 hello\n200 hello\nstats 0 1 0\nstats 0 0 0\n");
    assert_eq!(connections(&log), vec![1, 2]);

    let (output, log) = run_case(
        &executable,
        "pair",
        vec![
            reply("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nheXY"),
            reply(OK),
        ],
    );
    assert_eq!(
        output,
        "http.Client: expected chunk CRLF\n200 hello\nstats 0 1 0\nstats 0 0 0\n"
    );
    assert_eq!(connections(&log), vec![1, 2]);
}

#[test]
fn http_pool_streams_return_connections_only_after_complete_bodies() {
    let (_dir, executable) = build();
    let (output, log) = run_case(&executable, "stream", vec![reply(OK), reply(OK)]);
    assert_eq!(
        output,
        "hello EOF\nstats 0 1 0\n200 hello\nstats 0 1 0\nstats 0 0 0\n"
    );
    assert_eq!(connections(&log), vec![1, 1]);

    let (output, log) = run_case(&executable, "early", vec![reply(OK), reply(OK)]);
    assert_eq!(
        output,
        "1 nil\nstats 0 0 0\n200 hello\nstats 0 1 0\nstats 0 0 0\n"
    );
    assert_eq!(connections(&log), vec![1, 2]);

    let (output, log) = run_case(&executable, "idlecap", vec![reply(OK), reply(OK)]);
    assert_eq!(output, "hello EOF\nhello EOF\nstats 0 1 0\nstats 0 0 0\n");
    assert_eq!(connections(&log).len(), 2);
}

#[test]
fn http_pool_waiters_saturation_cancellation_and_shutdown() {
    let (_dir, executable) = build();
    let (output, log) = run_case(&executable, "saturate", vec![reply(OK), reply(OK)]);
    assert_eq!(
        output,
        "stats 1 0 1\nhello EOF\n200 hello\nstats 0 1 0\nstats 0 0 0\n"
    );
    assert_eq!(connections(&log), vec![1, 1]);

    let (output, log) = run_case(&executable, "cancel", vec![reply(OK), reply(OK)]);
    assert_eq!(
        output,
        "context deadline exceeded\nstats 1 0 0\nhttp.Pool: too many waiters\nhello EOF\n200 hello\nstats 0 1 0\nstats 0 0 0\n"
    );
    assert_eq!(connections(&log), vec![1, 1]);

    let (output, log) = run_case(&executable, "shutdown", vec![reply(OK), reply(OK)]);
    assert_eq!(
        output,
        "stats 1 0 1\nhttp.Pool: closed\nhttp.Pool: closed\nhello EOF\nstats 0 0 0\nstats 0 0 0\n"
    );
    assert_eq!(connections(&log), vec![1]);

    let (output, log) = run_case(&executable, "global", vec![reply(OK), reply(OK)]);
    assert_eq!(output, "200 hello\n200 hello\nstats 0 1 0\nstats 0 0 0\n");
    assert_eq!(connections(&log), vec![1, 2]);
}

#[test]
fn http_pool_expires_idle_connections_and_never_replays_stale_ones() {
    let (_dir, executable) = build();
    let (output, log) = run_case(&executable, "expire", vec![reply(OK), reply(OK)]);
    assert_eq!(
        output,
        "200 hello\nstats 0 0 0\n200 hello\nstats 0 1 0\nstats 0 0 0\n"
    );
    assert_eq!(connections(&log), vec![1, 2]);

    let (output, log) = run_case(
        &executable,
        "stale",
        vec![closing(OK, 100), reply(OK), reply(OK)],
    );
    assert_eq!(
        output,
        "200 hello\n200 hello\n200 hello\nstats 0 1 0\nstats 0 0 0\n"
    );
    assert_eq!(connections(&log), vec![1, 2, 2]);

    let (output, log) = run_case(
        &executable,
        "stale",
        vec![reply(OK), closing("", 0), reply(OK)],
    );
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines[0], "200 hello", "{output}");
    assert!(
        lines[1] == "http.Pool: reused connection closed" || lines[1].starts_with("net."),
        "{output}"
    );
    assert_eq!(
        &lines[2..],
        ["200 hello", "stats 0 1 0", "stats 0 0 0"],
        "{output}"
    );
    assert_eq!(connections(&log), vec![1, 1, 2]);
}

const SCRIPTED_POOL: &str = r#"package main
import "std/context"
import "std/errors"
import "std/net/http"
import "std/net/url"
import "std/strconv"
import "std/strings"
import "std/time"
type Conn struct { host string; request string; reply Array<byte>; cursor int; drops channel<string> }
func (c mut Conn) drop() { c.drops.send(c.host) }
func (c mut Conn) SetReadDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) SetWriteDeadline(deadline time.Instant) error { return nil }
func (c mut Conn) Write(data []byte) (int, error) {
    if c.host == "panic.test" { panic("injected pool panic") }
    let text, _ = strings.FromBytes(data)
    c.request += text
    if strings.HasSuffix(c.request, "\r\n\r\n") {
        c.request = ""
        c.reply = strings.Bytes("HTTP/1.1 200 OK\r\nContent-Length: " + strconv.Itoa(c.host.len()) + "\r\n\r\n" + c.host)
        c.cursor = 0
    }
    return data.len(), nil
}
func (c mut Conn) Read(data mut []byte) (int, error) {
    if c.cursor >= c.reply.len() { return 0, http.ClientPollTimeout }
    data[0] = c.reply[c.cursor]
    c.cursor += 1
    return 1, nil
}
type Transport struct { dials channel<string>; drops channel<string> }
func (t Transport) Dial(ctx context.Context, target url.AbsoluteURL) (http.ClientConn, error) {
    t.dials.send(target.Hostname)
    return Conn{host: strings.ToLower(target.Hostname), request: "", reply: Array<byte>{}, cursor: 0, drops: t.drops}, nil
}
func check(ok bool, what string) { if !ok { panic(what) } }
func count(ch channel<string>) int {
    var n = 0
    for {
        select {
            case ch.receive() { n += 1 }
            default { return n }
        }
    }
}
func get(pool http.Pool, target string) (string, error) {
    let response = pool.Do(context.Background(), "GET", target, http.NewHeader(), Array<byte>{}[:])?
    return strings.FromBytes(response.Body[:])
}
func newPool(dials channel<string>, drops channel<string>) (http.Pool, error) {
    let client = http.NewClientWithTransport(http.DefaultClientConfig(), Transport{dials: dials, drops: drops})?
    return http.NewPool(client, http.DefaultPoolConfig())
}
func run() error {
    var negative = http.DefaultClientConfig()
    negative.Timeout = -1
    let broken, _ = http.NewClient(negative)
    let _, emptyErr = http.NewPool(broken, http.DefaultPoolConfig())
    check(emptyErr == error("http.NewPool: empty client"), "empty client")
    var limits = http.DefaultPoolConfig()
    limits.MaxWaiters = -1
    let zero, negativeErr = http.NewPool(http.NewClient(http.DefaultClientConfig())?, limits)
    check(negativeErr == error("http.NewPool: negative configuration"), "negative limits")
    let _, zeroErr = get(zero, "http://example.test/")
    check(zeroErr == error("http.Pool: empty pool"), "empty pool")
    check(zero.Stats().Active == 0, "empty stats")
    zero.Close()
    let plain = http.NewPool(http.NewClient(http.DefaultClientConfig())?, http.DefaultPoolConfig())?
    let _, tlsErr = get(plain, "https://example.test/")
    check(tlsErr == error("http.Client: HTTPS requires verified TLS transport"), "https without transport")

    let dials = channel<string>(16)
    let drops = channel<string>(16)
    let pool = newPool(dials, drops)?
    check(get(pool, "https://a.test/")? == "a.test", "first a")
    check(get(pool, "https://a.test/x")? == "a.test", "second a")
    check(count(dials) == 1, "a reused")
    check(get(pool, "https://b.test/")? == "b.test", "b")
    check(get(pool, "https://A.TEST:443/")? == "a.test", "normalized a")
    check(get(pool, "http://a.test:443/")? == "a.test", "plain a")
    check(count(dials) == 2, "b and plain a dialed")
    check(pool.Stats().Idle == 3, "three idle")
    check(count(drops) == 0, "nothing closed yet")

    let otherDials = channel<string>(16)
    let other = newPool(otherDials, drops)?
    check(get(other, "https://a.test/")? == "a.test", "other a")
    check(count(otherDials) == 1 && count(dials) == 0, "pools never share")
    other.Close()
    check(count(drops) == 1, "other pool closed its connection")

    let ctx, cancel = context.WithCancel(context.Background())
    cancel()
    let _, cancelErr = pool.Do(ctx, "GET", "https://c.test/", http.NewHeader(), Array<byte>{}[:])
    check(cancelErr == error("context canceled"), "pre-canceled")
    check(count(dials) == 0, "pre-canceled never dials")

    pool.CloseIdle()
    check(count(drops) == 3 && pool.Stats().Idle == 0, "close idle")
    check(get(pool, "https://a.test/")? == "a.test", "after close idle")
    pool.Close()
    check(count(drops) == 1, "close drops idle")
    let _, closedErr = get(pool, "https://a.test/")
    check(closedErr == error("http.Pool: closed"), "closed pool")
    check(count(dials) == 1, "closed never dials")
    return nil
}
func main() {
    let err = run()
    if err != nil { panic(errors.Message(err)) }
    println("ok")
}
"#;

#[test]
fn http_pool_validates_keys_transports_and_closes_every_connection() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("scripted_pool");
    build_file(SCRIPTED_POOL, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
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
fn http_pool_panic_releases_lease_and_connection() {
    let source = SCRIPTED_POOL
        .replace(
            "func (c mut Conn) drop() { c.drops.send(c.host) }",
            "func (c mut Conn) drop() { c.drops.send(c.host); println(\"connection closed\") }",
        )
        .replace(
            "    let dials = channel<string>(16)",
            "    let dials = channel<string>(16)\n    let _, _ = get(newPool(dials, channel<string>(16))?, \"https://panic.test/\")",
        );
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("scripted_pool");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(!output.status.success());
    assert_eq!(stdout(&output), "connection closed\n");
    assert!(
        stderr(&output).contains("injected pool panic"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("leak"), "{}", stderr(&output));
}

#[test]
fn http_pool_plain_async_and_waiting_calls_progress_on_one_worker() {
    let source = r#"package main
import "std/context"
import "std/errors"
import "std/io"
import "std/net"
import "std/net/http"
import "std/strings"
async func serve(listener own net.Listener, requests int) error {
    let conn = listener.Accept()?
    for var i = 0; i < requests; i += 1 {
        var seen = ""
        var one = Array<byte>{0}
        for !strings.HasSuffix(seen, "\r\n\r\n") {
            let n = conn.Read(one[:])?
            if n == 1 { seen += strings.FromBytes(one[:])? }
        }
        conn.Write(strings.Bytes("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")[:])?
    }
    return nil
}
func fetch(pool http.Pool, target string) error {
    let response = pool.Do(context.Background(), "GET", target, http.NewHeader(), Array<byte>{}[:])?
    if strings.FromBytes(response.Body[:])? != "ok" { return error("bad body") }
    return nil
}
async func fetchAsync(pool http.Pool, target string) error {
    return fetch(pool, target)
}
func run() error {
    waitIdle(0)
    let listener = net.Listen("tcp", "127.0.0.1:0")?
    let target = "http://" + listener.Addr() + "/"
    let server = go serve(listener, 4)
    var settings = http.DefaultPoolConfig()
    settings.MaxConnectionsPerHost = 1
    let pool = http.NewPool(http.NewClient(http.DefaultClientConfig())?, settings)?
    fetch(pool, target)?
    let first = go fetchAsync(pool, target)
    first.wait()?
    var stream = pool.Open(context.Background(), "GET", target, http.NewHeader(), Array<byte>{}[:], http.DefaultStreamConfig())?
    let waiting = go fetchAsync(pool, target)
    var buf = [byte; 8]{0, 0, 0, 0, 0, 0, 0, 0}
    for {
        let _, err = stream.Read(buf[:])
        if err == io.EOF { break }
        if err != nil { return err }
    }
    waiting.wait()?
    server.wait()?
    pool.Close()
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
fn http_pool_guide_example_shares_one_pool_between_tasks() {
    let guide = include_str!("../../docs/http-pooling.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let peer = start(vec![reply(OK), reply(OK), reply(OK)]);
    let source = blocks[0].replace("http://localhost:8080/status", &peer.url);
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("guide");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    let log = peer.finish();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "200\n200\n200\n");
    assert_eq!(log.requests.len(), 3);
    assert!((1..=3).contains(&log.connections));
}
