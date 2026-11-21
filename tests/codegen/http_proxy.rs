use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PROXY_CLIENT: &str = r#"package main
import "std/context"
import "std/errors"
import "std/net"
import "std/net/http"
import "std/net/url"
import "std/os"
import "std/strings"
import "std/time"
type Tunnel struct { hosts channel<string> }
type Plain struct { conn net.Conn }
func (p mut Plain) Read(buf mut []byte) (int, error) { return p.conn.Read(buf) }
func (p mut Plain) Write(data []byte) (int, error) { return p.conn.Write(data) }
func (p mut Plain) SetReadDeadline(deadline time.Instant) error { return p.conn.SetReadDeadline(deadline) }
func (p mut Plain) SetWriteDeadline(deadline time.Instant) error { return p.conn.SetWriteDeadline(deadline) }
func (t Tunnel) Tunnel(ctx context.Context, conn own net.Conn, target url.AbsoluteURL) (http.ClientConn, error) {
    t.hosts.send(target.Hostname)
    return Plain{conn: conn}, nil
}
func message(err error) string {
    if err == nil { return "nil" }
    return errors.Message(err)
}
func show(client http.Client, target string, headers http.Header) {
    let empty = Array<byte>{}
    let response, err = client.Do(context.Background(), "GET", target, headers, empty[:])
    if err != nil {
        println(message(err))
        return
    }
    let text, _ = strings.FromBytes(response.Body[:])
    println(text)
}
func run() error {
    let args = os.Args()
    let proxy = http.ProxyConfig{URL: args[1], Authorization: "Basic cHJveHk6c2VjcmV0"}
    let mode = args[2]
    var config = http.DefaultClientConfig()
    config.Timeout = 3 * time.Second
    if mode == "plain" {
        let client = http.NewProxyClient(config, proxy)?
        show(client, "http://target.test:8080/path?q=1", http.NewHeader())
        show(client, "https://secure.test/", http.NewHeader())
        var owned = http.NewHeader()
        owned.Set("Proxy-Authorization", "Basic other")?
        show(client, "http://target.test/", owned)
    } else if mode == "tunnel" || mode == "refused" {
        let hosts = channel<string>(4)
        let client = http.NewProxyClientWithTunnel(config, proxy, Tunnel{hosts: hosts})?
        show(client, "https://secure.test/inside", http.NewHeader())
        select {
            case let host, _ = hosts.receive() { println("tunnel " + host) }
            default { println("no tunnel") }
        }
    } else if mode == "pool" {
        let pool = http.NewPool(http.NewProxyClient(config, proxy)?, http.DefaultPoolConfig())?
        let empty = Array<byte>{}
        for var i = 0; i < 2; i += 1 {
            let response = pool.Do(context.Background(), "GET", "http://target.test/again", http.NewHeader(), empty[:])?
            let text, _ = strings.FromBytes(response.Body[:])
            println(text)
        }
        pool.Close()
    } else if mode == "exchange" {
        let client = http.NewProxyClient(config, proxy)?
        let address, _ = url.ParseAbsolute(args[1])
        let conn = net.Dial("tcp", address.Address)?
        let empty = Array<byte>{}
        let response = client.Exchange(context.Background(), Plain{conn: conn}, "GET", "http://origin.test/direct", http.NewHeader(), empty[:])?
        let text, _ = strings.FromBytes(response.Body[:])
        println(text)
    } else if mode == "preflight" {
        config.MaxHeaderBytes = 512
        let huge = http.ProxyConfig{URL: args[1], Authorization: strings.Repeat("x", 600)}
        let client = http.NewProxyClientWithTunnel(config, huge, Tunnel{hosts: channel<string>(4)})?
        show(client, "https://secure.test/", http.NewHeader())
        show(client, "http://target.test/", http.NewHeader())
    } else if mode == "invalid" {
        for bad in [string; 4]{"https://proxy.test/", "http://proxy.test/path", "ftp://proxy.test/", "not a url"} {
            let _, err = http.NewProxyClient(config, http.ProxyConfig{URL: bad, Authorization: ""})
            println(message(err))
        }
        let _, authErr = http.NewProxyClient(config, http.ProxyConfig{URL: args[1], Authorization: "a\r\nb"})
        println(message(authErr))
    }
    return nil
}
func main() { let err = run(); if err != nil { println(message(err)) } }
"#;

fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return None,
        }
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}

fn answer(stream: &mut TcpStream, head: &str, tunneled: bool) -> bool {
    let line = head.lines().next().unwrap_or("");
    let authorized = head.contains("\r\nProxy-Authorization: Basic cHJveHk6c2VjcmV0\r\n");
    let host = head
        .lines()
        .find_map(|l| l.strip_prefix("Host: "))
        .unwrap_or("");
    let body = format!("{line} host={host} auth={authorized} tunneled={tunneled}");
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(reply.as_bytes()).is_ok()
}

fn serve(mut stream: TcpStream, refuse: bool, log: Arc<Mutex<Vec<String>>>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut tunneled = false;
    while let Some(head) = read_head(&mut stream) {
        log.lock().unwrap().push(head.clone());
        if head.starts_with("CONNECT ") {
            if refuse {
                let _ = stream.write_all(
                    b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
                );
                return;
            }
            if stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .is_err()
            {
                return;
            }
            tunneled = true;
            continue;
        }
        if !answer(&mut stream, &head, tunneled) {
            return;
        }
    }
}

