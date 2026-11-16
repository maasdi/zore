use super::*;
use std::time::Duration;

const PROGRAM: &str = r#"package main
import "std/encoding/hex"
import "std/errors"
import "std/net/http/hpack"
import "std/os"
import "std/strconv"
import "std/strings"

func decode(d mut hpack.Decoder, text string) {
    let block, hexErr = hex.DecodeString(text)
    if hexErr != nil {
        println("bad hex")
        return
    }
    let fields, err = d.Decode(block[:])
    if err != nil {
        println("error " + errors.Message(err))
        return
    }
    for field in fields {
        var mark = ""
        if field.Sensitive {
            mark = " (sensitive)"
        }
        println(field.Name + ": " + field.Value + mark)
    }
    println("table " + strconv.Itoa(d.TableBytes()))
}

func encode(e mut hpack.Encoder, d mut hpack.Decoder, text string) {
    var fields = Array<hpack.Field>{}
    for pair in strings.Split(text, ";") {
        let name, value, _ = strings.Cut(pair, "=")
        let secret = strings.HasPrefix(name, "!")
        fields.push(hpack.Field{Name: strings.TrimPrefix(name, "!"), Value: value, Sensitive: secret})
    }
    let block = e.Encode(fields[:])
    println(hex.EncodeToString(block[:]))
    let back, err = d.Decode(block[:])
    if err != nil {
        println("error " + errors.Message(err))
        return
    }
    var same = back.len() == fields.len()
    for i, field in back {
        same = same && field.Name == fields[i].Name && field.Value == fields[i].Value && (field.Sensitive || !fields[i].Sensitive)
    }
    println("round trip " + strconv.Itoa(back.len()) + " " + strconv.Itoa(d.TableBytes()))
    if !same {
        println("round trip differs")
    }
}

func main() {
    let args = os.Args()
    let tableBytes, _ = strconv.Atoi(args[1])
    let listBytes, _ = strconv.Atoi(args[2])
    var decoder, err = hpack.NewDecoder(tableBytes, listBytes)
    if err != nil {
        println("error " + errors.Message(err))
    }
    var encoder = hpack.NewEncoder()
    for var i = 3; i < args.len(); i += 1 {
        let arg = args[i]
        if strings.HasPrefix(arg, "enc:") {
            encode(encoder, decoder, strings.TrimPrefix(arg, "enc:"))
        } else if strings.HasPrefix(arg, "limit:") {
            let limit, _ = strconv.Atoi(strings.TrimPrefix(arg, "limit:"))
            let limitErr = encoder.SetTableLimit(limit)
            if limitErr != nil {
                println("error " + errors.Message(limitErr))
            }
        } else if strings.HasPrefix(arg, "big:") {
            let size, _ = strconv.Atoi(strings.TrimPrefix(arg, "big:"))
            encode(encoder, decoder, "x-big=" + strings.Repeat("a", size))
        } else {
            decode(decoder, arg)
        }
    }
}
"#;

