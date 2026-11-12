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
    let entry, present, err = reader.Next()
    if err != nil { return err }
    if present {
        println(entry.Header.Name)
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

#[test]
fn archive_deflate_extraction_and_staged_tree_public_contracts_check_without_native_linking() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/archive"
import "std/compress/flate"
import "std/io"
import "std/os"
import "std/path"
func extract(source own io.ReadSeeker, destination string) error {
    return archive.ExtractZIP(source, destination, 10000, 50000, 10)
}
async func write(destination string) error {
    var tree = os.StageTree(destination, 10)?
    tree.Create("dir/file")?
    var encoder = flate.NewEncoder(0, 100)?
    flate.Finish(encoder, tree)?
    tree.FinishFile()?
    if !path.IsLocal("dir/file") { return tree.Abort() }
    return tree.Commit()
}
func main() {}
"#,
    )]);
    for (body, expected) in [
        (
            "let tree = os.StageTree(\"result\", 10)?; let moved = tree; return tree.Abort()",
            "moved",
        ),
        (
            "let tree = os.StageTree(\"result\", 10)?; println(tree.id); return tree.Abort()",
            "not exported",
        ),
        (
            "let source = os.Open(\"input\")?; archive.ExtractZIP(source, \"result\", 10000, 10000, 10)?; return source.Close()",
            "moved",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/archive\"\nimport \"std/os\"\nfunc run() error {{ println(archive.MaxEntries); {body} }}\nfunc main() {{}}\n"
        );
        super::rejects(&[("main.ore", &source)], expected);
    }
    for state in ["Decoder", "Encoder"] {
        let source = format!(
            "package main\nimport \"std/compress/flate\"\nfunc run() error {{ let state = flate.New{state}(10, 10)?; let moved = state; println(state.InputBytes()); return nil }}\nfunc main() {{}}\n"
        );
        super::rejects(&[("main.ore", &source)], "moved");
    }
}

#[test]
fn archive_entries_are_copy_values_and_kinds_are_typed() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/archive"
import "std/os"
func list(path string) (Array<archive.Entry>, error) {
    let file = os.Open(path)?
    var reader = archive.NewTARReader(file, 10000, 5000, 10)?
    var entries = Array<archive.Entry>{}
    for {
        let entry, present, err = reader.Next()
        if err != nil { return entries, err }
        if !present { break }
        let copied = entry
        if copied.Kind == archive.SymlinkEntry || entry.Kind == archive.HardLinkEntry { println(entry.Link) }
        entries.push(entry)
    }
    return entries, reader.Close()
}
async func write(path string) error {
    let file = os.Create(path)?
    var writer = archive.NewZIPWriter(file, 10000, 5000, 10)?
    writer.WriteEntryWith(archive.Entry{Header: archive.Header{Name: "run", Size: 0, Directory: false}, Kind: archive.FileEntry, Link: "", Executable: true}, archive.Deflate)?
    return writer.Close()
}
func create(tree mut os.StagedTree) error {
    tree.CreateExecutable("run")?
    tree.Symlink("alias", "run")?
    return tree.Link("again", "run")
}
func unpack(path string) error {
    let file = os.Open(path)?
    var options = archive.DefaultExtractOptions(10000, 5000, 10)
    options.Links = true
    return archive.ExtractTARWith(file, "out", options)
}
func main() {}
"#,
    )]);
    for (body, expected) in [
        (
            "let entry = archive.Entry{Header: archive.Header{Name: \"a\", Size: 0, Directory: false}, Kind: 2, Link: \"\"}; println(entry.Link)",
            "missing field",
        ),
        (
            "let entry = archive.Entry{Header: archive.Header{Name: \"a\", Size: 0, Directory: false}, Kind: archive.FileEntry, Link: \"\", Executable: 1}; println(entry.Link)",
            "mismatched types",
        ),
    ] {
        let source = format!("package main\nimport \"std/archive\"\nfunc main() {{ {body} }}\n");
        super::rejects(&[("main.ore", &source)], expected);
    }
}
