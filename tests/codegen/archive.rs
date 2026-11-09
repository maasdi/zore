use super::*;
use std::time::Duration;

const SUPPORT: &str = r#"
import "std/archive"
import "std/bytes"
import "std/errors"
import "std/io"

func check(ok bool, label string) { if !ok { panic(label) } }

type Sink struct {
    data Array<byte>
    done channel<Array<byte>>
    mode int
    gate channel<bool>
}
func sink(mode int, done channel<Array<byte>>) Sink {
    return Sink{data: Array<byte>{}, done: done, mode: mode, gate: channel<bool>(1)}
}
func (s mut Sink) Write(data []byte) (int, error) {
    if s.mode == 7 { let _, _ = s.gate.receive(); s.mode = 0 }
    if s.mode == 1 { return 0, nil }
    if s.mode == 2 { return data.len() + 1, nil }
    if s.mode == 3 { return -1, nil }
    if s.mode == 4 { return 0, error("sink failed") }
    if data.len() > 0 && ((s.mode == 5 && s.data.len() >= 34) || (s.mode == 6 && s.data.len() >= 512)) {
        s.data.push(data[0])
        return 1, error("partial sink failure")
    }
    var count = data.len()
    if count > 2 { count = 2 }
    for var i = 0; i < count; i += 1 { s.data.push(data[i]) }
    return count, nil
}
func (s mut Sink) drop() { s.done.send(bytes.Clone(s.data[:])) }

type Feed struct {
    data Array<byte>
    position int
    calls int
    mode int
    done channel<int>
}
func feed(data []byte, mode int, done channel<int>) Feed {
    return Feed{data: bytes.Clone(data), position: 0, calls: 0, mode: mode, done: done}
}
func (f mut Feed) Read(buf mut []byte) (int, error) {
    f.calls += 1
    if f.mode == 1 { return 0, nil }
    if f.mode == 2 { return -1, nil }
    if f.mode == 3 { return buf.len() + 1, nil }
    if f.mode == 4 { return 0, error("source failed") }
    if f.position >= f.data.len() { return 0, io.EOF }
    if (f.mode == 10 && f.position == 512) || (f.mode == 11 && f.position == 34 && buf.len() <= 3) {
        buf[0] = f.data[f.position]
        f.position += 1
        return 1, error("partial source failure")
    }
    var count = buf.len()
    if count > 3 { count = 3 }
    if count > f.data.len() - f.position { count = f.data.len() - f.position }
    for var i = 0; i < count; i += 1 { buf[i] = f.data[f.position + i] }
    f.position += count
    if f.position == f.data.len() {
        if f.mode == 5 { return count, io.EOF }
        if f.mode == 6 { return count, error("last byte failed") }
    }
    return count, nil
}
func (f mut Feed) Seek(offset int, whence int) (int, error) {
    if f.mode == 7 { return offset + 1, nil }
    if f.mode == 8 { return 0, error("seek failed") }
    if f.mode == 9 { return -1, nil }
    var position = offset
    if whence == io.SeekEnd { position += f.data.len() }
    if whence == io.SeekCurrent { position += f.position }
    if position < 0 { return 0, error("negative seek") }
    f.position = position
    return position, nil
}
func (f mut Feed) drop() { f.done.send(f.calls) }

