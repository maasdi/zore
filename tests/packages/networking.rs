#[test]
fn public_addresses_dns_udp_and_uri_resolution_check_without_native_linking() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/net"
import "std/net/url"
import "std/context"
async func exchange(ctx context.Context, conn own net.UDPConn) error {
    let addresses = net.LookupIP(ctx, "ip", "localhost")?
    let endpoint = net.NewAddrPort(addresses[0], 8080)?
    var buffer = Array<byte>{0, 0}
    let count, peer, truncated, err = conn.ReadFrom(buffer[:])
    if err != nil { return err }
    if !truncated && peer.IsValid() { conn.WriteTo(buffer[:count], endpoint)? }
    return conn.Close()
}
func run() error {
    let parse = net.ParseIP
    let ip = parse("127.0.0.1")?
    let same = net.ParseIP(ip.String())?
    println(ip.Equal(same))
    println(ip.Compare(same))
    let endpoint = net.NewAddrPort(ip, 0)?
    let copy = net.ParseAddrPort(endpoint.String())?
    println(endpoint.Equal(copy))
    println(endpoint.Compare(copy))
    let resolved = url.ResolveReference("https://a.test/a", "../b")?
    let absolute = url.ParseAbsolute(resolved)?
    println(absolute.RequestTarget)
    return nil
}
func main() { let _ = run() }
"#,
    )]);
}

#[test]
fn network_value_and_datagram_apis_reject_wrong_types_and_private_storage() {
    for (body, expected) in [
        ("println(net.ParseIP(4))", "mismatched types"),
        (
            "let ip = net.ParseIP(\"127.0.0.1\")?; println(ip.text)",
            "not exported",
        ),
        (
            "let ap = net.ParseAddrPort(\"127.0.0.1:1\")?; println(ap.port)",
            "not exported",
        ),
        (
            "let ap = net.ParseAddrPort(\"127.0.0.1:1\")?; let conn = net.ListenUDP(\"udp4\", ap)?; println(conn.id)",
            "not exported",
        ),
        (
            "let ap = net.ParseAddrPort(\"127.0.0.1:1\")?; let conn = net.ListenUDP(\"udp4\", ap)?; let _ = conn.Close(); println(conn.LocalAddr().String())",
            "moved",
        ),
        (
            "let ap = net.ParseAddrPort(\"127.0.0.1:1\")?; let conn = net.ListenUDP(\"udp4\", ap)?; conn.WriteTo(\"text\", ap)?",
            "mismatched types",
        ),
        (
            "let _ = net.LookupIP(context.Background(), 4, \"localhost\")",
            "mismatched types",
        ),
        ("let _ = url.ResolveReference(1, \"a\")", "mismatched types"),
    ] {
        let imports = ["net", "net/url", "context"]
            .into_iter()
            .filter(|path| body.contains(&format!("{}.", path.rsplit('/').next().unwrap())))
            .map(|path| format!("import \"std/{path}\"\n"))
            .collect::<String>();
        let source = format!(
            "package main\n{imports}func run() error {{ {body}; return nil }}\nfunc main() {{ let _ = run() }}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn streamed_http_responses_check_as_owned_readers() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/context"
import "std/io"
import "std/net/http"
import "std/os"
func download(ctx context.Context, client http.Client, target string, path string) error {
    let empty = Array<byte>{}
    var config = http.DefaultStreamConfig()
    config.MaxBodyBytes = 1 << 30
    var stream = client.Open(ctx, "GET", target, http.NewHeader(), empty[:], config)?
    if stream.StatusCode() != 200 { return stream.Close() }
    var file = os.Create(path)?
    io.Copy(file, stream)?
    let trailers, complete = stream.Trailers()
    if complete { for value in trailers.Values("X-Sum") { println(value) } }
    file.Close()?
    var body io.ReadCloser = stream
    return body.Close()
}
func main() {
    let client, err = http.NewClient(http.DefaultClientConfig())
    if err == nil { let _ = download(context.Background(), client, "http://localhost/", "out.bin") }
}
"#,
    )]);
}

