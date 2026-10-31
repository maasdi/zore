use super::*;

const CHECK: &str = r#"
func check(ok bool, label string) {
    if !ok { panic(label) }
}

func same(left []byte, right []byte) bool {
    if left.len() != right.len() { return false }
    for var i = 0; i < left.len(); i += 1 {
        if left[i] != right[i] { return false }
    }
    return true
}

func fails(err error, message string) bool {
    return errors.Message(err) == message
}
"#;

#[test]
fn hex_round_trips_every_byte_and_rejects_malformed_text() {
    let source = format!(
        r#"package main
import "std/encoding/hex"
import "std/errors"
import "std/strconv"
import "std/strings"
{CHECK}
func rejects(input string, offset int) {{
    let partial, bad = hex.DecodeString(input)
    check(fails(bad, "hex.DecodeString: invalid byte at offset " + strconv.Itoa(offset)) && partial.len() == 0, input)
}}

func main() {{
    var every = Array<byte>{{}}
    for var i = 0; i < 256; i += 1 {{ every.push(byte(i)) }}
    let text = hex.EncodeToString(every[:])
    check(text.len() == 512 && hex.EncodedLen(256) == 512, "encoded length")
    check(strings.HasPrefix(text, "000102") && strings.HasSuffix(text, "fdfeff"), "lower-case digits")
    let back, err = hex.DecodeString(text)
    check(err == nil && same(back[:], every[:]), "round trip")
    let upper, upperErr = hex.DecodeString(strings.ToUpper(text))
    check(upperErr == nil && same(upper[:], every[:]), "upper-case digits")
    let encoded = hex.Encode(Array<byte>{{222, 173, 190, 239}}[:])
    check(same(encoded[:], strings.Bytes("deadbeef")[:]), "Encode bytes")
    let mixed, mixedErr = hex.Decode(strings.Bytes("DeAdBeEf")[:])
    check(mixedErr == nil && same(mixed[:], Array<byte>{{222, 173, 190, 239}}[:]), "mixed case")

    check(hex.EncodeToString(Array<byte>{{}}[:]) == "", "empty encode")
    let empty, emptyErr = hex.DecodeString("")
    check(emptyErr == nil && empty.len() == 0, "empty decode")
    check(hex.DecodedLen(7) == 3 && hex.DecodedLen(0) == 0 && hex.EncodedLen(0) == 0, "lengths")
    check(hex.DecodedLen(9223372036854775807) == 4611686018427387903, "largest DecodedLen")

    let odd, oddErr = hex.DecodeString("abc")
    check(fails(oddErr, "hex.DecodeString: odd length") && odd.len() == 0, "odd length")
    let _, oddBytes = hex.Decode(strings.Bytes("zzz")[:])
    check(fails(oddBytes, "hex.Decode: odd length"), "length before bytes")
    rejects("0x00", 1)
    rejects("g0", 0)
    rejects("0g", 1)
    rejects("00 1", 2)
    rejects("12\n4", 2)
    rejects("-1", 0)
    rejects("ab+c", 2)
    rejects("0/", 1)
    rejects(":0", 0)
    rejects("@0", 0)
    rejects("0G", 1)
    rejects("`0", 0)
    let _, high = hex.Decode(Array<byte>{{48, 128}}[:])
    check(fails(high, "hex.Decode: invalid byte at offset 1"), "byte 80 in Decode")
    println("ok")
}}
"#
    );
    prints(&source, "ok\n");
}

#[test]
fn hex_lengths_panic_on_negative_and_overflowing_input() {
    for (call, message) in [
        ("hex.EncodedLen(-1)", "hex.EncodedLen: negative length"),
        ("hex.DecodedLen(-1)", "hex.DecodedLen: negative length"),
        (
            "hex.EncodedLen(4611686018427387904)",
            "hex.EncodedLen: length overflows int",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/encoding/hex\"\n\nfunc main() {{\n    println(hex.EncodedLen(4611686018427387903))\n    println({call})\n}}\n"
        );
        panics(&source, message, "9223372036854775806\n");
    }
}

