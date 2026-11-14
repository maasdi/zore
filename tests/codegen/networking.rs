use super::*;
use std::time::Duration;

fn execute(source: &str, controlled: bool) -> Output {
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("networking");
    if controlled {
        controlled_runtime::build_with_workers(source, &executable, 1);
    } else {
        build_file(source, &executable).unwrap_or_else(|error| panic!("{error:?}"));
    }
    bounded_process::run(
        Command::new(executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap()
}

fn succeeds(source: &str, controlled: bool) {
    let output = execute(source, controlled);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn ip_addresses_and_literal_endpoints_are_canonical_independent_values() {
    succeeds(
        r##"package main
import "std/net"
import "std/errors"
func check(ok bool) { if !ok { panic("address check") } }
func local() (net.IP, error) { var text = "2001:0DB8:0:0:0:0:0:1"; let ip = net.ParseIP(text)?; text = "changed"; return ip, nil }
func run() error {
    let invalid, invalidErr = net.ParseIP("")
    check(invalidErr != nil && !invalid.IsValid() && !invalid.Is4() && !invalid.Is6() && invalid.String() == "")
    let ip = local()?
    check(ip.String() == "2001:db8::1" && ip.Is6() && !ip.Is4())
    let same = net.ParseIP("2001:db8::1")?
    check(ip.Equal(same) && ip.Compare(same) == 0)
    let v4 = net.ParseIP("192.0.2.1")?
    let mapped = net.ParseIP("::ffff:192.0.2.1")?
    check(mapped.Is6() && !mapped.Equal(v4) && v4.Compare(mapped) == -1 && mapped.Compare(v4) == 1)
    check(invalid.Compare(v4) == -1 && v4.Compare(invalid) == 1)
    let lower = net.ParseIP("2.0.0.0")?
    let higher = net.ParseIP("10.0.0.0")?
    check(lower.Compare(higher) == -1)
    let endpoint = net.ParseAddrPort("[2001:0DB8::1]:00080")?
    check(endpoint.Addr().Equal(ip) && endpoint.Port() == 80 && endpoint.String() == "[2001:db8::1]:80")
    let zero = net.NewAddrPort(v4, 0)?
    check(zero.IsValid() && zero.String() == "192.0.2.1:0")
    let one = net.NewAddrPort(v4, 1)?
    check(zero.Compare(one) == -1 && one.Compare(zero) == 1)
    for text in Array<string>{"127.1", "0177.0.0.1", "127.0.0.01", "0x7f000001", "localhost", "1.2.3.256", "1.2.3.-1", "1.2.3.4 ", "[::1]", "fe80::1%3", "::/0", "1::2::3", ":::1", "1:2:3:4:5:6:7:8:9", "::ffff:01.2.3.4"} {
        let value, err = net.ParseIP(text)
        check(err != nil && !value.IsValid())
    }
    for text in Array<string>{"localhost:80", "::1:80", "[127.0.0.1]:80", "1.2.3.4:+1", "1.2.3.4:-1", "1.2.3.4:65536", "1.2.3.4:000080", "1.2.3.4:", "[::1]80"} {
        let value, err = net.ParseAddrPort(text)
        check(err != nil && !value.IsValid() && value.Port() == 0 && value.String() == "")
    }
    let _, portErr = net.NewAddrPort(v4, 65536)
    check(portErr != nil)
    return nil
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"##,
        false,
    );
}

#[test]
fn uri_references_follow_rfc_3986_and_keep_http_admission_separate() {
    succeeds(
        r##"package main
import "std/net/url"
import "std/errors"
func check(ok bool) { if !ok { panic("URI check") } }
func run() error {
    let base = "http://a/b/c/d;p?q"
    let references = Array<string>{"g:h", "g", "./g", "g/", "/g", "//g", "?y", "g?y", "#s", "g#s", "g?y#s", ";x", "g;x", "g;x?y#s", "", ".", "./", "..", "../", "../g", "../..", "../../", "../../g", "../../../g", "../../../../g", "/./g", "/../g", "g.", ".g", "g..", "..g", "./../g", "./g/.", "g/./h", "g/../h", "g;x=1/./y", "g;x=1/../y", "g?y/./x", "g?y/../x", "g#s/./x", "g#s/../x", "http:g"}
    let expected = Array<string>{"g:h", "http://a/b/c/g", "http://a/b/c/g", "http://a/b/c/g/", "http://a/g", "http://g", "http://a/b/c/d;p?y", "http://a/b/c/g?y", "http://a/b/c/d;p?q#s", "http://a/b/c/g#s", "http://a/b/c/g?y#s", "http://a/b/c/;x", "http://a/b/c/g;x", "http://a/b/c/g;x?y#s", "http://a/b/c/d;p?q", "http://a/b/c/", "http://a/b/c/", "http://a/b/", "http://a/b/", "http://a/b/g", "http://a/", "http://a/", "http://a/g", "http://a/g", "http://a/g", "http://a/g", "http://a/g", "http://a/b/c/g.", "http://a/b/c/.g", "http://a/b/c/g..", "http://a/b/c/..g", "http://a/b/g", "http://a/b/c/g/", "http://a/b/c/g/h", "http://a/b/c/h", "http://a/b/c/g;x=1/y", "http://a/b/c/y", "http://a/b/c/g?y/./x", "http://a/b/c/g?y/../x", "http://a/b/c/g#s/./x", "http://a/b/c/g#s/../x", "http:g"}
    for var i = 0; i < references.len(); i += 1 {
        let resolved = url.ResolveReference(base, references[i])?
        check(resolved == expected[i])
    }
    check(url.ResolveReference("http://a/x?old#base", "?")? == "http://a/x?")
    check(url.ResolveReference("http://a/x?old#base", "#")? == "http://a/x?old#")
    check(url.ResolveReference("http://a/x", "%2e%2E/%2F//b")? == "http://a/%2e%2E/%2F//b")
    check(url.ResolveReference("file:///a/b", "../c")? == "file:///c")
    check(url.ResolveReference(base, "//[vF.a:b]:/c")? == "http://[vF.a:b]:/c")
    check(url.ResolveReference("HTTP://A/X", "#f")? == "HTTP://A/X#f")
    check(url.ResolveReference("urn:a/b", "../c")? == "urn:/c")
    check(url.ResolveReference("http://a/x", "//user:pass@b:999999/a")? == "http://user:pass@b:999999/a")
    for ref in Array<string>{"http://a:bad/", "http://[1:2]/", "http://u@v@host/", "%", "%GG", "a b", "é", "a\u0000b", "a\\b", "x##y", "1:bad"} {
        let output, err = url.ResolveReference(base, ref)
        check(err != nil && output == "")
    }
    for ref in Array<string>{"//u@b/a", "ftp://b/a", "//b:999999/a", "http:g"} {
        let output = url.ResolveReference(base, ref)?
        let _, err = url.ParseAbsolute(output)
        check(err != nil)
    }
    let http = url.ParseAbsolute(url.ResolveReference("https://example.test/a/b", "../c?x#f")?)?
    check(http.RequestTarget == "/c?x" && http.Scheme == "https")
    return nil
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"##,
        false,
    );
}

#[test]
fn udp_preserves_datagrams_peers_counts_and_deadlines() {
    succeeds(
        r##"package main
import "std/net"
import "std/time"
import "std/errors"
func check(ok bool) { if !ok { panic("UDP check") } }
func run() error {
    let local = net.ParseAddrPort("127.0.0.1:0")?
    let server = net.ListenUDP("udp4", local)?
    let client = net.ListenUDP("udp", local)?
    check(server.LocalAddr().IsValid() && server.LocalAddr().Port() > 0)
    server.SetDeadline(time.Now().Add(5 * time.Second))?
    client.SetDeadline(time.Now().Add(5 * time.Second))?
    let payload = Array<byte>{0, 255, 60, 128, 42}
    check(client.WriteTo(payload[:], server.LocalAddr())? == 5)
    var buffer = Array<byte>{99, 99, 99}
    let count, peer, truncated, err = server.ReadFrom(buffer[:])
    if err != nil { return err }
    check(count == 3 && truncated && peer.Equal(client.LocalAddr()) && buffer[0] == 0 && buffer[1] == 255 && buffer[2] == 60)
    check(client.WriteTo(Array<byte>{}[:], server.LocalAddr())? == 0)
    let emptyCount, emptyPeer, emptyTruncated, emptyErr = server.ReadFrom(buffer[:])
    if emptyErr != nil { return emptyErr }
    check(emptyCount == 0 && !emptyTruncated && emptyPeer.Equal(peer) && buffer[0] == 0)
    client.WriteTo(payload[:], server.LocalAddr())?
    var empty = Array<byte>{}
    let discarded, _, lost, discardErr = server.ReadFrom(empty[:])
    if discardErr != nil { return discardErr }
    check(discarded == 0 && lost)
    client.WriteTo(Array<byte>{7}[:], server.LocalAddr())?
    server.SetReadDeadline(time.Now().Add(-time.Second))?
    let failed, missing, _, expired = server.ReadFrom(buffer[:])
    check(expired != nil); check(failed == 0 && !missing.IsValid())
    server.ClearDeadline()?
    server.SetReadDeadline(time.Now().Add(time.Second))?
    let recovered, _, more, retryErr = server.ReadFrom(buffer[:])
    if retryErr != nil { return retryErr }
    check(recovered == 1 && !more && buffer[0] == 7)
    client.SetWriteDeadline(time.Now().Add(-time.Second))?
    let sent, sendErr = client.WriteTo(payload[:], server.LocalAddr())
    check(sendErr != nil); check(sent == 0)
    let invalid, invalidErr = net.ParseAddrPort("bad")
    check(invalidErr != nil)
    let _, peerErr = client.WriteTo(payload[:], invalid)
    check(peerErr != nil)
    var oversized = Array<byte>{}
    for var i = 0; i < 65508; i += 1 { oversized.push(0) }
    let _, sizeErr = client.WriteTo(oversized[:], server.LocalAddr())
    check(sizeErr != nil); check(errors.Message(sizeErr) == "net.UDP.WriteTo: datagram exceeds 65507 bytes")
    server.Close()?
    return client.Close()
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"##,
        false,
    );
}

#[test]
fn udp_plain_and_async_waits_allow_one_worker_progress() {
    succeeds(
        r##"package main
import "std/net"
import "std/time"
import "std/errors"
func check(ok bool) { if !ok { panic("UDP progress") } }
async func receiver(conn own net.UDPConn) error {
    var data = Array<byte>{0, 0}
    let count, peer, truncated, err = conn.ReadFrom(data[:])
    if err != nil { return err }
    check(count == 2 && !truncated && data[0] == 4 && data[1] == 5)
    conn.WriteTo(data[:], peer)?
    return nil
}
func plainReceiver(conn own net.UDPConn) error {
    var data = Array<byte>{0}
    let count, _, truncated, err = conn.ReadFrom(data[:])
    if err != nil { return err }
    check(count == 1 && !truncated && data[0] == 9)
    return nil
}
async func sender(conn own net.UDPConn, peer net.AddrPort) error {
    conn.WriteTo(Array<byte>{9}[:], peer)?
    return nil
}
func run() error {
    waitIdle(0)
    let local = net.ParseAddrPort("127.0.0.1:0")?
    let server = net.ListenUDP("udp4", local)?
    let client = net.ListenUDP("udp4", local)?
    server.SetDeadline(time.Now().Add(5 * time.Second))?
    client.SetDeadline(time.Now().Add(5 * time.Second))?
    let endpoint = server.LocalAddr()
    let pending = go receiver(server)
    waitPending(1)
    client.WriteTo(Array<byte>{4, 5}[:], endpoint)?
    var response = Array<byte>{0, 0}
    let count, peer, truncated, err = client.ReadFrom(response[:])
    if err != nil { return err }
    check(count == 2 && !truncated && peer.Equal(endpoint))
    pending.wait()?
    waitIdle(0)
    let blockedServer = net.ListenUDP("udp4", local)?
    let blockedPeer = blockedServer.LocalAddr()
    blockedServer.SetReadDeadline(time.Now().Add(5 * time.Second))?
    let blocked = go plainReceiver(blockedServer)
    waitBlocked(1)
    let writer = go sender(client, blockedPeer)
    writer.wait()?
    blocked.wait()?
    waitIdle(0)
    return nil
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"##,
        true,
    );
}

#[test]
fn dns_lookup_owns_results_and_cancels_before_late_resolver_cleanup() {
    succeeds(
        r##"package main
import "std/net"
import "std/context"
import "std/time"
import "std/errors"
func check(ok bool) { if !ok { panic("DNS check") } }
async func asyncLookup(ctx context.Context) (Array<net.IP>, error) {
    return net.LookupIP(ctx, "ip", "localhost")
}
func run() error {
    waitIdle(0)
    let background = context.Background()
    let literal = net.LookupIP(background, "ip4", "127.0.0.1")?
    check(literal.len() == 1 && literal[0].Is4())
    let _, mismatch = net.LookupIP(background, "ip6", "127.0.0.1")
    check(mismatch != nil)
    for host in Array<string>{"127.1", "0177.0.0.1", "0x7f000001", "0X7F000001", "0x7f.0.0.1", "bad host", "a..b", "a\u0000b", "é.test", "[::1]", "host:80", "-bad.test", "bad-.test"} {
        let value, err = net.LookupIP(background, "ip", host)
        check(err != nil); check(value.len() == 0)
    }
    lookupGate(0)
    let ctx, cancel = context.WithCancel(background)
    let task = go asyncLookup(ctx)
    lookupGate(1)
    waitPending(2)
    let immediate = net.LookupIP(background, "ip6", "::1")?
    check(immediate.len() == 1 && immediate[0].Is6())
    cancel()
    let cancelled, cancellation = task.wait()
    check(cancellation != nil); check(cancelled.len() == 0 && errors.Message(cancellation) == "context canceled")
    lookupGate(2)
    waitIdle(0)
    lookupGate(0)
    let plainContext, cancelPlain = context.WithCancel(background)
    let plainTask = go net.LookupIP(plainContext, "ip", "localhost")
    lookupGate(1)
    waitBlocked(1)
    cancelPlain()
    let plainResult, plainError = plainTask.wait()
    check(plainError != nil)
    check(plainResult.len() == 0)
    lookupGate(2)
    waitIdle(0)
    var host = "localhost"
    let resolved = net.LookupIP(background, "ip", host)?
    host = "changed"
    check(resolved.len() > 0 && resolved[0].IsValid() && host == "changed")
    for var i = 0; i < resolved.len(); i += 1 {
        for var j = i + 1; j < resolved.len(); j += 1 { check(!resolved[i].Equal(resolved[j])) }
    }
    waitIdle(0)
    let expired, stop = context.WithTimeout(background, -time.Second)
    let missing, timeout = net.LookupIP(expired, "ip", "localhost")
    stop()
    check(timeout != nil); check(missing.len() == 0 && errors.Message(timeout) == "context deadline exceeded")
    waitIdle(0)
    return nil
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) }; println("ok") }
"##,
        true,
    );
}

#[test]
fn udp_handles_are_cleaned_on_normal_error_and_panic_exits() {
    for (exit, success) in [
        ("return nil", true),
        ("return error(\"stop\")", true),
        ("panic(\"stop\")", false),
    ] {
        let source = format!(
            "package main\nimport \"std/net\"\nfunc inner() error {{ let endpoint = net.ParseAddrPort(\"127.0.0.1:0\")?; let conn = net.ListenUDP(\"udp4\", endpoint)?; println(conn.LocalAddr().IsValid()); {exit} }}\nfunc main() {{ waitIdle(0); let _ = inner(); checkSocketCleanup() }}\n"
        );
        let output = execute(&source, true);
        assert_eq!(output.status.success(), success, "{}", stderr(&output));
        assert!(!stderr(&output).contains("leak:"), "{}", stderr(&output));
        if !success {
            assert!(
                stderr(&output).contains("panic in the main task: stop"),
                "{}",
                stderr(&output)
            );
        }
    }
}

#[test]
fn networking_guide_runs_with_leak_checked_datagram_cleanup() {
    let source = include_str!("../../docs/networking.md")
        .split("```ore\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let output = execute(source, false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "hello\nhttps://example.test/c?x#f\n");
}
