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

#[test]
fn sha_hashes_match_known_answers() {
    let source = r#"package main
import "std/crypto/sha1"
import "std/crypto/sha256"
import "std/crypto/sha512"
import "std/encoding/hex"
import "std/strings"
func show(data []byte) {
    let one = sha1.Sum(data)
    let two = sha256.Sum256(data)
    let five = sha512.Sum512(data)
    println(hex.EncodeToString(one[:]))
    println(hex.EncodeToString(two[:]))
    println(hex.EncodeToString(five[:]))
}
func main() {
    for text in [string; 4]{"", "abc", "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq", "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"} {
        let data = strings.Bytes(text)
        show(data[:])
    }
    let million = strings.Repeat("a", 1000000)
    let data = strings.Bytes(million)
    show(data[:])
}
"#;
    prints(
        source,
        "da39a3ee5e6b4b0d3255bfef95601890afd80709\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\ncf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e\na9993e364706816aba3e25717850c26c9cd0d89d\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f\n84983e441c3bd26ebaae4aa1f95129e5e54670f1\n248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1\n204a8fc6dda82f0a0ced7beb8e08a41657c16ef468b228a8279be331a703c33596fd15c13b1b07f9aa1d3bea57789ca031ad85c7a71dd70354ec631238ca3445\na49b2446a02c645bf419f995b67091253a04a259\ncf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1\n8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909\n34aa973cd4c4daa4f61eeb2bdbad27316534016f\ncdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0\ne718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b\n",
    );
}

#[test]
fn sha_digests_give_the_same_hash_for_any_split() {
    let source = r#"package main
import "std/crypto/sha1"
import "std/crypto/sha256"
import "std/crypto/sha512"
import "std/encoding/hex"
import "std/io"
func check(ok bool, label string) {
    if !ok { panic(label) }
}
func same(a []byte, b []byte) bool {
    if a.len() != b.len() { return false }
    for var i = 0; i < a.len(); i += 1 {
        if a[i] != b[i] { return false }
    }
    return true
}
func main() {
    var data = Array<byte>{}
    for var i = 0; i < 300; i += 1 { data.push(byte((i * 7 + 3) % 256)) }
    check(sha256.Size == 32 && sha256.BlockSize == 64, "sha256 sizes")
    check(sha512.Size == 64 && sha512.BlockSize == 128, "sha512 sizes")
    check(sha1.Size == 20 && sha1.BlockSize == 64, "sha1 sizes")
    var all1 = Array<byte>{}
    var all2 = Array<byte>{}
    var all5 = Array<byte>{}
    for var n = 0; n <= data.len(); n += 1 {
        let part = data[0:n]
        let whole1 = sha1.Sum(part)
        let whole2 = sha256.Sum256(part)
        let whole5 = sha512.Sum512(part)
        for b in whole1 { all1.push(b) }
        for b in whole2 { all2.push(b) }
        for b in whole5 { all5.push(b) }
        check(whole1.len() == 20 && whole2.len() == 32 && whole5.len() == 64, "lengths")
        var bytewise1 = sha1.New()
        var bytewise2 = sha256.New()
        var bytewise5 = sha512.New()
        for var i = 0; i < n; i += 1 {
            let w1, e1 = bytewise1.Write(part[i:i + 1])
            let w2, e2 = bytewise2.Write(part[i:i + 1])
            let w5, e5 = bytewise5.Write(part[i:i + 1])
            check(e1 == nil, "sha1 write error")
            check(e2 == nil, "sha256 write error")
            check(e5 == nil, "sha512 write error")
            check(w1 == 1 && w2 == 1 && w5 == 1, "write count")
        }
        let b1 = bytewise1.Sum()
        let b2 = bytewise2.Sum()
        let b5 = bytewise5.Sum()
        check(same(b1[:], whole1[:]), "sha1 bytewise")
        check(same(b2[:], whole2[:]), "sha256 bytewise")
        check(same(b5[:], whole5[:]), "sha512 bytewise")
        var split1 = sha1.New()
        var split2 = sha256.New()
        var split5 = sha512.New()
        let cut = n / 3
        let _, _ = split1.Write(part[0:cut])
        let _, _ = split2.Write(part[0:cut])
        let _, _ = split5.Write(part[0:cut])
        let early1 = split1.Sum()
        let early2 = split2.Sum()
        let early5 = split5.Sum()
        let prefix1 = sha1.Sum(part[0:cut])
        let prefix2 = sha256.Sum256(part[0:cut])
        let prefix5 = sha512.Sum512(part[0:cut])
        check(same(early1[:], prefix1[:]) && same(early2[:], prefix2[:]) && same(early5[:], prefix5[:]), "sum mid-stream")
        let again1 = split1.Sum()
        let again2 = split2.Sum()
        let again5 = split5.Sum()
        check(same(again1[:], early1[:]) && same(again2[:], early2[:]) && same(again5[:], early5[:]), "repeated sum")
        let _, _ = io.WriteString(split1, "")
        let _, _ = io.WriteString(split2, "")
        let _, _ = io.WriteString(split5, "")
        let _, _ = split1.Write(part[cut:n])
        let _, _ = split2.Write(part[cut:n])
        let _, _ = split5.Write(part[cut:n])
        let s1 = split1.Sum()
        let s2 = split2.Sum()
        let s5 = split5.Sum()
        check(same(s1[:], whole1[:]), "sha1 split")
        check(same(s2[:], whole2[:]), "sha256 split")
        check(same(s5[:], whole5[:]), "sha512 split")
        split1.Reset()
        split2.Reset()
        split5.Reset()
        let _, _ = split1.Write(part)
        let _, _ = split2.Write(part)
        let _, _ = split5.Write(part)
        let r1 = split1.Sum()
        let r2 = split2.Sum()
        let r5 = split5.Sum()
        check(same(r1[:], whole1[:]) && same(r2[:], whole2[:]) && same(r5[:], whole5[:]), "reset")
    }
    println(hex.EncodeToString(all1[:]).len())
    let fold1 = sha256.Sum256(all1[:])
    let fold2 = sha256.Sum256(all2[:])
    let fold5 = sha256.Sum256(all5[:])
    println(hex.EncodeToString(fold1[:]))
    println(hex.EncodeToString(fold2[:]))
    println(hex.EncodeToString(fold5[:]))
}
"#;
    prints(
        source,
        "12040\n67888d6b2a15b4bc43c5788ce28aaa0a241369a940ef796d976aac3bbd00740e\n7d917fbd2cf49ddff9ad0a8706bba32d204e92e71d2e369c5a03d6af29278c9f\n60aaa3762fae9f89010087979a48af59703cbdd193aa92fd1ffe54f969edc0e7\n",
    );
}