#[test]
fn base64_matches_rfc_4648_vectors_in_all_four_encodings() {
    let source = format!(
        r#"package main
import "std/encoding/base64"
import "std/errors"
import "std/strings"
{CHECK}
func vector(input string, standard string, raw string) {{
    let data = strings.Bytes(input)
    check(base64.Standard.EncodeToString(data[:]) == standard, "Standard " + input)
    check(base64.URL.EncodeToString(data[:]) == standard, "URL " + input)
    check(base64.RawStandard.EncodeToString(data[:]) == raw, "RawStandard " + input)
    check(base64.RawURL.EncodeToString(data[:]) == raw, "RawURL " + input)
    for encoding in Array<base64.Encoding>{{base64.Standard, base64.URL}} {{
        let back, err = encoding.DecodeString(standard)
        check(err == nil && same(back[:], data[:]), "decode padded " + input)
    }}
    for encoding in Array<base64.Encoding>{{base64.RawStandard, base64.RawURL}} {{
        let back, err = encoding.DecodeString(raw)
        check(err == nil && same(back[:], data[:]), "decode raw " + input)
    }}
}}

func main() {{
    vector("", "", "")
    vector("f", "Zg==", "Zg")
    vector("fo", "Zm8=", "Zm8")
    vector("foo", "Zm9v", "Zm9v")
    vector("foob", "Zm9vYg==", "Zm9vYg")
    vector("fooba", "Zm9vYmE=", "Zm9vYmE")
    vector("foobar", "Zm9vYmFy", "Zm9vYmFy")

    let high = Array<byte>{{251, 239, 255}}
    check(base64.Standard.EncodeToString(high[:]) == "++//", "standard alphabet")
    check(base64.URL.EncodeToString(high[:]) == "--__", "URL alphabet")
    let encoded = base64.Standard.Encode(strings.Bytes("hi")[:])
    check(same(encoded[:], strings.Bytes("aGk=")[:]), "Encode bytes")
    let decoded, decodedErr = base64.URL.Decode(strings.Bytes("--__")[:])
    check(decodedErr == nil && same(decoded[:], high[:]), "Decode bytes")
    println("ok")
}}
"#
    );
    prints(&source, "ok\n");
}

#[test]
fn base64_round_trips_every_length_with_exact_length_functions() {
    let source = format!(
        r#"package main
import "std/encoding/base64"
import "std/errors"
{CHECK}
func main() {{
    var data = Array<byte>{{}}
    for var n = 0; n <= 70; n += 1 {{
        for encoding in Array<base64.Encoding>{{base64.Standard, base64.URL, base64.RawStandard, base64.RawURL}} {{
            let text = encoding.Encode(data[:])
            check(text.len() == encoding.EncodedLen(n), "EncodedLen")
            check(encoding.DecodedLen(text.len()) >= n, "DecodedLen bound")
            check(encoding.DecodedLen(text.len()) < n + 3, "DecodedLen tight")
            let back, err = encoding.Decode(text[:])
            check(err == nil && same(back[:], data[:]), "round trip")
        }}
        data.push(byte((n * 97 + 13) % 256))
    }}
    check(base64.Standard.EncodedLen(0) == 0 && base64.RawURL.EncodedLen(1) == 2, "small lengths")
    check(base64.RawURL.DecodedLen(3) == 2 && base64.RawURL.DecodedLen(2) == 1, "raw decoded lengths")
    check(base64.Standard.DecodedLen(8) == 6 && base64.Standard.DecodedLen(7) == 3, "padded decoded lengths")
    check(base64.Standard.DecodedLen(9223372036854775807) == 6917529027641081853, "largest padded DecodedLen")
    check(base64.RawStandard.DecodedLen(9223372036854775807) == 6917529027641081855, "largest raw DecodedLen")
    println("ok")
}}
"#
    );
    prints(&source, "ok\n");
}

