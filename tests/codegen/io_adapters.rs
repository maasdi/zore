use super::*;

const SOURCES: &str = r#"
func check(ok bool, label string) {
    if !ok { panic(label) }
}

func fails(err error, message string) bool {
    return err != nil && errors.Message(err) == message
}

type Feed struct {
    data     Array<byte>
    pos      int
    chunk    int
    endErr   error
    withLast bool
    calls    int
    asked    int
}

func feed(size int, chunk int) Feed {
    var data = Array<byte>{}
    for var i = 0; i < size; i += 1 { data.push(byte((i * 7 + 3) % 256)) }
    return Feed{data: data, pos: 0, chunk: chunk, endErr: io.EOF, withLast: false, calls: 0, asked: 0}
}

func (f mut Feed) Read(buf mut []byte) (int, error) {
    f.calls += 1
    f.asked += buf.len()
    if buf.len() == 0 { return 0, nil }
    if f.pos >= f.data.len() { return 0, f.endErr }
    var n = buf.len()
    if n > f.chunk { n = f.chunk }
    if n > f.data.len() - f.pos { n = f.data.len() - f.pos }
    for var i = 0; i < n; i += 1 { buf[i] = f.data[f.pos + i] }
    f.pos += n
    if f.withLast && f.pos == f.data.len() { return n, f.endErr }
    return n, nil
}

func expected(size int) Array<byte> {
    var data = Array<byte>{}
    for var i = 0; i < size; i += 1 { data.push(byte((i * 7 + 3) % 256)) }
    return data
}

func same(left []byte, right []byte) bool {
    if left.len() != right.len() { return false }
    for var i = 0; i < left.len(); i += 1 {
        if left[i] != right[i] { return false }
    }
    return true
}

type Stall struct {
    empties int
    calls   int
}

func (s mut Stall) Read(buf mut []byte) (int, error) {
    s.calls += 1
    if s.empties > 0 {
        s.empties -= 1
        return 0, nil
    }
    buf[0] = 42
    return 1, io.EOF
}

type Bad struct {
    count int
}

func (b mut Bad) Read(buf mut []byte) (int, error) {
    if b.count < 0 { return b.count, nil }
    return buf.len() + b.count, nil
}

type Sink struct {
    data  Array<byte>
    most  int
    fail  int
    zero  bool
    calls int
}

func sink(most int) Sink {
    return Sink{data: Array<byte>{}, most: most, fail: -1, zero: false, calls: 0}
}

func (s mut Sink) Write(p []byte) (int, error) {
    s.calls += 1
    if s.zero { return 0, nil }
    if s.fail >= 0 && s.data.len() >= s.fail { return 0, error("sink failed") }
    var n = p.len()
    if n > s.most { n = s.most }
    for var i = 0; i < n; i += 1 { s.data.push(p[i]) }
    return n, nil
}
"#;

fn program(imports: &str, body: &str) -> String {
    format!(
        "package main\nimport \"std/errors\"\nimport \"std/io\"\n{imports}\n{SOURCES}\n{body}\n"
    )
}

