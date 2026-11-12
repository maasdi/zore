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