#[test]
fn streamed_http_responses_reject_private_storage_reuse_and_wrong_types() {
    let open = "let client = http.NewClient(http.DefaultClientConfig())?; let empty = Array<byte>{}; var stream = client.Open(context.Background(), \"GET\", \"http://a.test/\", http.NewHeader(), empty[:], http.DefaultStreamConfig())?";
    for (body, expected) in [
        (format!("{open}; println(stream.status)"), "not exported"),
        (format!("{open}; println(stream.streams.len())"), "not exported"),
        (format!("{open}; stream.Close()?; println(stream.StatusCode())"), "moved"),
        (format!("{open}; let copy = stream; println(stream.StatusCode())"), "moved"),
        (format!("{open}; let n = stream.Read(\"text\")?; println(n)"), "mismatched types"),
        (
            "let client = http.NewClient(http.DefaultClientConfig())?; let empty = Array<byte>{}; let _ = client.Open(context.Background(), \"GET\", \"http://a.test/\", http.NewHeader(), empty[:], 5)".to_owned(),
            "mismatched types",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/context\"\nimport \"std/net/http\"\nfunc run() error {{ {body}; return nil }}\nfunc main() {{ let _ = run() }}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn streamed_http_uploads_check_as_owned_request_bodies() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/context"
import "std/net/http"
import "std/os"
func upload(client http.Client, path string, target string) (int, error) {
    let info = os.Stat(path)?
    let file = os.Open(path)?
    let body = http.NewRequestBody(file, info.Size())?
    println(body.Len())
    var config = http.DefaultUploadConfig()
    config.MaxBodyBytes = 1 << 30
    let response = client.Upload(context.Background(), "PUT", target, http.NewHeader(), body, config)?
    return response.StatusCode, nil
}
func main() {
    let client, err = http.NewClient(http.DefaultClientConfig())
    if err == nil { let _, _ = upload(client, "in.bin", "http://localhost/upload") }
}
"#,
    )]);
}

#[test]
fn streamed_http_uploads_reject_private_storage_reuse_and_wrong_types() {
    let open = "let client = http.NewClient(http.DefaultClientConfig())?; let file = os.Open(\"in.bin\")?; let body = http.NewRequestBody(file, 3)?";
    let send = "client.Upload(context.Background(), \"PUT\", \"http://a.test/\", http.NewHeader(), body, http.DefaultUploadConfig())";
    for (body, expected) in [
        (format!("{open}; println(body.length)"), "not exported"),
        (format!("{open}; let _ = body.source"), "not exported"),
        (
            format!("{open}; let _, _ = {send}; println(body.Len())"),
            "moved",
        ),
        (
            format!("{open}; let _, _ = {send}; let _, _ = {send}"),
            "moved",
        ),
        (
            "let _, _ = http.NewRequestBody(\"text\", 3)".to_owned(),
            "does not satisfy `io.Reader`",
        ),
        (
            format!(
                "{open}; let _, _ = client.Upload(context.Background(), \"PUT\", \"http://a.test/\", http.NewHeader(), body, 5)"
            ),
            "mismatched types",
        ),
    ] {
        let imports = ["context", "net/http", "os"]
            .into_iter()
            .filter(|path| body.contains(&format!("{}.", path.rsplit('/').next().unwrap())))
            .map(|path| format!("import \"std/{path}\"\n"))
            .collect::<String>();
        let source = format!(
            "package main\n{imports}func run() error {{ {body}; return nil }}\nfunc main() {{ let _ = run() }}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn pooled_http_clients_check_as_shared_handles() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/context"
import "std/io"
import "std/net/http"
import "std/time"
async func worker(pool http.Pool, target string, done channel<int>) {
    let empty = Array<byte>{}
    let response, err = pool.Do(context.Background(), "GET", target, http.NewHeader(), empty[:])
    if err != nil { done.send(0); return }
    done.send(response.StatusCode)
}
func download(pool http.Pool, target string, sink mut io.Writer) error {
    let empty = Array<byte>{}
    var stream = pool.Open(context.Background(), "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig())?
    io.Copy(sink, stream)?
    return stream.Close()
}
func main() {
    var config = http.DefaultPoolConfig()
    config.MaxConnectionsPerHost = 2
    config.IdleTimeout = 30 * time.Second
    let client, err = http.NewClient(http.DefaultClientConfig())
    if err != nil { return }
    let pool, poolErr = http.NewPool(client, config)
    if poolErr != nil { return }
    let done = channel<int>(2)
    go worker(pool, "http://localhost/a", done)
    go worker(pool, "http://localhost/b", done)
    let stats = pool.Stats()
    println(stats.Active + stats.Idle + stats.Waiting)
    pool.CloseIdle()
    pool.Close()
}
"#,
    )]);
}