#[test]
fn read_at_most_separates_fitting_input_from_input_past_the_limit() {
    let source = program(
        "",
        r#"
func main() {
    for chunk in Array<int>{1, 7, 300, 40000}[:] {
        var fits = feed(300, chunk)
        let data, err = io.ReadAtMost(fits, 300)
        check(err == nil && same(data[:], expected(300)[:]), "exactly the limit")
        check(fits.pos == 300, "reads nothing past the input")

        var over = feed(301, chunk)
        let cut, overErr = io.ReadAtMost(over, 300)
        check(overErr == io.LimitExceeded && same(cut[:], expected(300)[:]), "one byte over")
        check(over.pos == 301, "consumes exactly one extra byte")

        var longer = feed(5000, chunk)
        let first, longErr = io.ReadAtMost(longer, 300)
        check(longErr == io.LimitExceeded && first.len() == 300 && longer.pos == 301, "far over")

        var short = feed(299, chunk)
        let all, shortErr = io.ReadAtMost(short, 300)
        check(shortErr == nil && same(all[:], expected(299)[:]), "under the limit")
    }

    var withEnd = feed(10, 4)
    withEnd.withLast = true
    let ended, endedErr = io.ReadAtMost(withEnd, 10)
    check(endedErr == nil && ended.len() == 10, "EOF with the last bytes")

    var empty = feed(0, 4)
    let none, emptyErr = io.ReadAtMost(empty, 0)
    check(emptyErr == nil && none.len() == 0, "limit zero, empty input")
    var one = feed(1, 4)
    let nothing, oneErr = io.ReadAtMost(one, 0)
    check(oneErr == io.LimitExceeded && nothing.len() == 0 && one.pos == 1, "limit zero, one byte")

    var untouched = feed(5, 4)
    let _, negative = io.ReadAtMost(untouched, -1)
    check(fails(negative, "io.ReadAtMost: negative limit") && untouched.calls == 0, "negative limit")

    var broken = feed(6, 4)
    broken.endErr = error("disk gone")
    let partial, brokenErr = io.ReadAtMost(broken, 100)
    check(fails(brokenErr, "disk gone") && partial.len() == 6, "error forwarded with the bytes")

    var big = feed(70000, 70000)
    let huge, hugeErr = io.ReadAtMost(big, 9223372036854775807)
    check(hugeErr == nil && huge.len() == 70000, "largest limit")

    var bad = Bad{count: 1}
    let _, badErr = io.ReadAtMost(bad, 10)
    check(fails(badErr, "io.ReadAtMost: invalid read count"), "count past the buffer")
    var negativeCount = Bad{count: -1}
    let _, negativeErr = io.ReadAtMost(negativeCount, 10)
    check(fails(negativeErr, "io.ReadAtMost: invalid read count"), "negative count")

    var stuck = Stall{empties: 1000, calls: 0}
    let _, stuckErr = io.ReadAtMost(stuck, 10)
    check(fails(stuckErr, "io.ReadAtMost: no read progress") && stuck.calls == 100, "no progress")
    var slow = Stall{empties: 99, calls: 0}
    let late, slowErr = io.ReadAtMost(slow, 10)
    check(slowErr == nil && late.len() == 1 && late[0] == 42, "99 empty reads are tolerated")
    println("ok")
}
"#,
    );
    prints(&source, "ok\n");
}

#[test]
fn copy_n_copies_exactly_n_bytes_through_short_and_failing_endpoints() {
    let source = program(
        "",
        r#"
func main() {
    for chunk in Array<int>{1, 5, 40000}[:] {
        for most in Array<int>{1, 3, 40000}[:] {
            var src = feed(1000, chunk)
            var dst = sink(most)
            let n, err = io.CopyN(dst, src, 600)
            check(err == nil && n == 600 && same(dst.data[:], expected(600)[:]), "copied")
            check(src.pos == 600, "never reads past n")
        }
    }

    var big = feed(100000, 100000)
    var bigSink = sink(100000)
    let bigN, bigErr = io.CopyN(bigSink, big, 70000)
    check(bigErr == nil && bigN == 70000 && big.pos == 70000, "more than one buffer")

    var idle = feed(10, 4)
    var idleSink = sink(10)
    let zero, zeroErr = io.CopyN(idleSink, idle, 0)
    check(zeroErr == nil && zero == 0 && idle.calls == 0, "zero count")
    let _, negative = io.CopyN(idleSink, idle, -1)
    check(fails(negative, "io.CopyN: negative count") && idle.calls == 0, "negative count")

    var empty = feed(0, 4)
    let none, emptyErr = io.CopyN(idleSink, empty, 5)
    check(emptyErr == io.EOF && none == 0, "EOF before any byte")
    var short = feed(3, 4)
    var shortSink = sink(10)
    let three, shortErr = io.CopyN(shortSink, short, 5)
    check(fails(shortErr, "unexpected EOF") && three == 3 && shortSink.data.len() == 3, "ends part way")

    var last = feed(5, 5)
    last.withLast = true
    last.endErr = error("closing")
    let five, lastErr = io.CopyN(idleSink, last, 5)
    check(lastErr == nil && five == 5, "last bytes with an error")

    var failing = feed(10, 10)
    failing.endErr = error("disk gone")
    var failSink = sink(100)
    let got, failErr = io.CopyN(failSink, failing, 20)
    check(fails(failErr, "disk gone") && got == 10, "read error forwarded")

    var toBroken = feed(10, 10)
    var broken = sink(100)
    broken.fail = 4
    broken.most = 4
    let wrote, writeErr = io.CopyN(broken, toBroken, 10)
    check(fails(writeErr, "sink failed") && wrote == 4, "write error with the count written")

    var toStuck = feed(10, 10)
    var stuck = sink(10)
    stuck.zero = true
    let stuckN, stuckErr = io.CopyN(stuck, toStuck, 10)
    check(fails(stuckErr, "io.CopyN: no write progress") && stuckN == 0, "no write progress")

    var stall = Stall{empties: 1000, calls: 0}
    let _, stallErr = io.CopyN(idleSink, stall, 4)
    check(fails(stallErr, "io.CopyN: no read progress") && stall.calls == 100, "no read progress")
    var bad = Bad{count: 1}
    let _, badErr = io.CopyN(idleSink, bad, 4)
    check(fails(badErr, "io.CopyN: invalid read count"), "invalid read count")
    println("ok")
}
"#,
    );
    prints(&source, "ok\n");
}