fn run(executable: &Path, args: &[&str]) -> String {
    let output = bounded_process::run(
        Command::new(executable)
            .args(args)
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(output.status.success(), "{args:?}: {}", stderr(&output));
    assert!(output.stderr.is_empty(), "{args:?}: {}", stderr(&output));
    stdout(&output)
}

const REQUEST: &str = ":method: GET\n:scheme: http\n:path: /\n:authority: www.example.com\n";
const DATE: &str = "Mon, 21 Oct 2013 20:13:21 GMT";

#[test]
fn hpack_decodes_reference_vectors_and_bounds_hostile_blocks() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("hpack");
    build_file(PROGRAM, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));

    let second = format!("{REQUEST}cache-control: no-cache\ntable 110\n");
    let third = ":method: GET\n:scheme: https\n:path: /index.html\n:authority: www.example.com\n\
                 custom-key: custom-value\ntable 164\n";
    let expected = format!("{REQUEST}table 57\n{second}{third}");
    assert_eq!(
        run(
            &executable,
            &[
                "4096",
                "65536",
                "828684410f7777772e6578616d706c652e636f6d",
                "828684be58086e6f2d6361636865",
                "828785bf400a637573746f6d2d6b65790c637573746f6d2d76616c7565",
            ]
        ),
        expected
    );
    assert_eq!(
        run(
            &executable,
            &[
                "4096",
                "65536",
                "828684418cf1e3c2e5f23a6ba0ab90f4ff",
                "828684be5886a8eb10649cbf",
                "828785bf408825a849e95ba97d7f8925a849e95bb8e8b4bf",
            ]
        ),
        expected
    );

    let location = "location: https://www.example.com\n";
    let cookie = "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1";
    assert_eq!(
        run(
            &executable,
            &[
                "256",
                "65536",
                "488264025885aec3771a4b6196d07abe941054d444a8200595040b8166e082a62d1bff6e919d29ad171863c78f0b97c8e9ae82ae43d3",
                "4883640effc1c0bf",
                "88c16196d07abe941054d444a8200595040b8166e084a62d1bffc05a839bd9ab77ad94e7821dd7f2e6c7b335dfdfcd5b3960d5af27087f3672c1ab270fb5291f9587316065c003ed4ee5b1063d5007"
            ]
        ),
        format!(
            ":status: 302\ncache-control: private\ndate: {DATE}\n{location}table 222\n\
             :status: 307\ncache-control: private\ndate: {DATE}\n{location}table 222\n\
             :status: 200\ncache-control: private\ndate: Mon, 21 Oct 2013 20:13:22 GMT\n{location}\
             content-encoding: gzip\nset-cookie: {cookie}\ntable 215\n"
        )
    );

    for (block, message) in [
        ("80", "invalid table index"),
        ("be", "invalid table index"),
        ("ff80808080", "integer too large"),
        ("ff8080", "truncated header block"),
        ("4003616263", "truncated header block"),
        ("400361", "truncated header block"),
        ("3fe21f", "table size update too large"),
        ("823f00", "misplaced table size update"),
        ("202020", "misplaced table size update"),
        ("0085ffffffffff01", "invalid Huffman data"),
        ("0081180161", "invalid Huffman data"),
        ("0081fe0161", "invalid Huffman data"),
        ("00821fff0161", "invalid Huffman data"),
        ("0002c328016100", "invalid field text"),
    ] {
        let output = run(&executable, &["4096", "65536", block, "82"]);
        assert_eq!(
            output,
            format!("error hpack: {message}\nerror hpack: decoder failed\n"),
            "{block}"
        );
    }

    assert_eq!(
        run(&executable, &["4096", "65536", "3fe11f", "20", "82"]),
        "table 0\ntable 0\n:method: GET\ntable 0\n"
    );
    assert_eq!(
        run(&executable, &["4096", "80", "8284"]),
        ":method: GET\n:path: /\ntable 0\n"
    );
    assert_eq!(
        run(&executable, &["4096", "79", "8284"]),
        "error hpack: header list too large\n"
    );
    assert_eq!(
        run(&executable, &["4096", "40", "00016186000000000007"]),
        "error hpack: header list too large\n"
    );
    assert_eq!(
        run(&executable, &["4096", "40", "000161086161616161616161"]),
        "error hpack: header list too large\n"
    );
    assert_eq!(
        run(&executable, &["4096", "41", "000161086161616161616161"]),
        "a: aaaaaaaa\ntable 0\n"
    );
    assert_eq!(
        run(&executable, &["4096", "42", "00016186000000000007"]),
        "a: 000000000\ntable 0\n"
    );
    assert_eq!(
        run(
            &executable,
            &["4096", "200", "100161016200016101621f080163"]
        ),
        "a: b (sensitive)\na: b\nauthorization: c (sensitive)\ntable 0\n"
    );

    let mut args = vec!["4096".to_string(), "65536".to_string(), "3f45".to_string()];
    for i in 0..40u8 {
        args.push(format!("4001610{}{:02x}", 1, b'A' + i));
    }
    args.push("bebf".to_string());
    args.push("c0".to_string());
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    let output = run(&executable, &args);
    assert!(
        output
            .ends_with("a: h\ntable 68\na: h\na: g\ntable 68\nerror hpack: invalid table index\n"),
        "{output}"
    );

    for limits in [
        ["-1", "100"],
        ["65537", "100"],
        ["0", "0"],
        ["0", "16777217"],
    ] {
        assert_eq!(
            run(&executable, &[limits[0], limits[1], "82"]),
            "error hpack: invalid limit\nerror hpack: decoder failed\n"
        );
    }
}

#[test]
fn hpack_encodes_fields_a_decoder_reads_back() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("hpack");
    build_file(PROGRAM, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));

    assert_eq!(
        run(
            &executable,
            &[
                "4096",
                "65536",
                "enc::method=GET;:scheme=http;:path=/;:authority=www.example.com",
                "enc:cache-control=no-cache;custom-key=custom-value;x=1",
                "enc:authorization=Bearer abc;cookie=;:status=200",
                "limit:100",
                "enc::method=POST",
                "enc::method=POST",
                "limit:5000",
                "limit:-1",
                "enc:!x-api-key=k;!:method=GET",
            ]
        ),
        "828684018cf1e3c2e5f23a6ba0ab90f4ff\nround trip 4 0\n\
         0f0986a8eb10649cbf008825a849e95ba97d7f8925a849e95bb8e8b4bf0001780131\nround trip 3 0\n\
         1f0887ba51d85b141c641f110088\nround trip 3 0\n\
         3f4583\nround trip 1 0\n\
         83\nround trip 1 0\n\
         error hpack: invalid limit\n\
         1087f2b0eb32dd4beb016b1203474554\nround trip 2 0\n"
    );

    let big = run(&executable, &["4096", "65536", "big:300", "big:5000"]);
    let lines: Vec<_> = big.lines().collect();
    assert_eq!(lines.len(), 4, "{big}");
    assert!(lines[0].starts_with("0084f2b4669bff3d18c6318c63"));
    assert_eq!(lines[0].len(), 2 * (8 + 188));
    assert!(lines[0].ends_with("3f"));
    assert_eq!(lines[1], "round trip 1 0");
    assert!(lines[2].starts_with("0084f2b4669bffb61718c6318c63"));
    assert_eq!(lines[3], "round trip 1 0");

    assert_eq!(
        run(&executable, &["4096", "300", "big:300"]).lines().nth(1),
        Some("error hpack: header list too large")
    );
}

#[test]
fn hpack_guide_example_round_trips_a_header_block() {
    let guide = include_str!("../../docs/http-header-compression.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("guide");
    build_file(blocks[0], &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    assert_eq!(
        run(&executable, &[]),
        ":method: GET\n:path: /status\naccept: text/plain\n"
    );
}