#[test]
fn base64_decoding_is_strict_and_reports_the_first_rejected_byte() {
    let source = format!(
        r#"package main
import "std/encoding/base64"
import "std/errors"
{CHECK}
func rejects(encoding base64.Encoding, input string, problem string) {{
    let partial, err = encoding.DecodeString(input)
    check(fails(err, "base64.Encoding.DecodeString: " + problem) && partial.len() == 0, input)
}}

func main() {{
    rejects(base64.Standard, "Zg=", "invalid length")
    rejects(base64.Standard, "Zm9vY", "invalid length")
    rejects(base64.RawStandard, "Zm9vY", "invalid length")
    rejects(base64.RawURL, "Z", "invalid length")
    rejects(base64.Standard, "Zg", "invalid length")

    rejects(base64.Standard, "--__", "invalid byte at offset 0")
    rejects(base64.URL, "++//", "invalid byte at offset 0")
    rejects(base64.RawURL, "Zm9+", "invalid byte at offset 3")
    rejects(base64.Standard, "Zm9v Yg==", "invalid length")
    rejects(base64.Standard, "Zm9v\nYmFy", "invalid length")
    rejects(base64.Standard, "Zm 9", "invalid byte at offset 2")
    rejects(base64.Standard, "Zm9v\r\nYg", "invalid byte at offset 4")
    rejects(base64.Standard, "Z===", "invalid byte at offset 1")
    rejects(base64.Standard, "Zm=v", "invalid byte at offset 2")
    rejects(base64.Standard, "Z=m=", "invalid byte at offset 1")
    rejects(base64.Standard, "====", "invalid byte at offset 0")
    rejects(base64.Standard, "Zg==Zg==", "invalid byte at offset 2")
    rejects(base64.RawStandard, "Zg==", "invalid byte at offset 2")
    rejects(base64.RawURL, "Zm8=", "invalid byte at offset 3")

    rejects(base64.Standard, "Zh==", "nonzero padding bits at offset 1")
    rejects(base64.Standard, "Zm9=", "nonzero padding bits at offset 2")
    rejects(base64.RawURL, "Zh", "nonzero padding bits at offset 1")
    rejects(base64.RawStandard, "Zm9vYmF", "nonzero padding bits at offset 6")
    rejects(base64.Standard, "!h==", "invalid byte at offset 0")

    rejects(base64.Standard, "AA@A", "invalid byte at offset 2")
    rejects(base64.Standard, "AA[A", "invalid byte at offset 2")
    rejects(base64.Standard, "AA`A", "invalid byte at offset 2")
    rejects(base64.Standard, "AA{{A", "invalid byte at offset 2")
    rejects(base64.Standard, "AA,A", "invalid byte at offset 2")
    rejects(base64.URL, "AA:A", "invalid byte at offset 2")
    rejects(base64.URL, "AA/A", "invalid byte at offset 2")
    let _, high = base64.RawURL.Decode(Array<byte>{{65, 65, 128}}[:])
    check(fails(high, "base64.Encoding.Decode: invalid byte at offset 2"), "byte 80")

    let _, bytesErr = base64.Standard.Decode(Array<byte>{{0, 0, 0, 0}}[:])
    check(fails(bytesErr, "base64.Encoding.Decode: invalid byte at offset 0"), "Decode name")
    let empty, emptyErr = base64.Standard.DecodeString("")
    check(emptyErr == nil && empty.len() == 0, "empty input")
    println("ok")
}}
"#
    );
    prints(&source, "ok\n");
}

#[test]
fn base64_methods_panic_on_unknown_encodings_and_bad_lengths() {
    for (call, message) in [
        (
            "base64.Encoding(4).EncodeToString(Array<byte>{}[:])",
            "base64: unknown Encoding 4",
        ),
        (
            "base64.Encoding(-1).DecodedLen(4)",
            "base64: unknown Encoding -1",
        ),
        (
            "base64.Standard.EncodedLen(-1)",
            "base64.Encoding.EncodedLen: negative length",
        ),
        (
            "base64.RawURL.DecodedLen(-3)",
            "base64.Encoding.DecodedLen: negative length",
        ),
        (
            "base64.RawURL.EncodedLen(6917529027641081856)",
            "base64.Encoding.EncodedLen: length overflows int",
        ),
        (
            "base64.Standard.EncodedLen(6917529027641081854)",
            "base64.Encoding.EncodedLen: length overflows int",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/encoding/base64\"\n\nfunc main() {{\n    println(base64.Standard.EncodedLen(6917529027641081853))\n    println(base64.RawURL.EncodedLen(6917529027641081855))\n    println({call})\n}}\n"
        );
        panics(
            &source,
            message,
            "9223372036854775804\n9223372036854775807\n",
        );
    }
}

#[test]
fn every_base64_method_panics_on_an_unknown_encoding() {
    for (call, value) in [
        ("_ = base64.Encoding(4).EncodedLen(0)", "4"),
        ("_ = base64.Encoding(-1).Encode(Array<byte>{}[:])", "-1"),
        (
            "let _, _ = base64.Encoding(4).Decode(Array<byte>{}[:])",
            "4",
        ),
        ("let _, _ = base64.Encoding(-1).DecodeString(\"\")", "-1"),
    ] {
        let source = format!(
            "package main\nimport \"std/encoding/base64\"\n\nfunc main() {{\n    println(\"start\")\n    {call}\n}}\n"
        );
        panics(
            &source,
            &format!("base64: unknown Encoding {value}"),
            "start\n",
        );
    }
}