#[test]
fn limited_readers_stop_at_their_limit_and_give_their_source_back() {
    let source = program(
        "import \"std/bufio\"",
        r#"
type Loud struct {
    inner Feed
}

func (l mut Loud) Read(buf mut []byte) (int, error) {
    return l.inner.Read(buf)
}

func (l mut Loud) drop() {
    println("source dropped")
}

func main() {
    for chunk in Array<int>{1, 3, 100}[:] {
        var limited = io.LimitReader(feed(50, chunk), 20)
        let head, err = io.ReadAll(limited)
        check(err == nil && same(head[:], expected(20)[:]) && limited.Remaining() == 0, "first 20")
        var buf = Array<byte>{0, 0, 0}
        let after, afterErr = limited.Read(buf[:])
        check(afterErr == io.EOF && after == 0, "EOF after the limit")
        var source = limited.Source()
        let rest, restErr = io.ReadAll(source)
        check(restErr == nil && same(rest[:], expected(50)[20:]), "the source continues after the limit")
    }

    var short = io.LimitReader(feed(5, 10), 20)
    let five, shortErr = io.ReadAll(short)
    check(shortErr == nil && five.len() == 5 && short.Remaining() == 15, "source ends first")

    var asking = io.LimitReader(feed(100, 100), 7)
    var wide = Array<byte>{}
    for var i = 0; i < 64; i += 1 { wide.push(0) }
    let seven, _ = asking.Read(wide[:])
    check(seven == 7 && asking.Remaining() == 0, "asks no more than remaining")
    var inner = asking.Source()
    let next, _ = io.ReadAll(inner)
    check(next.len() == 93, "nothing past the limit was taken")

    var emptyBuf = io.LimitReader(feed(10, 10), 5)
    var nothing = Array<byte>{}
    let zero, zeroErr = emptyBuf.Read(nothing[:])
    check(zeroErr == nil && zero == 0 && emptyBuf.Remaining() == 5, "empty buffer")

    var negative = io.LimitReader(feed(10, 10), -3)
    let none, noneErr = io.ReadAll(negative)
    check(noneErr == nil && none.len() == 0 && negative.Remaining() == 0, "negative limit is zero")

    var bad = io.LimitReader(Bad{count: 1}, 10)
    var small = Array<byte>{0, 0}
    let badN, badErr = bad.Read(small[:])
    check(fails(badErr, "io.LimitedReader: invalid read count") && badN == 0 && bad.Remaining() == 10, "invalid count")

    var withEnd = feed(4, 4)
    withEnd.withLast = true
    withEnd.endErr = error("late")
    var passing = io.LimitReader(withEnd, 10)
    var four = Array<byte>{0, 0, 0, 0, 0}
    let got, gotErr = passing.Read(four[:])
    check(fails(gotErr, "late") && got == 4 && passing.Remaining() == 6, "errors pass through with bytes")

    var lines = bufio.NewReader(io.LimitReader(feed(0, 1), 0))
    let _, linesErr = lines.ReadByte()
    check(linesErr == io.EOF, "wrapped by bufio")

    var text = Feed{data: Array<byte>{104, 105, 10, 116, 104, 101, 114, 101, 10}, pos: 0, chunk: 2, endErr: io.EOF, withLast: false, calls: 0, asked: 0}
    var scanner = bufio.NewScanner(io.LimitReader(text, 6))
    var seen = 0
    for scanner.Scan() { seen += 1 }
    check(scanner.Err() == nil && seen == 2, "scanner over a limited reader")

    var loud = io.LimitReader(Loud{inner: feed(3, 3)}, 2)
    let _, _ = io.ReadAll(loud)
    println("before drop")
    drop(loud)
    println("ok")
}
"#,
    );
    prints(&source, "before drop\nsource dropped\nok\n");
}

