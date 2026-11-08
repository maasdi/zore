#[test]
fn gzip_wrappers_compose_with_io_and_consume_endpoints() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/compress/gzip"
import "std/bytes"
import "std/io"
func read() (Array<byte>, error) {
    var r = gzip.NewReader(bytes.NewBufferString(""), 100, 1000)?
    let output, err = io.ReadAll(r)
    if err != nil { return output, err }
    r.Close()?
    return output, nil
}
async func write() error {
    var w = gzip.NewWriter(bytes.NewBufferString(""), 100, 1000)?
    let _, err = io.WriteString(w, "hello")
    if err != nil { return err }
    w.Finish()?
    return w.Close()
}
func main() {}
"#,
    )]);
}

#[test]
fn gzip_resources_and_private_state_enforce_ownership() {
    for (body, expected) in [
        (
            "let source = bytes.NewBufferString(\"\"); let reader = gzip.NewReader(source, 10, 10)?; println(source.Len()); return reader.Close()",
            "moved",
        ),
        (
            "let reader = gzip.NewReader(bytes.NewBufferString(\"\"), 10, 10)?; let moved = reader; return reader.Close()",
            "moved",
        ),
        (
            "let writer = gzip.NewWriter(bytes.NewBufferString(\"\"), 10, 10)?; writer.Close()?; return writer.Close()",
            "moved",
        ),
        (
            "let reader = gzip.NewReader(bytes.NewBufferString(\"\"), 10, 10)?; println(reader.crc); return reader.Close()",
            "not exported",
        ),
        (
            "let writer = gzip.NewWriter(bytes.NewBufferString(\"\"), 10, 10)?; println(writer.crc); return writer.Close()",
            "not exported",
        ),
        (
            "let writer = gzip.NewWriter(bytes.NewBufferString(\"\"), 10, 10)?; return writer.Finish()",
            "mutable",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/compress/gzip\"\nimport \"std/bytes\"\nfunc run() error {{ {body} }}\nfunc main() {{}}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}
