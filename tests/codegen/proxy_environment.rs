use super::*;
use std::time::Duration;

#[test]
fn ip_prefixes_mask_canonical_addresses_and_preserve_families() {
    let output = run(r#"package main
import "std/net"
import "std/errors"
func check(ok bool) { if !ok { panic("prefix check") } }
func run() error {
    let inputs = Array<string>{"192.0.2.255/24", "255.255.255.255/0", "192.0.2.129/25", "192.0.2.1/32", "2001:DB8:abcd:ffff::1/33", "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/0", "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/128", "::ffff:192.0.2.129/120", "::ffff:192.0.2.129/97", "::1/127"}
    let expected = Array<string>{"192.0.2.0/24", "0.0.0.0/0", "192.0.2.128/25", "192.0.2.1/32", "2001:db8:8000::/33", "::/0", "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/128", "::ffff:192.0.2.0/120", "::ffff:128.0.0.0/97", "::/127"}
    for i, text in inputs {
        let prefix = net.ParseIPPrefix(text)?
        check(prefix.IsValid() && prefix.String() == expected[i])
        check(prefix.Contains(prefix.Addr()))
        let again = net.ParseIPPrefix(prefix.String())?
        check(again.Bits() == prefix.Bits() && again.Addr().Equal(prefix.Addr()))
    }
    let v4 = net.ParseIPPrefix("192.0.2.129/25")?
    check(v4.Contains(net.ParseIP("192.0.2.255")?) && !v4.Contains(net.ParseIP("192.0.2.127")?))
    check(!v4.Contains(net.ParseIP("::ffff:192.0.2.129")?))
    let paddedZero = net.ParseIPPrefix("::1/0000")?
    check(paddedZero.String() == "::/0")
    let v6 = net.ParseIPPrefix("::/0")?
    check(v6.Contains(net.ParseIP("::ffff:192.0.2.129")?) && !v6.Contains(net.ParseIP("192.0.2.129")?))
    for text in Array<string>{"", "192.0.2.1", "192.0.2.1/", "192.0.2.1/-1", "192.0.2.1/+1", "192.0.2.1/33", "::1/129", "::1/1/2", "::1/ 1", "[::1]/128", "fe80::1%eth0/64", "127.1/8", "localhost/8", "::1/1x"} {
        let prefix, err = net.ParseIPPrefix(text)
        check(err != nil && !prefix.IsValid() && !prefix.Addr().IsValid() && prefix.Bits() == -1 && prefix.String() == "" && !prefix.Contains(v4.Addr()))
    }
    println("ok")
    return nil
}
func main() { let err = run(); if err != nil { panic(errors.Message(err)) } }
"#);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[test]
fn proxy_environment_selection_validates_precedence_credentials_and_bypasses() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("environment");
    build_file(r#"package main
import "std/net/http"
import "std/os"
import "std/errors"
func main() {
    let args = os.Args()
    for var i = 1; i < args.len(); i += 1 {
        let proxy, enabled, err = http.ProxyFromEnvironment(args[i])
        if err != nil { println(errors.Message(err)) } else if !enabled { println("direct") } else { println(proxy.URL + "|" + proxy.Authorization) }
    }
}
"#, &executable).unwrap();
    let invoke = |env: &[(&str, &str)], targets: &[&str]| {
        let mut command = Command::new(&executable);
        command.env_clear();
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        command.args(targets).env("ZORE_CHECK_LEAKS", "1");
        command.envs(env.iter().copied());
        let output = bounded_process::run(&mut command, Duration::from_secs(10)).unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(output.stderr.is_empty(), "{}", stderr(&output));
        stdout(&output)
    };
    assert_eq!(
        invoke(&[], &["http://example.test/", "https://example.test/"]),
        "direct\ndirect\n"
    );
    if !cfg!(windows) {
        assert_eq!(
            invoke(
                &[
                    ("HTTP_PROXY", "https://upper.test:443/"),
                    ("http_proxy", "invalid"),
                    ("HTTPS_PROXY", "http://secure.test/")
                ],
                &["http://a/", "https://b/", "http://c/"]
            ),
            "https://upper.test:443|\nhttp://secure.test|\nhttps://upper.test:443|\n"
        );
        assert_eq!(
            invoke(
                &[
                    ("HTTP_PROXY", ""),
                    ("http_proxy", "invalid"),
                    ("NO_PROXY", ""),
                    ("no_proxy", "invalid/path")
                ],
                &["http://a/"]
            ),
            "direct\n"
        );
    }
    assert_eq!(
        invoke(
            &[(
                "http_proxy",
                "http://u%2Bser:p%3A%40%2B%C3%A9@proxy.test:80/#ignored"
            )],
            &["http://a/"]
        ),
        "http://proxy.test:80|Basic dStzZXI6cDpAK8Op\n"
    );
    assert_eq!(
        invoke(&[("HTTP_PROXY", "http://name@proxy.test/")], &["http://a/"]),
        "http://proxy.test|Basic bmFtZTo=\n"
    );
    for value in [
        "http://u%3Aname:secret@proxy.test/",
        "http://u:%0Dsecret@proxy.test/",
        "http://u:%FF@proxy.test/",
        "http://u:%ZZ@proxy.test/",
        "http://u@evil@proxy.test/",
        "http://proxy.test/path",
        "http://proxy.test/?q",
        "ftp://proxy.test/",
        "proxy.test:80",
    ] {
        assert_eq!(
            invoke(&[("HTTPS_PROXY", value)], &["http://a/"]),
            "http.ProxyFromEnvironment: invalid HTTPS_PROXY\n",
            "{value}"
        );
    }
    let rules = [
        ("example.test", "http://example.test/", true),
        ("EXAMPLE.test.", "http://sub.example.test./", true),
        ("example.test", "http://badexample.test/", false),
        (".example.test", "http://example.test/", false),
        ("*.example.test", "http://a.example.test/", true),
        ("example.test:80", "http://example.test/", true),
        ("example.test:443", "http://example.test/", false),
        ("192.0.2.1", "http://192.0.2.1/", true),
        ("192.0.2.129/25", "http://192.0.2.255/", true),
        ("192.0.2.129/25", "http://192.0.2.127/", false),
        ("192.0.2.0/24:8080", "http://192.0.2.1:8080/", true),
        ("192.0.2.0/24", "http://example.test/", false),
        ("2001:0DB8::1", "http://[2001:db8::1]/", true),
        ("[2001:db8::1]:80", "http://[2001:db8::1]/", true),
        ("[2001:db8::1]:443", "http://[2001:db8::1]/", false),
        ("[2001:db8::/32]:80", "http://[2001:db8::ffff]/", true),
        ("2001:db8::/32:80", "http://[2001:db8::ffff]/", true),
        ("::ffff:192.0.2.0/120", "http://[::ffff:192.0.2.1]/", true),
        ("::ffff:192.0.2.0/120", "http://192.0.2.1/", false),
        ("*", "http://any.test/", true),
        ("*:443", "https://any.test/", true),
        ("*:443", "http://any.test/", false),
        (
            " ,\t,.other.test, example.test\t,",
            "http://example.test/",
            true,
        ),
    ];
    for (rule, target, bypass) in rules {
        let expected = if bypass {
            "direct\n"
        } else {
            "http://proxy.test|\n"
        };
        assert_eq!(
            invoke(
                &[
                    ("HTTP_PROXY", "http://proxy.test/"),
                    ("HTTPS_PROXY", "http://proxy.test/"),
                    ("NO_PROXY", rule)
                ],
                &[target]
            ),
            expected,
            "{rule} {target}"
        );
    }
    for rule in [
        "example.*",
        "*example.test",
        "example.test/path",
        "example.test:0",
        "example.test:65536",
        "example.test:",
        "example.test:+80",
        "192.0.2.1/33",
        "[::1",
        "[127.0.0.1]:80",
        "::1/129",
        "*,invalid/path",
    ] {
        assert_eq!(
            invoke(&[("NO_PROXY", rule)], &["http://a/"]),
            "http.ProxyFromEnvironment: invalid NO_PROXY\n",
            "{rule}"
        );
    }
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"] {
        let large = "x".repeat(4097);
        assert_eq!(
            invoke(&[(name, &large)], &["http://a/"]),
            format!("http.ProxyFromEnvironment: invalid {name}\n")
        );
    }
    assert_eq!(
        invoke(&[("NO_PROXY", &vec!["a"; 256].join(","))], &["http://a/"]),
        "direct\n"
    );
    assert_eq!(
        invoke(&[("NO_PROXY", &vec!["a"; 257].join(","))], &["http://a/"]),
        "http.ProxyFromEnvironment: invalid NO_PROXY\n"
    );
    assert_eq!(
        invoke(
            &[
                ("HTTP_PROXY", "http://proxy.test/"),
                ("REQUEST_METHOD", "GET")
            ],
            &["http://a/"]
        ),
        "http.ProxyFromEnvironment: HTTP_PROXY refused in CGI\n"
    );
    assert_eq!(
        invoke(
            &[
                ("HTTP_PROXY", "http://proxy.test/"),
                ("REQUEST_METHOD", "GET"),
                ("NO_PROXY", "a")
            ],
            &["http://a/"]
        ),
        "direct\n"
    );
    assert_eq!(
        invoke(
            &[
                ("http_proxy", "http://proxy.test/"),
                ("HTTPS_PROXY", "http://proxy.test/"),
                ("REQUEST_METHOD", "GET")
            ],
            &["http://a/", "https://a/"]
        ),
        "http://proxy.test|\nhttp://proxy.test|\n"
    );
}