#[test]
fn byte_order_reads_writes_and_appends_every_width_at_its_boundaries() {
    let source = format!(
        r#"package main
import "std/encoding/binary"
import "std/errors"
{CHECK}
func main() {{
    let big = binary.BigEndian
    let little = binary.LittleEndian
    var bytes = Array<byte>{{1, 2, 3, 4, 5, 6, 7, 8}}

    let b16, e1 = big.Uint16(bytes[0:2])
    let l16, e2 = little.Uint16(bytes[0:2])
    check(e1 == nil, "Uint16 big")
    check(e2 == nil && b16 == 258 && l16 == 513, "Uint16")
    let b32, e3 = big.Uint32(bytes[0:4])
    let l32, e4 = little.Uint32(bytes[0:4])
    check(e3 == nil, "Uint32 big")
    check(e4 == nil && b32 == 16909060 && l32 == 67305985, "Uint32")
    let b64, e5 = big.Uint64(bytes[:])
    let l64, e6 = little.Uint64(bytes[:])
    check(e5 == nil, "Uint64 big")
    check(e6 == nil && b64 == 72623859790382856 && l64 == 578437695752307201, "Uint64")

    var ones = Array<byte>{{255, 255, 255, 255, 255, 255, 255, 255}}
    let u16, _ = big.Uint16(ones[0:2])
    let u32, _ = big.Uint32(ones[0:4])
    let u64, _ = big.Uint64(ones[:])
    check(u16 == 65535 && u32 == 4294967295 && u64 == 18446744073709551615, "unsigned maxima")
    let s16, _ = big.Int16(ones[0:2])
    let s32, _ = little.Int32(ones[0:4])
    let s64, _ = big.Int64(ones[:])
    check(s16 == -1 && s32 == -1 && s64 == -1, "minus one")

    var frame = Array<byte>{{}}
    big.AppendInt16(frame, -32768)
    big.AppendInt16(frame, 32767)
    little.AppendInt32(frame, -2147483648)
    little.AppendInt32(frame, 2147483647)
    big.AppendInt64(frame, -9223372036854775808)
    little.AppendInt64(frame, 9223372036854775807)
    check(same(frame[0:4], Array<byte>{{128, 0, 127, 255}}[:]), "big Int16 bytes")
    check(same(frame[4:12], Array<byte>{{0, 0, 0, 128, 255, 255, 255, 127}}[:]), "little Int32 bytes")
    check(same(frame[12:20], Array<byte>{{128, 0, 0, 0, 0, 0, 0, 0}}[:]), "big Int64 bytes")
    check(same(frame[20:28], Array<byte>{{255, 255, 255, 255, 255, 255, 255, 127}}[:]), "little Int64 bytes")
    let min16, _ = big.Int16(frame[0:2])
    let max16, _ = big.Int16(frame[2:4])
    let min32, _ = little.Int32(frame[4:8])
    let max32, _ = little.Int32(frame[8:12])
    let min64, _ = big.Int64(frame[12:20])
    let max64, _ = little.Int64(frame[20:28])
    check(min16 == -32768 && max16 == 32767, "Int16 round trip")
    check(min32 == -2147483648 && max32 == 2147483647, "Int32 round trip")
    check(min64 == -9223372036854775808 && max64 == 9223372036854775807, "Int64 round trip")

    var appended = Array<byte>{{9}}
    big.AppendUint16(appended, 258)
    little.AppendUint16(appended, 258)
    big.AppendUint32(appended, 16909060)
    little.AppendUint32(appended, 16909060)
    big.AppendUint64(appended, 18446744073709551615)
    little.AppendUint64(appended, 1)
    check(same(appended[:], Array<byte>{{9, 1, 2, 2, 1, 1, 2, 3, 4, 4, 3, 2, 1, 255, 255, 255, 255, 255, 255, 255, 255, 1, 0, 0, 0, 0, 0, 0, 0}}[:]), "Append bytes")

    var out = Array<byte>{{0, 0, 0, 0, 0, 0, 0, 0, 0, 0}}
    check(big.PutUint16(out[0:2], 258) == nil, "PutUint16")
    check(little.PutInt16(out[2:4], -2) == nil, "PutInt16")
    check(little.PutUint32(out[4:8], 4294967295) == nil, "PutUint32")
    check(big.PutInt32(out[6:10], -16909061) == nil, "PutInt32")
    check(same(out[:], Array<byte>{{1, 2, 254, 255, 255, 255, 254, 253, 252, 251}}[:]), "Put bytes")
    var wide = Array<byte>{{0, 0, 0, 0, 0, 0, 0, 0}}
    check(little.PutUint64(wide[:], 72623859790382856) == nil, "PutUint64")
    check(same(wide[:], Array<byte>{{8, 7, 6, 5, 4, 3, 2, 1}}[:]), "PutUint64 bytes")
    check(big.PutInt64(wide[:], -9223372036854775808) == nil, "PutInt64")
    check(same(wide[:], Array<byte>{{128, 0, 0, 0, 0, 0, 0, 0}}[:]), "PutInt64 bytes")
    println("ok")
}}
"#
    );
    prints(&source, "ok\n");
}