fn run_case(executable: &Path, mode: &str, refuse: bool) -> (String, Vec<String>, usize) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let log = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let connections = Arc::new(Mutex::new(0usize));
    let acceptor = {
        let (log, stop, connections) = (log.clone(), stop.clone(), connections.clone());
        std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        *connections.lock().unwrap() += 1;
                        let log = log.clone();
                        workers.push(std::thread::spawn(move || serve(stream, refuse, log)));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        })
    };
    let output = bounded_process::run(
        Command::new(executable)
            .args([url.as_str(), mode])
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    stop.store(true, Ordering::SeqCst);
    acceptor.join().unwrap();
    assert!(output.status.success(), "{mode}: {}", stderr(&output));
    assert!(output.stderr.is_empty(), "{mode}: {}", stderr(&output));
    let log = log.lock().unwrap().clone();
    let count = *connections.lock().unwrap();
    (stdout(&output), log, count)
}

#[test]
fn http_proxies_forward_tunnel_and_keep_credentials_with_the_proxy() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("proxy");
    build_file(PROXY_CLIENT, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));

    let (output, log, connections) = run_case(&executable, "plain", false);
    assert_eq!(
        output,
        "GET http://target.test:8080/path?q=1 HTTP/1.1 host=target.test:8080 auth=true tunneled=false\n\
         http.Client: HTTPS requires verified TLS transport\n\
         http.Client: client owns Proxy-Authorization\n"
    );
    assert_eq!((log.len(), connections), (1, 1), "{log:?}");

    let (output, log, _) = run_case(&executable, "tunnel", false);
    assert_eq!(
        output,
        "GET /inside HTTP/1.1 host=secure.test auth=false tunneled=true\ntunnel secure.test\n"
    );
    assert!(
        log[0].starts_with("CONNECT secure.test:443 HTTP/1.1\r\nHost: secure.test:443\r\n"),
        "{log:?}"
    );
    assert!(log[0].contains("\r\nProxy-Authorization: Basic cHJveHk6c2VjcmV0\r\n"));
    assert!(!log[1].contains("Proxy-Authorization"), "{log:?}");

    let (output, log, _) = run_case(&executable, "refused", true);
    assert_eq!(
        output,
        "http.Client: proxy refused tunnel with status 407\nno tunnel\n"
    );
    assert_eq!(log.len(), 1);

    let (output, log, connections) = run_case(&executable, "pool", false);
    let line = "GET http://target.test/again HTTP/1.1 host=target.test auth=true tunneled=false";
    assert_eq!(output, format!("{line}\n{line}\n"));
    assert_eq!((log.len(), connections), (2, 1));

    let (output, log, _) = run_case(&executable, "exchange", false);
    assert_eq!(
        output,
        "GET /direct HTTP/1.1 host=origin.test auth=false tunneled=false\n"
    );
    assert!(!log[0].contains("Proxy-Authorization"), "{log:?}");

    let (output, log, connections) = run_case(&executable, "preflight", false);
    assert_eq!(output, "http.Client: metadata too large\n".repeat(2));
    assert_eq!((log.len(), connections), (0, 0));

    let (output, log, connections) = run_case(&executable, "invalid", false);
    assert_eq!(
        output,
        "http.Client: HTTPS proxy requires verified TLS transport\n".to_owned()
            + &"http.Client: invalid proxy URL\n".repeat(3)
            + "http.Client: invalid proxy authorization\n"
    );
    assert_eq!((log.len(), connections), (0, 0));
}

#[test]
fn http_proxy_guide_example_sends_absolute_requests_to_the_proxy() {
    let guide = include_str!("../../docs/http-proxies.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let source = blocks[0].replace("http://proxy.example:3128/", &format!("http://{address}/"));
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("guide");
    build_file(&source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let head = read_head(&mut stream).unwrap();
        assert!(
            head.starts_with("GET http://example.test/status HTTP/1.1\r\nHost: example.test\r\n"),
            "{head}"
        );
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
            .unwrap();
    });
    let output = bounded_process::run(
        Command::new(&executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(20),
    )
    .unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "204\n");
}

#[test]
fn owned_proxy_transport_works_with_one_worker_and_pool_leases() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("owned-proxy");
    let mut source = PROXY_CLIENT.replace(
        "func main() { let err = run(); if err != nil { println(message(err)) } }",
        "func main() { waitPending(0); let done = channel<error>(1); go oneWorker(done); let err, _ = done.receive(); if err != nil { println(message(err)) } }",
    ).replace("URL: args[1], Authorization:", "URL: \"https://\" + strings.TrimPrefix(args[1], \"http://\"), Authorization:");
    source.push_str(r#"
async func oneWorker(done channel<error>) { done.send(run()) }
type OwnedTransport struct {}
func (t OwnedTransport) WrapProxy(ctx context.Context, conn own net.Conn, proxy url.AbsoluteURL) (http.ProxyConn, error) { return conn, nil }
func (t OwnedTransport) Tunnel(ctx context.Context, conn own http.ProxyConn, target url.AbsoluteURL) (http.ClientConn, error) { return conn, nil }
"#);
    source = source
        .replace(
            "http.NewProxyClient(config, proxy)?",
            "http.NewProxyClientWithTransport(config, proxy, OwnedTransport{})?",
        )
        .replace(
            "http.NewProxyClientWithTunnel(config, proxy, Tunnel{hosts: hosts})?",
            "http.NewProxyClientWithTransport(config, proxy, OwnedTransport{})?",
        );
    controlled_runtime::build_with_workers(&source, &executable, 1);
    let (output, log, count) = run_case(&executable, "pool", false);
    assert_eq!(output.lines().count(), 2, "{output}");
    assert_eq!((log.len(), count), (2, 1));
    let (output, log, _) = run_case(&executable, "tunnel", false);
    assert_eq!(
        output,
        "GET /inside HTTP/1.1 host=secure.test auth=false tunneled=true\nno tunnel\n"
    );
    assert_eq!(log.len(), 2);
}