#[test]
fn hmac_matches_rfc_4231_and_handles_key_sizes() {
    let source = r#"package main
import "std/crypto/hmac"
import "std/crypto/subtle"
import "std/encoding/hex"
func show(keyText string, dataText string) {
    let key, keyErr = hex.DecodeString(keyText)
    if keyErr != nil { panic("hex") }
    let data, dataErr = hex.DecodeString(dataText)
    if dataErr != nil { panic("hex") }
    let a = hmac.SHA256(key[:], data[:])
    let b = hmac.SHA512(key[:], data[:])
    println(hex.EncodeToString(a[:]))
    println(hex.EncodeToString(b[:]))
}
func sha1Line(keyText string, dataText string) {
    let key, keyErr = hex.DecodeString(keyText)
    if keyErr != nil { panic("hex") }
    let data, dataErr = hex.DecodeString(dataText)
    if dataErr != nil { panic("hex") }
    let mac = hmac.SHA1(key[:], data[:])
    println(hex.EncodeToString(mac[:]))
}
func main() {
    show("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b", "4869205468657265")
    show("4a656665", "7768617420646f2079612077616e7420666f72206e6f7468696e673f")
    show("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd")
    show("0102030405060708090a0b0c0d0e0f10111213141516171819", "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd")
    show("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "54657374205573696e67204c6172676572205468616e20426c6f636b2d53697a65204b6579202d2048617368204b6579204669727374")
    show("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "5468697320697320612074657374207573696e672061206c6172676572207468616e20626c6f636b2d73697a65206b657920616e642061206c6172676572207468616e20626c6f636b2d73697a6520646174612e20546865206b6579206e6565647320746f20626520686173686564206265666f7265206265696e6720757365642062792074686520484d414320616c676f726974686d2e")
    show("", "")
    var k64 = Array<byte>{}
    for var i = 0; i < 128; i += 1 { k64.push(byte(i)) }
    let x = Array<byte>{120}
    let a = hmac.SHA256(k64[0:64], x[:])
    let b = hmac.SHA512(k64[:], x[:])
    println(hex.EncodeToString(a[:]))
    println(hex.EncodeToString(b[:]))
    var k65 = Array<byte>{}
    for var i = 0; i < 64; i += 1 { k65.push(byte(i)) }
    k65.push(33)
    var k129 = Array<byte>{}
    for var i = 0; i < 128; i += 1 { k129.push(byte(i)) }
    k129.push(33)
    let c = hmac.SHA256(k65[:], x[:])
    let d = hmac.SHA512(k129[:], x[:])
    println(hex.EncodeToString(c[:]))
    println(hex.EncodeToString(d[:]))
    var k1000 = Array<byte>{}
    for var i = 0; i < 1000; i += 1 { k1000.push(byte(i % 256)) }
    let e1 = hmac.SHA256(k1000[0:63], x[:])
    let e2 = hmac.SHA256(k1000[:], x[:])
    let e3 = hmac.SHA512(k1000[0:127], x[:])
    let e4 = hmac.SHA512(k1000[:], x[:])
    println(hex.EncodeToString(e1[:]))
    println(hex.EncodeToString(e2[:]))
    println(hex.EncodeToString(e3[:]))
    println(hex.EncodeToString(e4[:]))
    sha1Line("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b", "4869205468657265")
    sha1Line("4a656665", "7768617420646f2079612077616e7420666f72206e6f7468696e673f")
    sha1Line("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "54657374205573696e67204c6172676572205468616e20426c6f636b2d53697a65204b6579202d2048617368204b6579204669727374")
    sha1Line("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f", "78")
    sha1Line("", "")
    let again = hmac.SHA256(k65[:], x[:])
    println(subtle.ConstantTimeEqual(c[:], again[:]))
    let other = hmac.SHA256(k64[0:64], x[:])
    println(subtle.ConstantTimeEqual(c[:], other[:]))
}
"#;
    prints(
        source,
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7\n87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cdedaa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854\n5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843\n164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737\n773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe\nfa73b0089d56a284efb0f0756c890be9b1b5dbdd8ee81a3655f83e33b2279d39bf3e848279a722c806b485a47e67c807b946a337bee8942674278859e13292fb\n82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b\nb0ba465637458c6990e5a8c5f61d4af7e576d97ff94b872de76f8050361ee3dba91ca5c11aa25eb4d679275cc5788063a5f19741120c4f2de2adebeb10a298dd\n60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54\n80b24263c7c1a3ebb71493c1dd7be8b49b46d1f41b4aeec1121b013783f8f3526b56d037e05f2598bd0fd2215d6a1e5295e64f73f63f0aec8b915a985d786598\n9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2\ne37b6a775dc87dbaa4dfa9f96e5e3ffddebd71f8867289865df5a32d20cdc944b6022cac3c4982b10d5eeb55c3e4de15134676fb6de0446065c97440fa8c6a58\nb613679a0814d9ec772f95d778c35fc5ff1697c493715653c6c712144292c5ad\nb936cee86c9f87aa5d3c6f2e84cb5a4239a5fe50480a6ec66b70ab5b1f4ac6730c6c515421b327ec1d69402e53dfb49ad7381eb067b338fd7b0cb22247225d47\nb8dc75d4220cfd86a5bf7c4301b3698f142a66fd7c004627df558626edb480cd\ndf0d02dfb105a623db2ca1ac8108103706a65c7aea41d702c8d26f285f1174be2bd9f094e3ce6a3ffdd76633236ee61e64a60ed80e9aa19fdd7eef2dfb5575c9\n1472ca3b7ce487865cfbcba4d890f1199715de8ec225b912285e16032768e6dd\n189d449f2c721aac66ad3dcbdbcb92f077e9f70b73d55e5d73e3d6b241160f15838e03eb4f1b479357f40964077c288e1459275feae846180047b35f8392f49a\nfb5f49d0867564e7dcea4ad21ec4ad1654761f4c5878694d9c1f7fa1309edf8e\n6273d115a5ccf79411c838f52c548d4c760fa0846e9fd229d0d8ffd25246715f\n52100d4fff41fc6bc0187305722ec9f61054c9468ef4fadc0557876e042f6c12325444244c311fc3cd03444f04206caa50fc7f8b4ac7b5185d8792d4dd0dd723\n4d549427d3d8dd9b8de9ae1c0b692385517eb5b300bce422696a130ea342b1868db836dd10eb98f6d619ba967664349dbe6cc9625d04a522610d6ef585966be6\nb617318655057264e28bc0b6fb378c8ef146be00\neffcdf6ae5eb2fa2d27416d5f184df9c259a7c79\naa4ae5e15272d00e95705637ce8a3b55ed402112\nea0254d7ad556218f4722ee78bba1dd24b873a5c\nfbdb1d1b18aa6c08324b7d64b71fb76370690e1d\ntrue\nfalse\n",
    );
}

#[test]
fn sha256_digest_hashes_a_file_through_io_copy() {
    let source = r#"package main
import "std/crypto/sha256"
import "std/encoding/hex"
import "std/io"
import "std/os"
import "std/strings"
func fileHash(path string) (string, error) {
    var file = os.Open(path)?
    var digest = sha256.New()
    _ = io.Copy(digest, file)?
    let sum = digest.Sum()
    return hex.EncodeToString(sum[:]), nil
}
func main() {
    let dir, dirErr = os.MkdirTemp(os.TempDir(), "sha")
    if dirErr != nil { panic("temp") }
    let path = dir + "/data.bin"
    let text = strings.Repeat("zore hashes files ", 20000)
    let data = strings.Bytes(text)
    if os.WriteFile(path, data[:], 420) != nil { panic("write") }
    let fromFile, err = fileHash(path)
    if err != nil { panic("hash") }
    let direct = sha256.Sum256(data[:])
    println(fromFile == hex.EncodeToString(direct[:]))
    println(fromFile)
    _ = os.RemoveAll(dir)
}
"#;
    prints(
        source,
        "true\na709946f9c8bf0c894cba13fecc4ea5f83a32f2c4a2ff429b27979465a19e860\n",
    );
}
