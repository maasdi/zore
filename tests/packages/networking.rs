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