#[test]
fn pooled_http_clients_reject_private_state_moved_clients_and_wrong_types() {
    let open = "let client = http.NewClient(http.DefaultClientConfig())?; let pool = http.NewPool(client, http.DefaultPoolConfig())?";
    for (body, expected) in [
        (format!("{open}; let _ = pool.state"), "not exported"),
        (format!("{open}; println(pool.live)"), "not exported"),
        (format!("{open}; let _, _ = client.Get(context.Background(), \"http://a.test/\")"), "moved"),
        (
            "let client = http.NewClient(http.DefaultClientConfig())?; let _, _ = http.NewPool(client, http.DefaultStreamConfig())".to_owned(),
            "mismatched types",
        ),
        (
            format!("{open}; let _, _ = pool.Open(context.Background(), \"GET\", \"http://a.test/\", http.NewHeader(), Array<byte>{{}}[:], http.DefaultPoolConfig())"),
            "mismatched types",
        ),
        (format!("{open}; let _ = pool.Stats().Leased"), "Leased"),
    ] {
        let imports = ["context", "net/http"]
            .into_iter()
            .filter(|path| body.contains(&format!("{}.", path.rsplit('/').next().unwrap())))
            .map(|path| format!("import \"std/{path}\"\n"))
            .collect::<String>();
        let source = format!(
            "package main\n{imports}func run() error {{ {body}; return nil }}\nfunc main() {{ let _ = run() }}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn http_redirects_check_as_opt_in_calls() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/context"
import "std/io"
import "std/net/http"
func fetch(client http.Client, target string, sink mut io.Writer) (string, error) {
    var redirects = http.DefaultRedirectConfig()
    redirects.MaxRedirects = 5
    redirects.AllowHTTPSDowngrade = false
    let empty = Array<byte>{}
    let response, final = client.Follow(context.Background(), "GET", target, http.NewHeader(), empty[:], redirects)?
    println(response.StatusCode)
    var stream, streamed = client.OpenFollow(context.Background(), "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig(), redirects)?
    io.Copy(sink, stream)?
    stream.Close()?
    return final + streamed, nil
}
func pooled(pool http.Pool, target string) error {
    let empty = Array<byte>{}
    let _, _ = pool.Follow(context.Background(), "GET", target, http.NewHeader(), empty[:], http.DefaultRedirectConfig())?
    var stream, _ = pool.OpenFollow(context.Background(), "GET", target, http.NewHeader(), empty[:], http.DefaultStreamConfig(), http.DefaultRedirectConfig())?
    return stream.Close()
}
func main() {}
"#,
    )]);
}

#[test]
fn http_redirects_reject_wrong_results_and_types() {
    let open =
        "let client = http.NewClient(http.DefaultClientConfig())?; let empty = Array<byte>{}";
    for (body, expected) in [
        (
            format!(
                "{open}; let _, final, _ = client.Follow(context.Background(), \"GET\", \"http://a.test/\", http.NewHeader(), empty[:], http.DefaultRedirectConfig()); let _ = final - 1"
            ),
            "mismatched types",
        ),
        (
            format!(
                "{open}; let _, _, _ = client.Follow(context.Background(), \"GET\", \"http://a.test/\", http.NewHeader(), empty[:], http.DefaultStreamConfig())"
            ),
            "mismatched types",
        ),
        (
            format!(
                "{open}; let _, _, _ = client.OpenFollow(context.Background(), \"GET\", \"http://a.test/\", http.NewHeader(), empty[:], http.DefaultRedirectConfig(), http.DefaultStreamConfig())"
            ),
            "mismatched types",
        ),
        (
            "let _ = http.DefaultRedirectConfig().Hops".to_owned(),
            "Hops",
        ),
    ] {
        let imports = ["context", "net/http"]
            .into_iter()
            .filter(|path| body.contains(&format!("{}.", path.rsplit('/').next().unwrap())))
            .map(|path| format!("import \"std/{path}\"\n"))
            .collect::<String>();
        let source = format!(
            "package main\n{imports}func run() error {{ {body}; return nil }}\nfunc main() {{ let _ = run() }}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn http_retries_check_as_opt_in_calls() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/context"
import "std/net/http"
import "std/time"
func fetch(client http.Client, pool http.Pool, target string) (int, error) {
    var config = http.DefaultRetryConfig()
    config.MaxAttempts = 4
    config.Backoff = 50 * time.Millisecond
    config.MaxBackoff = time.Second
    config.Timeout = 10 * time.Second
    config.RetryUnsafe = false
    let empty = Array<byte>{}
    let response = client.DoRetry(context.Background(), "GET", target, http.NewHeader(), empty[:], config)?
    let pooled = pool.DoRetry(context.Background(), "PUT", target, http.NewHeader(), empty[:], http.DefaultRetryConfig())?
    return response.StatusCode + pooled.StatusCode, nil
}
func main() {}
"#,
    )]);
}

