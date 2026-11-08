#[test]
fn archive_io_and_crc_are_available_without_native_linking() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/archive"
import "std/hash/crc32"
import "std/io"
import "std/os"
func read(path string) error {
    let file = os.Open(path)?
    var reader = archive.NewZIPReader(file, 10000, 5000, 10)?
    let header, present, err = reader.Next()
    if err != nil { return err }
    if present {
        println(header.Name)
        let data = io.ReadAll(reader)?
        println(crc32.Checksum(data[:]))
    }
    return reader.Close()
}
async func write(path string) error {
    let file = os.Create(path)?
    var writer = archive.NewTARWriter(file, 10000, 5000, 10)?
    writer.WriteHeader(archive.Header{Name: "empty", Size: 0, Directory: false})?
    writer.Finish()?
    return writer.Close()
}
func main() {}
"#,
    )]);
}

#[test]
fn archive_resources_preserve_move_and_visibility_rules() {
    for (body, expected) in [
        (
            "let file = os.Open(\"x\")?; let reader = archive.NewTARReader(file, 100, 100, 1)?; return file.Close()",
            "moved",
        ),
        (
            "let file = os.Create(\"x\")?; let writer = archive.NewZIPWriter(file, 100, 100, 1)?; let moved = writer; return writer.Close()",
            "moved",
        ),
        (
            "let file = os.Open(\"x\")?; let reader = archive.NewZIPReader(file, 100, 100, 1)?; reader.Close()?; return reader.Close()",
            "moved",
        ),
        (
            "let file = os.Open(\"x\")?; let reader = archive.NewTARReader(file, 100, 100, 1)?; println(reader.remaining); return reader.Close()",
            "not exported",
        ),
        (
            "let file = os.Create(\"x\")?; let writer = archive.NewZIPWriter(file, 100, 100, 1)?; println(writer.crc); return writer.Close()",
            "not exported",
        ),
        (
            "let file = os.Open(\"x\")?; let reader = archive.NewZIPReader(file, 100, 100, 1)?; let _, _, err = reader.Next(); return err",
            "mutable",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/archive\"\nimport \"std/os\"\nfunc run() error {{ {body} }}\nfunc main() {{}}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
}

#[test]
fn archive_writers_stay_move_for_copy_sinks_and_recover_concrete_file_types() {
    for name in ["TAR", "ZIP"] {
        let source = format!(
            r#"package main
import "std/archive"
type CopySink struct {{}}
func (s CopySink) Write(data []byte) (int, error) {{ return data.len(), nil }}
func run() error {{
    var writer = archive.New{name}Writer(CopySink{{}}, 10000, 10, 1)?
    let moved = writer
    return writer.Finish()
}}
func main() {{}}
"#
        );
        super::rejects(&[("main.ore", &source)], "moved");
    }
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/archive"
import "std/os"
func recover(path string) (os.File, error) {
    let file = os.Create(path)?
    var writer = archive.NewTARWriter(file, 1024, 0, 0)?
    return writer.Sink()
}
func main() {}
"#,
    )]);
}
