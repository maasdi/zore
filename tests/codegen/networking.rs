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