type entriesWriter interface {
    mut WriteHeader(header archive.Header) error
    mut Write(data []byte) (int, error)
    mut Finish() error
}
type entriesReader interface {
    mut Next() (archive.Header, bool, error)
    mut Read(buf mut []byte) (int, error)
    mut Skip() error
}
func emit(writer mut entriesWriter, data []byte) error {
    writer.WriteHeader(archive.Header{Name: "file", Size: data.len(), Directory: false})?
    let count, err = writer.Write(data)
    if err != nil { return err }
    check(count == data.len(), "short sink writes retried")
    writer.Finish()?
    return writer.Finish()
}
func pack(zip bool, data []byte) (Array<byte>, error) {
    let done = channel<Array<byte>>(1)
    if zip {
        var writer = archive.NewZIPWriter(sink(0, done), 100000, 5000, 10)?
        emit(writer, data)?
        writer.Close()?
    } else {
        var writer = archive.NewTARWriter(sink(0, done), 100000, 5000, 10)?
        emit(writer, data)?
        writer.Close()?
    }
    let encoded, _ = done.receive()
    return encoded, nil
}
func inspect(reader mut entriesReader, expected []byte) error {
    var empty = Array<byte>{}
    let zero, emptyErr = reader.Read(empty[:])
    check(emptyErr == nil, "empty read")
    check(zero == 0, "empty read count")
    let header, present, err = reader.Next()
    if err != nil { return err }
    check(present && header.Name == "file" && header.Size == expected.len() && !header.Directory, "entry metadata")
    let data = io.ReadAll(reader)?
    check(bytes.Equal(data[:], expected), "entry payload")
    let absent, more, endErr = reader.Next()
    if endErr != nil { return endErr }
    check(!more && absent.Name == "" && absent.Size == 0, "validated end presence")
    let _, again, repeated = reader.Next()
    check(repeated == nil, "repeat end")
    check(!again, "repeat end presence")
    return nil
}
"#;

