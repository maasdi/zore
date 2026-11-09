use super::*;
use std::time::Duration;

fn execute(source: &str, root: &Path, controlled: bool) -> Output {
    let mut source = source.to_string();
    for package in ["archive", "bytes", "errors", "io", "os", "flate"] {
        if !source.contains(&format!("{package}.")) {
            let import_path = if package == "flate" {
                "compress/flate"
            } else {
                package
            };
            source = source.replace(&format!("import \"std/{import_path}\"\n"), "");
        }
    }
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("archive-followup");
    if controlled {
        controlled_runtime::build_with_workers(&source, &executable, 1);
    } else {
        build_file(&source, &executable).unwrap_or_else(|e| panic!("{e:?}"));
    }
    bounded_process::run(
        Command::new(executable)
            .arg(root)
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap()
}
const SUPPORT: &str = r#"package main
import "std/archive"
import "std/bytes"
import "std/errors"
import "std/io"
import "std/os"
import "std/compress/flate"
func check(ok bool, label string) { if !ok { panic(label) } }
"#;

#[test]
fn raw_flate_borrowed_boundaries_limits_and_finalization() {
    let source = format!(
        r#"{SUPPORT}
func scenario() error {{
    let raw = Array<byte>{{1,5,0,250,255,104,101,108,108,111,77}}
    var source = bytes.NewBuffer(raw[:])
    var decoder = flate.NewDecoder(10, 5)?
    var buf = [byte; 8]{{0,0,0,0,0,0,0,0}}
    let count, end = flate.Read(decoder, source, buf[:])
    check(end == io.EOF && count == 5, "raw payload and final block")
    check(decoder.InputBytes() == 10 && decoder.OutputBytes() == 5, "raw counts")
    check(source.ReadByte()? == 77, "raw decoder does not prefetch framing")
    let _, repeated = flate.Read(decoder, source, buf[:])
    check(repeated == io.EOF, "sticky raw EOF")
    var limited = flate.NewDecoder(10, 4)?
    var again = bytes.NewBuffer(raw[:10])
    let accepted, limitErr = flate.Read(limited, again, buf[:])
    check(limitErr == error("flate: output byte limit exceeded") && accepted == 4, "raw output ceiling")
    var inputLimited = flate.NewDecoder(9, 5)?
    var short = bytes.NewBuffer(raw[:10])
    let _, inputErr = flate.Read(inputLimited, short, buf[:])
    check(inputErr == error("flate: input byte limit exceeded"), "raw input ceiling")
    var sink = bytes.NewBuffer(Array<byte>{{}}[:])
    var encoder = flate.NewEncoder(5, 100)?
    let wrote, writeErr = flate.Write(encoder, sink, raw[5:10])
    if writeErr != nil {{ return writeErr }}
    check(wrote == 5, "raw accepted count")
    flate.Finish(encoder, sink)?
    flate.Finish(encoder, sink)?
    check(encoder.InputBytes() == 5 && encoder.OutputBytes() == sink.Len(), "encoder counters")
    let _, after = flate.Write(encoder, sink, Array<byte>{{}}[:])
    check(after != nil, "write after finish")
    let encoded = sink.Bytes()
    var encodedSource = bytes.NewBuffer(encoded[:])
    var decoded = flate.NewDecoder(encoded.len(), 5)?
    let size, eof = flate.Read(decoded, encodedSource, buf[:])
    check(eof == io.EOF && size == 5 && bytes.Equal(buf[:5], raw[5:10]), "raw roundtrip")
    return nil
}}
func main() {{ let err = scenario(); if err != nil {{ panic(errors.Message(err)) }}; println("ok") }}
"#
    );
    let directory = TempDir::new().unwrap();
    let output = execute(&source, directory.path(), false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn staged_tree_ownership_sticky_errors_and_one_worker_cleanup() {
    let source = format!(
        r#"{SUPPORT}
func scenario(root string) error {{
    var tree = os.StageTree(root + "/result", 3)?
    tree.Create("dir/file")?
    let _, err = tree.Write(Array<byte>{{104,105}}[:]); if err != nil {{ return err }}
    tree.FinishFile()?
    tree.Mkdir("dir")?
    tree.Commit()?
    let existing, existsErr = os.StageTree(root + "/result", 3)
    check(existsErr != nil, "existing destination rejected")
    drop(existing)
    {{ var abandoned = os.StageTree(root + "/abandoned", 3)?; abandoned.Create("implicit/file")? }}
    var failed = os.StageTree(root + "/failed", 3)?
    let invalid = failed.Create("../escape")
    check(invalid != nil && failed.Mkdir("valid") == invalid, "sticky invalid path")
    check(failed.Commit() == invalid, "failed commit aborts")
    var unfinished = os.StageTree(root + "/unfinished", 1)?
    unfinished.Create("file")?
    check(unfinished.Commit() != nil, "active file blocks commit and cleans up")
    return nil
}}
async func work(root string) error {{ return scenario(root) }}
func main() {{
    waitIdle(0)
    let task = go work(os.Args()[1])
    let err = task.wait()
    if err != nil {{ panic(errors.Message(err)) }}
    waitIdle(0)
    println("ok")
}}
"#
    );
    let directory = TempDir::new().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let output = execute(&source, &root, true);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(std::fs::read(root.join("result/dir/file")).unwrap(), b"hi");
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn staged_tree_panic_removes_private_staging() {
    let source = format!(
        r#"{SUPPORT}
func scenario() error {{
    var tree = os.StageTree(os.Args()[1] + "/result", 3)?
    tree.Create("dir/file")?
    panic("staging cleanup probe")
}}
func main() {{ let err = scenario(); if err != nil {{ panic(errors.Message(err)) }} }}
"#
    );
    let directory = TempDir::new().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let output = execute(&source, &root, false);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("staging cleanup probe"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("leak:"), "{}", stderr(&output));
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn raw_flate_rejects_broken_io_and_preserves_sticky_errors() {
    let source = format!(
        r#"{SUPPORT}
type Source struct {{ mode int; calls int }}
func (s mut Source) Read(buf mut []byte) (int, error) {{
    s.calls += 1
    if s.mode == 1 {{ return 0, nil }}
    if s.mode == 2 {{ return -1, nil }}
    if s.mode == 3 {{ return buf.len() + 1, nil }}
    return 0, error("raw source failed")
}}
type Sink struct {{ mode int; calls int }}
func (s mut Sink) Write(data []byte) (int, error) {{
    s.calls += 1
    if s.mode == 1 {{ return 0, nil }}
    if s.mode == 2 {{ return -1, nil }}
    if s.mode == 3 {{ return data.len() + 1, nil }}
    return 1, error("raw sink failed")
}}
func scenario() error {{
    for var mode = 1; mode <= 4; mode += 1 {{
        var source = Source{{mode: mode, calls: 0}}
        var decoder = flate.NewDecoder(100, 100)?
        var buf = [byte; 1]{{0}}
        let count, err = flate.Read(decoder, source, buf[:])
        check(err != nil, "broken raw source rejected")
        check(count == 0, "broken source returns no output")
        let calls = source.calls
        let _, repeated = flate.Read(decoder, source, buf[:])
        check(repeated == err && source.calls == calls, "raw read error sticky")
        if mode == 1 {{ check(calls == 100, "raw stalled-read ceiling") }}
        var sink = Sink{{mode: mode, calls: 0}}
        var encoder = flate.NewEncoder(3, 100)?
        let accepted, failed = flate.Write(encoder, sink, Array<byte>{{1,2,3}}[:])
        check(failed != nil, "broken raw sink rejected")
        check(accepted == 3, "raw chunk accepted before sink failure")
        let writes = sink.calls
        check(flate.Finish(encoder, sink) == failed && sink.calls == writes, "raw write error sticky")
        if mode == 4 {{ check(encoder.OutputBytes() == 1, "raw partial compressed count") }}
    }}
    var untouched = Source{{mode: 4, calls: 0}}
    var invalid, invalidErr = flate.NewDecoder(-1, 0)
    check(invalidErr != nil, "invalid raw constructor")
    var empty = Array<byte>{{}}
    let _, emptyErr = flate.Read(invalid, untouched, empty[:])
    check(emptyErr == nil, "empty raw read")
    var probe = [byte; 1]{{0}}
    let _, invalidRead = flate.Read(invalid, untouched, probe[:])
    check(invalidRead == invalidErr && untouched.calls == 0, "inert raw decoder does no IO")
    return nil
}}
func main() {{ let err = scenario(); if err != nil {{ panic(errors.Message(err)) }}; println("ok") }}
"#
    );
    let directory = TempDir::new().unwrap();
    let output = execute(&source, directory.path(), false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
}