#[test]
fn byte_order_requires_the_exact_width_and_leaves_data_alone_on_error() {
    let source = format!(
        r#"package main
import "std/encoding/binary"
import "std/errors"
{CHECK}
func main() {{
    var data = Array<byte>{{1, 2, 3, 4, 5, 6, 7, 8, 9}}
    let short, shortErr = binary.BigEndian.Uint32(data[0:3])
    check(fails(shortErr, "binary.ByteOrder.Uint32: need 4 bytes, got 3") && short == 0, "short read")
    let long, longErr = binary.LittleEndian.Uint16(data[0:3])
    check(fails(longErr, "binary.ByteOrder.Uint16: need 2 bytes, got 3") && long == 0, "long read")
    let none, noneErr = binary.BigEndian.Int64(data[:])
    check(fails(noneErr, "binary.ByteOrder.Int64: need 8 bytes, got 9") && none == 0, "Int64 width")
    let _, emptyErr = binary.BigEndian.Int16(data[0:0])
    check(fails(emptyErr, "binary.ByteOrder.Int16: need 2 bytes, got 0"), "empty read")
    let _, int32Err = binary.BigEndian.Int32(data[0:5])
    check(fails(int32Err, "binary.ByteOrder.Int32: need 4 bytes, got 5"), "Int32 width")
    let _, uint64Err = binary.LittleEndian.Uint64(data[0:7])
    check(fails(uint64Err, "binary.ByteOrder.Uint64: need 8 bytes, got 7"), "Uint64 width")

    check(fails(binary.BigEndian.PutUint16(data[0:1], 65535), "binary.ByteOrder.PutUint16: need 2 bytes, got 1"), "PutUint16 width")
    check(fails(binary.BigEndian.PutUint32(data[0:8], 0), "binary.ByteOrder.PutUint32: need 4 bytes, got 8"), "PutUint32 width")
    check(fails(binary.LittleEndian.PutUint64(data[0:4], 0), "binary.ByteOrder.PutUint64: need 8 bytes, got 4"), "PutUint64 width")
    check(fails(binary.LittleEndian.PutInt16(data[0:3], -1), "binary.ByteOrder.PutInt16: need 2 bytes, got 3"), "PutInt16 width")
    check(fails(binary.BigEndian.PutInt32(data[0:2], -1), "binary.ByteOrder.PutInt32: need 4 bytes, got 2"), "PutInt32 width")
    check(fails(binary.BigEndian.PutInt64(data[:], -1), "binary.ByteOrder.PutInt64: need 8 bytes, got 9"), "PutInt64 width")
    check(same(data[:], Array<byte>{{1, 2, 3, 4, 5, 6, 7, 8, 9}}[:]), "data unchanged")
    println("ok")
}}
"#
    );
    prints(&source, "ok\n");
}

#[test]
fn byte_order_methods_panic_on_unknown_orders() {
    for call in [
        "let _, _ = binary.ByteOrder(2).Uint16(Array<byte>{0, 0}[:])",
        "binary.ByteOrder(-1).AppendUint32(data, 1)",
    ] {
        let source = format!(
            "package main\nimport \"std/encoding/binary\"\n\nfunc main() {{\n    var data = Array<byte>{{}}\n    println(\"start\")\n    {call}\n    println(data.len())\n}}\n"
        );
        let order = if call.contains("(-1)") { "-1" } else { "2" };
        panics(
            &source,
            &format!("binary: unknown ByteOrder {order}"),
            "start\n",
        );
    }
}

#[test]
fn decoding_errors_propagate_with_question_mark_and_leave_no_storage_behind() {
    let source = r#"package main
import "std/encoding/base64"
import "std/encoding/binary"
import "std/encoding/hex"
import "std/errors"

func frameLength(token string) (uint32, error) {
    let raw = base64.RawURL.DecodeString(token)?
    let header = hex.Decode(raw[:])?
    return binary.BigEndian.Uint32(header[0:4])
}

func main() {
    let header = hex.Encode(Array<byte>{0, 0, 1, 0}[:])
    let good = base64.RawURL.EncodeToString(header[:])
    let length, err = frameLength(good)
    println(length)
    println(err == nil)
    let _, bad = frameLength("!!")
    println(errors.Message(bad))
    let _, odd = frameLength(base64.RawURL.EncodeToString(Array<byte>{48}[:]))
    println(errors.Message(odd))
}
"#;
    prints(
        source,
        "256\ntrue\nbase64.Encoding.DecodeString: invalid byte at offset 0\nhex.Decode: odd length\n",
    );
}