#[test]
fn http_retries_reject_wrong_configuration_types() {
    let open =
        "let client = http.NewClient(http.DefaultClientConfig())?; let empty = Array<byte>{}";
    for (body, expected) in [
        (
            format!(
                "{open}; let _, _ = client.DoRetry(context.Background(), \"GET\", \"http://a.test/\", http.NewHeader(), empty[:], http.DefaultRedirectConfig())"
            ),
            "mismatched types",
        ),
        (
            "var config = http.DefaultRetryConfig(); config.Backoff = \"slow\"".to_owned(),
            "mismatched types",
        ),
        (
            "let _ = http.DefaultRetryConfig().Jitter".to_owned(),
            "Jitter",
        ),
    ] {
        let imports = ["context", "net/http"]
            .into_iter()
            .filter(|path| body.contains(&format!("{}.", path.rsplit('/').next().unwrap())))
            .map(|path| format!("import \"std/{path}\"\n"))
            .collect::<String>();
        let source = format!(
            "package main\n{imports}func run() error {{ {body}; return nil }}\nfunc main() {{ let _ = run() }}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn http_decoding_checks_as_opt_in_calls() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/context"
import "std/net/http"
func fetch(client http.Client, pool http.Pool, target string) (int, error) {
    var config = http.DefaultDecodeConfig()
    config.MaxBytes = 1 << 20
    config.MaxRatio = 50
    let empty = Array<byte>{}
    let response = client.DoDecoded(context.Background(), "GET", target, http.NewHeader(), empty[:], config)?
    let pooled = pool.DoDecoded(context.Background(), "GET", target, http.NewHeader(), empty[:], http.DefaultDecodeConfig())?
    return response.Body.len() + pooled.Body.len(), nil
}
func main() {}
"#,
    )]);
}

#[test]
fn http_decoding_rejects_wrong_configuration_types() {
    let open =
        "let client = http.NewClient(http.DefaultClientConfig())?; let empty = Array<byte>{}";
    for (body, expected) in [
        (
            format!(
                "{open}; let _, _ = client.DoDecoded(context.Background(), \"GET\", \"http://a.test/\", http.NewHeader(), empty[:], http.DefaultRetryConfig())"
            ),
            "mismatched types",
        ),
        (
            "var config = http.DefaultDecodeConfig(); config.MaxRatio = \"high\"".to_owned(),
            "mismatched types",
        ),
        (
            "let _ = http.DefaultDecodeConfig().Encodings".to_owned(),
            "Encodings",
        ),
    ] {
        let imports = ["context", "net/http"]
            .into_iter()
            .filter(|path| body.contains(&format!("{}.", path.rsplit('/').next().unwrap())))
            .map(|path| format!("import \"std/{path}\"\n"))
            .collect::<String>();
        let source = format!(
            "package main\n{imports}func run() error {{ {body}; return nil }}\nfunc main() {{ let _ = run() }}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn http_proxies_check_as_explicit_clients() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/context"
import "std/net"
import "std/net/http"
import "std/net/url"
type Tunnel struct {}
func (t Tunnel) Tunnel(ctx context.Context, conn own net.Conn, target url.AbsoluteURL) (http.ClientConn, error) {
    return conn, nil
}
func fetch(target string) (int, error) {
    let proxy = http.ProxyConfig{URL: "http://proxy.test:3128/", Authorization: "Basic eDp5"}
    let plain = http.NewProxyClient(http.DefaultClientConfig(), proxy)?
    let tunneled = http.NewProxyClientWithTunnel(http.DefaultClientConfig(), proxy, Tunnel{})?
    let empty = Array<byte>{}
    let response = plain.Do(context.Background(), "GET", target, http.NewHeader(), empty[:])?
    let secure = tunneled.Do(context.Background(), "GET", target, http.NewHeader(), empty[:])?
    return response.StatusCode + secure.StatusCode, nil
}
func main() {}
"#,
    )]);
}

#[test]
fn http_proxies_reject_wrong_tunnels_and_types() {
    for (body, expected) in [
        (
            "let _, _ = http.NewProxyClient(http.DefaultClientConfig(), \"http://proxy.test/\")".to_owned(),
            "mismatched types",
        ),
        (
            "let _, _ = http.NewProxyClientWithTunnel(http.DefaultClientConfig(), http.ProxyConfig{URL: \"\", Authorization: \"\"}, 5)".to_owned(),
            "expected `http.ClientTunnel`",
        ),
        ("let _ = http.ProxyConfig{URL: \"\", Authorization: \"\"}.Password".to_owned(), "Password"),
    ] {
        let source = format!(
            "package main\nimport \"std/net/http\"\nfunc run() error {{ {body}; return nil }}\nfunc main() {{ let _ = run() }}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}
