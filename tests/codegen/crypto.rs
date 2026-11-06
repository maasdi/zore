use super::*;

const CHECK: &str = r#"
func check(ok bool, label string) {
    if !ok { panic(label) }
}
"#;

#[test]
fn random_bytes_come_from_the_system_within_bounds() {
    let source = format!(
        r#"package main
import "std/crypto/rand"
import "std/crypto/subtle"
import "std/errors"
{CHECK}
func main() {{
    for size in [int; 5]{{1, 7, 255, 256, 4097}} {{
        let data, err = rand.Bytes(size)
        check(err == nil && data.len() == size, "size")
    }}
    let empty, emptyErr = rand.Bytes(0)
    check(emptyErr == nil && empty.len() == 0, "empty")
    let largest, largestErr = rand.Bytes(rand.MaxBytes)
    check(largestErr == nil && largest.len() == rand.MaxBytes, "largest")
    for size in [int; 3]{{-1, rand.MaxBytes + 1, -9223372036854775807 - 1}} {{
        let data, err = rand.Bytes(size)
        check(data.len() == 0, "no output")
        println(errors.Message(err))
    }}
    var seen = Array<int>{{}}
    for seen.len() < 256 {{ seen.push(0) }}
    let sample, _ = rand.Bytes(65536)
    for b in sample {{ seen[int(b)] += 1 }}
    var missing = 0
    var most = 0
    for count in seen {{
        if count == 0 {{ missing += 1 }}
        if count > most {{ most = count }}
    }}
    println(missing == 0 && most < 512)
    let first, _ = rand.Bytes(32)
    let second, _ = rand.Bytes(32)
    println(subtle.ConstantTimeEqual(first[:], second[:]))
}}
"#
    );
    prints(
        &source,
        "crypto/rand.Bytes: size outside 0..1048576\ncrypto/rand.Bytes: size outside 0..1048576\ncrypto/rand.Bytes: size outside 0..1048576\ntrue\nfalse\n",
    );
}

#[test]
fn random_text_is_a_url_safe_unique_token() {
    let source = format!(
        r#"package main
import "std/crypto/rand"
import "std/strings"
{CHECK}
func main() {{
    var tokens = map[string]bool{{}}
    for var i = 0; i < 2000; i += 1 {{
        let token, err = rand.Text()
        check(err == nil && token.len() == 43, "length")
        for var j = 0; j < token.len(); j += 1 {{
            check(strings.IndexByte("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_", token[j]) >= 0, "alphabet")
        }}
        tokens[token] = true
    }}
    println(tokens.len())
}}
"#
    );
    prints(&source, "2000\n");
}

#[test]
fn constant_time_equal_reads_whole_inputs() {
    let source = format!(
        r#"package main
import "std/crypto/subtle"
import "std/strings"
{CHECK}
func main() {{
    let empty = Array<byte>{{}}
    check(subtle.ConstantTimeEqual(empty[:], empty[:]), "empty")
    let base = strings.Bytes("session-token-0123456789")
    check(subtle.ConstantTimeEqual(base[:], base[:]), "same slice")
    var copy = clone(base)
    check(subtle.ConstantTimeEqual(base[:], copy[:]), "same bytes")
    for var i = 0; i < base.len(); i += 1 {{
        var changed = clone(base)
        changed[i] = changed[i] ^ 1
        check(!subtle.ConstantTimeEqual(base[:], changed[:]), "difference")
    }}
    check(!subtle.ConstantTimeEqual(base[:], base[:base.len() - 1]), "shorter")
    check(!subtle.ConstantTimeEqual(empty[:], base[:]), "empty and full")
    let high = Array<byte>{{0, 255, 128}}
    let low = Array<byte>{{0, 255, 127}}
    check(!subtle.ConstantTimeEqual(high[:], low[:]), "binary")
    println("ok")
}}
"#
    );
    prints(&source, "ok\n");
}

#[test]
fn random_bytes_work_from_many_async_tasks() {
    let source = format!(
        r#"package main
import "std/crypto/rand"
{CHECK}
async func token() (string, error) {{
    let first = rand.Text()?
    let data = rand.Bytes(64)?
    check(data.len() == 64, "bytes")
    return first, nil
}}
func main() {{
    var tasks = Array<Task<string, error>>{{}}
    for var i = 0; i < 32; i += 1 {{ tasks.push(go token()) }}
    var tokens = map[string]bool{{}}
    for tasks.len() > 0 {{
        let _, t = tasks.pop()
        let value, err = t.wait()
        check(err == nil, "token")
        tokens[value] = true
    }}
    println(tokens.len())
}}
"#
    );
    prints(&source, "32\n");
}
