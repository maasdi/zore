use super::*;

#[test]
fn bytes_buffer_reuse_bounds_allocations_for_every_write_path() {
    let source = r#"package main
import "std/bytes"

func writeChunk(buffer mut bytes.Buffer, mode int) {
    if mode == 0 {
        let n, err = buffer.Write(Array<byte>{1, 0, 255}[:])
        if err != nil || n != 3 { panic("Write count") }
    } else if mode == 1 {
        if buffer.WriteByte(1) != nil { panic("WriteByte") }
        if buffer.WriteByte(0) != nil { panic("WriteByte") }
        if buffer.WriteByte(255) != nil { panic("WriteByte") }
    } else if mode == 2 {
        let n, err = buffer.WriteString("hé")
        if err != nil || n != 3 { panic("WriteString count") }
    } else {
        let n, err = buffer.WriteRune('界')
        if err != nil || n != 3 { panic("WriteRune count") }
    }
}

func cycle(buffer mut bytes.Buffer, mode int, backlog bool) {
    writeChunk(buffer, mode)
    var data = [byte; 3]{0, 0, 0}
    let n, err = buffer.Read(data[:])
    if err != nil || n != 3 { panic("Read count") }
    var expected = [byte; 3]{1, 0, 255}
    if mode == 2 { expected = [byte; 3]{104, 195, 169} }
    if mode == 3 { expected = [byte; 3]{231, 149, 140} }
    if !bytes.Equal(data[:], expected[:]) { panic("byte order") }
    var remaining = 0
    if backlog { remaining = 3 }
    if buffer.Len() != remaining { panic("unread length") }
}

func checkGrowth(mode int, backlog bool) {
    var buffer = bytes.NewBufferString("")
    if backlog { writeChunk(buffer, mode) }
    for var i = 0; i < 64; i += 1 { cycle(buffer, mode, backlog) }
    let warmed = allocatedBytes()
    for var i = 0; i < 4096; i += 1 { cycle(buffer, mode, backlog) }
    if allocatedBytes() > warmed + 1024 { panic("buffer retained consumed history") }
}

func main() {
    waitIdle(0)
    let baseline = allocatedBytes()
    for var mode = 0; mode < 4; mode += 1 {
        checkGrowth(mode, false)
        checkGrowth(mode, true)
        if allocatedBytes() != baseline { panic("buffer storage survived drop") }
    }
    println("ok")
}
"#;
    let output = run_controlled_one_worker(source);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn bytes_buffer_compaction_preserves_queue_and_snapshot_contracts() {
    let source = r#"package main
import "std/bytes"
import "std/io"

func check(ok bool) {
    if !ok { panic("buffer contract") }
}

func main() {
    var input = Array<byte>{65, 66, 67, 68}
    var buffer = bytes.NewBuffer(input[:])
    input[0] = 88
    let snapshot = buffer.Bytes()
    var pair = [byte; 2]{0, 0}
    (var count, var err) = buffer.Read(pair[:0])
    check(err == nil && count == 0 && buffer.Len() == 4)
    let first, firstErr = buffer.ReadByte()
    check(firstErr == nil && first == 65)
    count, err = buffer.WriteString("EF")
    check(err == nil && count == 2)
    count, err = buffer.Read(pair[:])
    check(err == nil && count == 2 && pair[0] == 66 && pair[1] == 67)
    count, err = buffer.Write(Array<byte>{71, 72}[:])
    check(err == nil && count == 2)
    let next, nextErr = buffer.ReadByte()
    check(nextErr == nil && next == 68)
    check(buffer.WriteByte(73) == nil)
    count, err = buffer.Read(pair[:])
    check(err == nil && count == 2 && pair[0] == 69 && pair[1] == 70)
    count, err = buffer.WriteRune('界')
    check(err == nil && count == 3)
    let remaining = buffer.Bytes()
    check(bytes.Equal(remaining[:], Array<byte>{71, 72, 73, 231, 149, 140}[:]))
    check(bytes.Equal(snapshot[:], Array<byte>{65, 66, 67, 68}[:]))

    buffer.Truncate(3)
    count, err = buffer.WriteString("JKL")
    check(err == nil && count == 3)
    let truncated = buffer.Bytes()
    check(bytes.Equal(truncated[:], Array<byte>{71, 72, 73, 74, 75, 76}[:]))
    buffer.Truncate(0)
    count, err = buffer.Read(pair[:0])
    check(err == nil && count == 0)
    let empty, eof = buffer.ReadByte()
    check(eof == io.EOF && empty == 0)
    check(buffer.WriteByte(0) == nil)
    count, err = buffer.Read(pair[:])
    check(err == nil && count == 1 && pair[0] == 0)
    count, err = buffer.Read(pair[:])
    check(err == io.EOF && count == 0)

    count, err = buffer.WriteString("reset")
    check(err == nil && count == 5)
    let _, _ = buffer.ReadByte()
    buffer.Reset()
    check(buffer.Len() == 0)
    check(buffer.WriteByte(255) == nil)
    let reused, reusedErr = buffer.ReadByte()
    check(reusedErr == nil && reused == 255)
    let _, finalErr = buffer.ReadByte()
    check(finalErr == io.EOF)
    let copied = buffer.Bytes()
    check(copied.len() == 0)
    println("ok")
}
"#;
    let output = run_poll_bounded(source, None);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}