fn run(body: &str, controlled: bool) -> Output {
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("archive");
    let imports = if body.contains("crc32.") {
        "import \"std/hash/crc32\"\nimport \"std/strings\"\n"
    } else if body.contains("strings.") {
        "import \"std/strings\"\n"
    } else {
        ""
    };
    let source = format!(
        "package main\n{imports}{SUPPORT}\n{body}\nfunc main() {{ let err = scenario(); if err != nil {{ panic(errors.Message(err)) }} }}\n"
    );
    if controlled {
        controlled_runtime::build_with_workers(&source, &executable, 1);
    } else {
        build_file(&source, &executable).unwrap_or_else(|error| panic!("build: {error:?}"));
    }
    bounded_process::run(
        Command::new(executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap()
}

fn succeeds(body: &str, controlled: bool) {
    let output = run(body, controlled);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn crc32_vectors_and_incremental_updates_match_ieee() {
    succeeds(
        r#"func scenario() error {
    let data = strings.Bytes("123456789")
    check(crc32.Checksum(data[:]) == uint32(0xcbf43926), "IEEE check vector")
    check(crc32.Checksum(data[:0]) == 0, "empty CRC")
    for var split = 0; split <= data.len(); split += 1 {
        let prefix = crc32.Update(0, data[:split])
        check(crc32.Update(prefix, data[split:]) == uint32(0xcbf43926), "incremental finalized CRC")
    }
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn archive_roundtrips_short_io_final_eof_and_owned_endpoint_cleanup() {
    succeeds(
        r#"func scenario() error {
    let done = channel<int>(10)
    let payload = Array<byte>{0, 1, 2, 255}
    for zip in Array<bool>{false, true} {
        let encoded = pack(zip, payload[:])?
        let again = pack(zip, payload[:])?
        check(bytes.Equal(encoded[:], again[:]), "deterministic archive bytes")
        if zip {
            var reader = archive.NewZIPReader(feed(encoded[:], 5, done), encoded.len(), payload.len(), 1)?
            inspect(reader, payload[:])?
            reader.Close()?
        } else {
            var reader = archive.NewTARReader(feed(encoded[:], 5, done), encoded.len(), payload.len(), 1)?
            inspect(reader, payload[:])?
            reader.Close()?
        }
        let calls, _ = done.receive()
        check(calls > 0, "source dropped")
    }
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn archive_readers_require_explicit_consumption_and_skip_validates_entries() {
    succeeds(
        r#"func scenario() error {
    let done = channel<int>(20)
    let payload = Array<byte>{1, 2, 3}
    for zip in Array<bool>{false, true} {
        let encoded = pack(zip, payload[:])?
        if zip {
            var reader = archive.NewZIPReader(feed(encoded[:], 0, done), encoded.len(), 3, 1)?
            let _, _, first = reader.Next()
            if first != nil { return first }
            let _, present, err = reader.Next()
            check(err == error("archive: unread ZIP entry data"), "unread ZIP data")
            check(!present, "absence on failure")
            check(reader.Close() == err, "close reports terminal ZIP error")
            var skipped = archive.NewZIPReader(feed(encoded[:], 0, done), encoded.len(), 3, 1)?
            let _, _, skipHeader = skipped.Next()
            if skipHeader != nil { return skipHeader }
            skipped.Skip()?
            let _, more, endErr = skipped.Next()
            check(endErr == nil, "skip ZIP validation")
            check(!more, "skip ZIP end")
            skipped.Close()?
        } else {
            var reader = archive.NewTARReader(feed(encoded[:], 0, done), encoded.len(), 3, 1)?
            let _, _, first = reader.Next()
            if first != nil { return first }
            let _, present, err = reader.Next()
            check(err == error("archive: unread TAR entry data"), "unread TAR data")
            check(!present, "absence on failure")
            check(reader.Close() == err, "close reports terminal TAR error")
            var skipped = archive.NewTARReader(feed(encoded[:], 0, done), encoded.len(), 3, 1)?
            let _, _, skipHeader = skipped.Next()
            if skipHeader != nil { return skipHeader }
            skipped.Skip()?
            let _, more, endErr = skipped.Next()
            check(endErr == nil, "skip TAR validation")
            check(!more, "skip TAR end")
            skipped.Close()?
        }
        let _, _ = done.receive()
        let _, _ = done.receive()
    }
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn archive_sources_reject_stalls_bad_counts_and_broken_seeks_stickily() {
    succeeds(
        r#"func scenario() error {
    let done = channel<int>(40)
    for zip in Array<bool>{false, true} {
        let encoded = pack(zip, Array<byte>{1}[:])?
        for mode in Array<int>{1, 2, 3, 4, 6} {
            if zip {
                var reader = archive.NewZIPReader(feed(encoded[:], mode, done), encoded.len(), 1, 1)?
                let _, present, err = reader.Next()
                check(err != nil, "ZIP source failure")
                check(!present, "ZIP failure absence")
                let _, _, again = reader.Next()
                check(again == err, "sticky ZIP source failure")
                check(reader.Close() == err, "ZIP failure close")
            } else {
                var reader = archive.NewTARReader(feed(encoded[:], mode, done), encoded.len(), 1, 1)?
                let _, present, err = reader.Next()
                if mode == 6 {
                    if err != nil { return err }
                    check(present, "TAR first entry precedes terminal source error")
                    reader.Skip()?
                    let _, _, tailErr = reader.Next()
                    check(tailErr == error("last byte failed"), "TAR tail error")
                    check(reader.Close() == tailErr, "TAR terminal tail close")
                } else {
                    check(err != nil, "TAR source failure")
                    check(!present, "TAR failure absence")
                    let _, _, again = reader.Next()
                    check(again == err, "sticky TAR source failure")
                    check(reader.Close() == err, "TAR failure close")
                }
            }
            let calls, _ = done.receive()
            if mode == 1 { check(calls == 100, "bounded empty reads") }
        }
        if zip {
            for mode in Array<int>{7, 8, 9} {
                var reader = archive.NewZIPReader(feed(encoded[:], mode, done), encoded.len(), 1, 1)?
                let _, _, err = reader.Next()
                check(err != nil, "invalid ZIP seeker")
                check(reader.Close() == err, "seeker cleanup")
                let calls, _ = done.receive()
                check(calls == 0, "reject seek before read")
            }
        }
    }
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn archive_writer_failures_are_sticky_and_drop_does_not_finalize() {
    succeeds(
        r#"func exercise(writer mut entriesWriter) error {
    writer.WriteHeader(archive.Header{Name: "file", Size: 2, Directory: false})?
    let count, err = writer.Write(Array<byte>{1, 2, 3}[:])
    check(err == error("archive: entry size exceeded"), "size preflight")
    check(count == 0, "oversized write accepts nothing")
    check(writer.Finish() == err && writer.Finish() == err, "sticky writer failure")
    return nil
}
func scenario() error {
    let done = channel<Array<byte>>(30)
    for zip in Array<bool>{false, true} {
        if zip {
            var writer = archive.NewZIPWriter(sink(0, done), 10000, 10, 2)?
            exercise(writer)?
            check(writer.Close() == error("archive: entry size exceeded"), "ZIP error close")
            let dropped = archive.NewZIPWriter(sink(0, done), 22, 0, 0)?
            drop(dropped)
        } else {
            var writer = archive.NewTARWriter(sink(0, done), 10000, 10, 2)?
            exercise(writer)?
            check(writer.Close() == error("archive: entry size exceeded"), "TAR error close")
            let dropped = archive.NewTARWriter(sink(0, done), 1024, 0, 0)?
            drop(dropped)
        }
        let partial, _ = done.receive()
        check(partial.len() > 0, "partial header retained")
        let empty, _ = done.receive()
        check(empty.len() == 0, "drop never writes")
        for mode in Array<int>{1, 2, 3, 4} {
            if zip {
                var writer = archive.NewZIPWriter(sink(mode, done), 10000, 10, 2)?
                let err = writer.WriteHeader(archive.Header{Name: "empty", Size: 0, Directory: false})
                check(err != nil, "ZIP bad sink")
                check(writer.Close() == err, "ZIP sticky sink error")
            } else {
                var writer = archive.NewTARWriter(sink(mode, done), 10000, 10, 2)?
                let err = writer.WriteHeader(archive.Header{Name: "empty", Size: 0, Directory: false})
                check(err != nil, "TAR bad sink")
                check(writer.Close() == err, "TAR sticky sink error")
            }
            let _, _ = done.receive()
        }
    }
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn archive_constructors_and_early_close_do_no_io_and_release_owned_endpoints() {
    succeeds(
        r#"func scenario() error {
    let sourceDone = channel<int>(10)
    let sinkDone = channel<Array<byte>>(10)
    let data = Array<byte>{1, 2, 3}
    let tar = archive.NewTARReader(feed(data[:], 0, sourceDone), 100, 100, 10)?
    tar.Close()?
    let zip = archive.NewZIPReader(feed(data[:], 0, sourceDone), 100, 100, 10)?
    zip.Close()?
    for var i = 0; i < 2; i += 1 { let calls, _ = sourceDone.receive(); check(calls == 0, "close does not drain") }
    for limit in Array<int>{-1, archive.MaxBytes + 1} {
        let badTAR, tarErr = archive.NewTARReader(feed(data[:], 0, sourceDone), limit, 100, 10)
        check(tarErr == error("archive: invalid limit"), "bad TAR constructor")
        drop(badTAR)
        let badZIP, zipErr = archive.NewZIPWriter(sink(0, sinkDone), 100, limit, 10)
        check(zipErr == error("archive: invalid limit"), "bad ZIP constructor")
        drop(badZIP)
        let calls, _ = sourceDone.receive()
        let encoded, _ = sinkDone.receive()
        check(calls == 0 && encoded.len() == 0, "invalid constructor no IO")
    }
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn archive_io_composes_on_one_worker_with_a_blocked_owned_sink() {
    succeeds(
        r#"async func worker(zip bool, gate channel<bool>, result channel<bool>, done channel<Array<byte>>) error {
    let output = Sink{data: Array<byte>{}, done: done, mode: 7, gate: gate}
    let payload = Array<byte>{1, 2, 3}
    if zip {
        var writer = archive.NewZIPWriter(output, 10000, 10, 1)?
        emit(writer, payload[:])?
        writer.Close()?
    } else {
        var writer = archive.NewTARWriter(output, 10000, 10, 1)?
        emit(writer, payload[:])?
        writer.Close()?
    }
    let encoded, _ = done.receive()
    let sourceDone = channel<int>(1)
    if zip {
        var reader = archive.NewZIPReader(feed(encoded[:], 5, sourceDone), encoded.len(), 3, 1)?
        inspect(reader, payload[:])?
        reader.Close()?
    } else {
        var reader = archive.NewTARReader(feed(encoded[:], 5, sourceDone), encoded.len(), 3, 1)?
        inspect(reader, payload[:])?
        reader.Close()?
    }
    let _, _ = sourceDone.receive()
    result.send(true)
    return nil
}
func scenario() error {
    for zip in Array<bool>{false, true} {
        let gate = channel<bool>(1)
        let result = channel<bool>(1)
        let done = channel<Array<byte>>(1)
        go worker(zip, gate, result, done)
        waitBlocked(1)
        gate.send(true)
        let ok, _ = result.receive()
        check(ok, "one-worker archive progress")
        waitIdle(0)
    }
    println("ok")
    return nil
}
"#,
        true,
    );
}

#[test]
fn archive_panic_drops_readers_and_unfinished_writers_without_leaks() {
    let output = run(
        r#"func scenario() error {
    let sourceDone = channel<int>(10)
    let sinkDone = channel<Array<byte>>(10)
    let tarBytes = pack(false, Array<byte>{1}[:])?
    let zipBytes = pack(true, Array<byte>{1}[:])?
    let tar = archive.NewTARReader(feed(tarBytes[:], 0, sourceDone), tarBytes.len(), 10, 1)?
    let zip = archive.NewZIPReader(feed(zipBytes[:], 0, sourceDone), zipBytes.len(), 10, 1)?
    let tarWriter = archive.NewTARWriter(sink(0, sinkDone), 10000, 10, 1)?
    let zipWriter = archive.NewZIPWriter(sink(0, sinkDone), 10000, 10, 1)?
    panic("archive cleanup probe")
}
"#,
        false,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("archive cleanup probe"));
    assert!(!stderr(&output).contains("leak:"), "{}", stderr(&output));
}

#[test]
fn archive_partial_source_and_sink_errors_preserve_counts_and_output_prefixes() {
    succeeds(
        r#"func scenario() error {
    let sourceDone = channel<int>(10)
    let sinkDone = channel<Array<byte>>(10)
    let payload = Array<byte>{1, 2, 3}
    for zip in Array<bool>{false, true} {
        let encoded = pack(zip, payload[:])?
        if zip {
            var reader = archive.NewZIPReader(feed(encoded[:], 11, sourceDone), encoded.len(), 3, 1)?
            let _, _, startErr = reader.Next()
            if startErr != nil { return startErr }
            let partial, err = io.ReadAll(reader)
            check(err == error("partial source failure"), "ZIP partial source error")
            check(partial.len() == 1 && partial[0] == 1, "ZIP partial decoded bytes")
            check(reader.Close() == err, "ZIP partial source cleanup")
            var writer = archive.NewZIPWriter(sink(5, sinkDone), 10000, 3, 1)?
            writer.WriteHeader(archive.Header{Name: "file", Size: 3, Directory: false})?
            let count, writeErr = writer.Write(payload[:])
            check(writeErr == error("partial sink failure"), "ZIP partial sink error")
            check(count == 1, "ZIP accepted byte count")
            check(writer.Close() == writeErr, "ZIP partial sink cleanup")
        } else {
            var reader = archive.NewTARReader(feed(encoded[:], 10, sourceDone), encoded.len(), 3, 1)?
            let _, _, startErr = reader.Next()
            if startErr != nil { return startErr }
            let partial, err = io.ReadAll(reader)
            check(err == error("partial source failure"), "TAR partial source error")
            check(partial.len() == 1 && partial[0] == 1, "TAR partial decoded bytes")
            check(reader.Close() == err, "TAR partial source cleanup")
            var writer = archive.NewTARWriter(sink(6, sinkDone), 10000, 3, 1)?
            writer.WriteHeader(archive.Header{Name: "file", Size: 3, Directory: false})?
            let count, writeErr = writer.Write(payload[:])
            check(writeErr == error("partial sink failure"), "TAR partial sink error")
            check(count == 1, "TAR accepted byte count")
            check(writer.Close() == writeErr, "TAR partial sink cleanup")
        }
        let _, _ = sourceDone.receive()
        let prefix, _ = sinkDone.receive()
        if zip { check(prefix.len() == 35, "ZIP external prefix") } else { check(prefix.len() == 513, "TAR external prefix") }
    }
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn archive_sink_recovery_keeps_the_concrete_type_and_partial_failure_ownership() {
    succeeds(
        r#"func scenario() error {
    let done = channel<Array<byte>>(10)
    for zip in Array<bool>{false, true} {
        if zip {
            var writer = archive.NewZIPWriter(sink(0, done), 10000, 3, 1)?
            writer.WriteHeader(archive.Header{Name: "file", Size: 3, Directory: false})?
            let count, err = writer.Write(Array<byte>{1}[:])
            check(err == nil, "ZIP prefix")
            check(count == 1, "ZIP prefix count")
            let recovered, finishErr = writer.Sink()
            check(finishErr == error("archive: incomplete ZIP entry"), "ZIP finalization failure")
            check(recovered.data.len() == 35, "concrete ZIP sink retained")
            drop(recovered)
        } else {
            var writer = archive.NewTARWriter(sink(0, done), 10000, 3, 1)?
            writer.WriteHeader(archive.Header{Name: "file", Size: 3, Directory: false})?
            let count, err = writer.Write(Array<byte>{1}[:])
            check(err == nil, "TAR prefix")
            check(count == 1, "TAR prefix count")
            let recovered, finishErr = writer.Sink()
            check(finishErr == error("archive: incomplete TAR entry"), "TAR finalization failure")
            check(recovered.data.len() == 513, "concrete TAR sink retained")
            drop(recovered)
        }
        let prefix, _ = done.receive()
        check(prefix[prefix.len() - 1] == 1, "recovered sink dropped exactly once")
    }
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn archive_guide_example_finalizes_tar_then_gzip() {
    let guide = include_str!("../../docs/archives.md");
    let source = guide
        .split("```ore\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("archive-guide");
    build_file(source, &executable).unwrap();
    let output = bounded_process::run(
        Command::new(executable)
            .arg(directory.path().join("hello.tar.gz"))
            .env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "hello.txt\n14\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn long_tar_names_use_pax_and_dot_prefixes_are_removed() {
    succeeds(
        r#"func scenario() error {
    let done = channel<Array<byte>>(2)
    let long = "deep/" + strings.Repeat("segment-é/", 40) + "file.txt"
    let folder = strings.Repeat("d", 300) + "/"
    var writer = archive.NewTARWriter(sink(0, done), 1048576, 100, 3)?
    writer.WriteHeader(archive.Header{Name: long, Size: 3, Directory: false})?
    let count, err = writer.Write(Array<byte>{97, 98, 99}[:])
    if err != nil { return err }
    check(count == 3, "payload")
    writer.WriteHeader(archive.Header{Name: folder, Size: 0, Directory: true})?
    writer.Close()?
    let encoded, _ = done.receive()
    check(encoded.len() == 512 * 9, "two PAX headers, two entries, padding and end")
    check(encoded[156] == 120 && encoded[1024 + 512 + 512 + 156] == 120, "PAX headers")
    var reader = archive.NewTARReader(bytes.NewBuffer(encoded[:]), 1048576, 100, 2)?
    let first, present, firstErr = reader.Next()
    if firstErr != nil { return firstErr }
    check(present && first.Name == long && first.Size == 3, "long file name")
    let data = io.ReadAll(reader)?
    check(data.len() == 3 && data[2] == 99, "long file data")
    let second, again, secondErr = reader.Next()
    if secondErr != nil { return secondErr }
    check(again && second.Name == folder && second.Directory, "long directory name")
    let _, ended, endErr = reader.Next()
    if endErr != nil { return endErr }
    check(!ended, "end")
    reader.Close()?
    var short = archive.NewTARReader(bytes.NewBuffer(encoded[:1024]), 1048576, 100, 2)?
    let _, _, cut = short.Next()
    check(cut == error("archive: TAR extended header without entry") || cut == error("archive: truncated TAR stream"), "extended header needs an entry")
    check(short.Close() == cut, "sticky")
    check(!archive.ValidName("./a", false), "writers still reject dot prefixes")
    println("ok")
    return nil
}
"#,
        false,
    );
}

#[test]
fn entries_report_kinds_and_writers_reject_links_and_executable_directories() {
    succeeds(
        r#"func scenario() error {
    let done = channel<Array<byte>>(8)
    let tool = archive.Entry{Header: archive.Header{Name: "bin/tool", Size: 2, Directory: false}, Kind: archive.FileEntry, Link: "", Executable: true}
    var writer = archive.NewTARWriter(sink(0, done), 1048576, 100, 3)?
    writer.WriteEntry(tool)?
    let count, err = writer.Write(Array<byte>{35, 33}[:])
    if err != nil { return err }
    check(count == 2, "payload")
    writer.WriteHeader(archive.Header{Name: "bin/", Size: 0, Directory: true})?
    writer.Close()?
    let encoded, _ = done.receive()
    check(encoded[100] == 48 && encoded[104] == 55 && encoded[105] == 53 && encoded[106] == 53, "mode 0755")
    var reader = archive.NewTARReader(bytes.NewBuffer(encoded[:]), 1048576, 100, 3)?
    let first, present, firstErr = reader.NextEntry()
    if firstErr != nil { return firstErr }
    check(present && first.Kind == archive.FileEntry && first.Executable && first.Header.Name == "bin/tool", "executable file")
    reader.Skip()?
    let second, again, secondErr = reader.NextEntry()
    if secondErr != nil { return secondErr }
    check(again && second.Kind == archive.DirectoryEntry && !second.Executable && second.Link == "", "directory")
    reader.Close()?
    for kind in Array<int>{2, 3} {
        var links = archive.NewZIPWriter(sink(0, done), 1048576, 100, 3)?
        let entry = archive.Entry{Header: archive.Header{Name: "l", Size: 0, Directory: false}, Kind: archive.EntryKind(kind), Link: "target", Executable: false}
        let failed = links.WriteEntry(entry)
        check(failed == error("archive: links cannot be written"), "links rejected")
        check(links.Close() == failed, "sticky")
        let _, _ = done.receive()
    }
    var mismatch = archive.NewTARWriter(sink(0, done), 1048576, 100, 3)?
    let wrong = mismatch.WriteEntry(archive.Entry{Header: archive.Header{Name: "d/", Size: 0, Directory: true}, Kind: archive.FileEntry, Link: "", Executable: false})
    check(wrong == error("archive: entry kind does not match header"), "kind mismatch")
    let _ = mismatch.Close()
    let _, _ = done.receive()
    var folder = archive.NewZIPWriter(sink(0, done), 1048576, 100, 3)?
    let runnable = folder.WriteEntryWith(archive.Entry{Header: archive.Header{Name: "d/", Size: 0, Directory: true}, Kind: archive.DirectoryEntry, Link: "", Executable: true}, archive.Deflate)
    check(runnable == error("archive: directories cannot be executable"), "executable directory")
    let _ = folder.Close()
    let _, _ = done.receive()
    println("ok")
    return nil
}
"#,
        false,
    );
}