#[test]
fn existing_helpers_stop_on_sources_that_never_make_progress() {
    let source = program(
        "",
        r#"
func main() {
    var stuck = Stall{empties: 1000, calls: 0}
    let _, allErr = io.ReadAll(stuck)
    check(fails(allErr, "io.ReadAll: no read progress") && stuck.calls == 100, "ReadAll")
    var slow = Stall{empties: 99, calls: 0}
    let late, lateErr = io.ReadAll(slow)
    check(lateErr == nil && late.len() == 1, "ReadAll tolerates 99")

    var copyStuck = Stall{empties: 1000, calls: 0}
    var out = sink(10)
    let _, copyErr = io.Copy(out, copyStuck)
    check(fails(copyErr, "io.Copy: no read progress") && copyStuck.calls == 100, "Copy")

    var fullStuck = Stall{empties: 1000, calls: 0}
    var buf = Array<byte>{0, 0}
    let _, fullErr = io.ReadFull(fullStuck, buf[:])
    check(fails(fullErr, "io.ReadFull: no read progress") && fullStuck.calls == 100, "ReadFull")

    var bad = Bad{count: 1}
    let _, badErr = io.ReadAll(bad)
    check(fails(badErr, "io.ReadAll: invalid read count"), "ReadAll invalid count")
    var negative = Bad{count: -1}
    let _, negativeErr = io.ReadAll(negative)
    check(fails(negativeErr, "io.ReadAll: invalid read count"), "ReadAll negative count")

    var every = feed(256, 3)
    var copied = sink(5)
    let n, err = io.Copy(copied, every)
    check(err == nil && n == 256 && same(copied.data[:], expected(256)[:]), "Copy still copies every byte value")
    println("ok")
}
"#,
    );
    prints(&source, "ok\n");
}

#[test]
fn adapters_work_inside_async_functions_and_over_files() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("data.bin");
    let bytes: Vec<u8> = (0..1000u32).map(|i| ((i * 7 + 3) % 256) as u8).collect();
    std::fs::write(&path, &bytes).unwrap();
    let source = program(
        "import \"std/os\"",
        &r#"
async func head(path string, n int) (Array<byte>, error) {
    var file = os.Open(path)?
    var limited = io.LimitReader(file, n)
    let data = io.ReadAtMost(limited, n)?
    return data, nil
}

async func copyPart(path string) (Array<byte>, error) {
    var file = os.Open(path)?
    var out = sink(64)
    let _ = io.CopyN(out, file, 700)?
    return out.data, nil
}

func main() {
    var tasks = Array<Task<Array<byte>, error>>{}
    for var i = 0; i < 6; i += 1 { tasks.push(go head("PATH", 300)) }
    for tasks.len() > 0 {
        let _, task = tasks.pop()
        let data, err = task.wait()
        check(err == nil && same(data[:], expected(300)[:]), "async head")
    }
    let part = go copyPart("PATH")
    let copied, err = part.wait()
    check(err == nil && same(copied[:], expected(700)[:]), "async CopyN")
    var whole, _ = os.Open("PATH")
    let _, tooBig = io.ReadAtMost(whole, 999)
    check(tooBig == io.LimitExceeded, "file over the limit")
    println("ok")
}
"#
        .replace("PATH", &path.to_string_lossy().replace('\\', "/")),
    );
    prints(&source, "ok\n");
}
