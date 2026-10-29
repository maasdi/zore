use super::*;

const CAPTURE: &str = r#"
type Capture struct {
    data Mutex<Array<byte>>
}

func (c Capture) Write(p []byte) (int, error) {
    c.data.withLock(func(d mut Array<byte>) {
        for b in p { d.push(b) }
    })
    return p.len(), nil
}

func captured(data Mutex<Array<byte>>) string {
    return data.withLock(func(d mut Array<byte>) string {
        let text, err = strings.FromBytes(d[:])
        if err != nil { panic("captured bytes are not UTF-8") }
        return text
    })
}
"#;

fn program(imports: &str, body: &str) -> String {
    format!(
        "package main\n\nimport \"std/log\"\nimport \"std/strings\"\n{imports}\n{CAPTURE}\n{body}"
    )
}

#[test]
fn log_records_have_levels_fields_and_stable_escaping() {
    let source = program(
        "import \"std/os\"",
        r#"
func main() {
    let data = mutex(Array<byte>{})
    let logger = log.New(Capture{data: data}, log.LevelInfo).WithoutTime()
    logger.Debug("filtered")
    logger.Info("ready")
    logger.Warn("hello world")
    logger.Error("")
    let request = logger.With(Array<log.Field>{log.String("method", "GET"), log.Int("id", -7)}[:])
    let nested = request.With(Array<log.Field>{log.Bool("cached", true)}[:])
    nested.Log(log.LevelWarn, "slow", Array<log.Field>{log.Float("ratio", 0.25), log.Err(error("disk \"full\"")), log.Err(nil)}[:])
    request.Info("parent unchanged")
    logger.Log(log.LevelInfo, "values", Array<log.Field>{
        log.String("empty", ""),
        log.String("equals", "a=b"),
        log.String("slash", "c:\\tmp"),
        log.String("lines", "one\ntwo\r\tthree"),
        log.String("control", "\u0001\u001f\u007f"),
        log.String("utf8", "héllo"),
        log.String("bare", "!#$~"),
    }[:])
    logger.Log(log.LevelError, "keys", Array<log.Field>{log.String("", "x"), log.String("a key", "y"), log.String("k=\"é", "z")}[:])
    logger.Log(log.Level(7), "custom", Array<log.Field>{}[:])
    logger.WithLevel(log.LevelDebug).Debug("now enabled")
    println(logger.Enabled(log.LevelDebug))
    println(logger.Enabled(log.LevelInfo))
    println(logger.WithLevel(log.LevelError).Enabled(log.LevelWarn))
    println(log.Level(-1).String())
    println(logger.Err() == nil)
    let _, _ = os.Stdout().WriteString(captured(data))
}
"#,
    );
    prints(
        &source,
        concat!(
            "false\ntrue\nfalse\nLEVEL(-1)\ntrue\n",
            "level=INFO msg=ready\n",
            "level=WARN msg=\"hello world\"\n",
            "level=ERROR msg=\"\"\n",
            "level=WARN msg=slow method=GET id=-7 cached=true ratio=0.25 error=\"disk \\\"full\\\"\" error=<nil>\n",
            "level=INFO msg=\"parent unchanged\" method=GET id=-7\n",
            "level=INFO msg=values empty=\"\" equals=\"a=b\" slash=\"c:\\\\tmp\" lines=\"one\\ntwo\\r\\tthree\" control=\"\\u0001\\u001f\\u007f\" utf8=\"héllo\" bare=!#$~\n",
            "level=ERROR msg=keys _=x a_key=y k____=z\n",
            "level=LEVEL(7) msg=custom\n",
            "level=DEBUG msg=\"now enabled\"\n",
        ),
    );
}

#[test]
fn log_timestamps_are_utc_rfc3339_seconds() {
    let source = program(
        "import \"std/os\"\nimport \"std/time\"",
        r#"
func main() {
    println(log.FormatTime(0))
    println(log.FormatTime(-1))
    println(log.FormatTime(951782400))
    println(log.FormatTime(1793273029))
    println(log.FormatTime(-62135596800))
    println(log.FormatTime(253402300799))
    println(log.FormatTime(-62135596801))
    println(log.FormatTime(253402300800))
    let data = mutex(Array<byte>{})
    let before = time.UnixSeconds()
    log.New(Capture{data: data}, log.LevelInfo).Info("stamped")
    let after = time.UnixSeconds()
    let line = captured(data)
    if line != "time=" + log.FormatTime(before) + " level=INFO msg=stamped\n" && line != "time=" + log.FormatTime(after) + " level=INFO msg=stamped\n" {
        panic("unexpected timestamped record: " + line)
    }
    let _, _ = os.Stdout().WriteString(line[:5])
    println(line.len())
}
"#,
    );
    prints(
        &source,
        concat!(
            "1970-01-01T00:00:00Z\n",
            "1969-12-31T23:59:59Z\n",
            "2000-02-29T00:00:00Z\n",
            "2026-10-29T11:23:49Z\n",
            "0001-01-01T00:00:00Z\n",
            "9999-12-31T23:59:59Z\n",
            "-62135596801\n",
            "253402300800\n",
            "time=49\n",
        ),
    );
}

#[test]
fn log_records_from_plain_and_async_tasks_never_interleave() {
    let source = program(
        "import \"std/os\"\nimport \"std/time\"",
        r#"
type Trickle struct {
    data Mutex<Array<byte>>
}

func (t Trickle) Write(p []byte) (int, error) {
    if p.len() == 0 { return 0, nil }
    t.data.withLock(func(d mut Array<byte>) { d.push(p[0]) })
    time.Sleep(time.Microsecond)
    return 1, nil
}

func plainWorker(logger log.Logger, id int, done channel<bool>) {
    for var i = 0; i < 25; i += 1 {
        logger.Log(log.LevelInfo, "plain step", Array<log.Field>{log.Int("task", id), log.Int("i", i)}[:])
    }
    done.send(true)
}

async func asyncWorker(logger log.Logger, id int) int {
    for var i = 0; i < 25; i += 1 {
        logger.Log(log.LevelWarn, "async step", Array<log.Field>{log.Int("task", id), log.Int("i", i)}[:])
    }
    return id
}

func main() {
    let data = mutex(Array<byte>{})
    let logger = log.New(Trickle{data: data}, log.LevelDebug).WithoutTime()
    let done = channel<bool>(4)
    var tasks = Array<Task<int>>{}
    for var t = 0; t < 4; t += 1 {
        go plainWorker(logger, t, done)
        tasks.push(go asyncWorker(logger.With(Array<log.Field>{log.String("kind", "async")}[:]), t + 4))
    }
    for var t = 0; t < 4; t += 1 { let _, _ = done.receive() }
    for tasks.len() > 0 {
        let _, task = tasks.pop()
        _ = task.wait()
    }
    if logger.Err() != nil { panic("unexpected output failure") }
    let _, _ = os.Stdout().WriteString(captured(data))
}
"#,
    );
    let output = run(&source);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.ends_with('\n'));
    let mut next = [0usize; 8];
    for line in text.lines() {
        let (task, i) = if let Some(rest) = line.strip_prefix("level=INFO msg=\"plain step\" task=")
        {
            rest.split_once(" i=").unwrap()
        } else if let Some(rest) =
            line.strip_prefix("level=WARN msg=\"async step\" kind=async task=")
        {
            rest.split_once(" i=").unwrap()
        } else {
            panic!("interleaved record: {line:?}");
        };
        let task: usize = task.parse().unwrap();
        assert_eq!(i.parse::<usize>().unwrap(), next[task], "{line}");
        next[task] += 1;
    }
    assert_eq!(next, [25; 8]);
}

#[test]
fn log_output_failures_are_retried_reported_and_sticky() {
    let source = program(
        "import \"std/errors\"",
        r#"
let errFull = error("sink full")

type Faulty struct {
    calls  Mutex<int>
    data   Mutex<Array<byte>>
    limit  int
    mode   int
}

func (f Faulty) Write(p []byte) (int, error) {
    f.calls.withLock(func(n mut int) { n += 1 })
    if f.mode == 1 { return 0, nil }
    if f.mode == 2 { return p.len() + 1, nil }
    if f.mode == 3 { return -1, nil }
    let stored = f.data.withLock(func(d mut Array<byte>) int { return d.len() })
    var count = p.len()
    if count > 3 { count = 3 }
    if stored + count > f.limit {
        count = f.limit - stored
        f.data.withLock(func(d mut Array<byte>) {
            for var i = 0; i < count; i += 1 { d.push(p[i]) }
        })
        return count, errFull
    }
    f.data.withLock(func(d mut Array<byte>) {
        for var i = 0; i < count; i += 1 { d.push(p[i]) }
    })
    return count, nil
}

func calls(f Faulty) int {
    return f.calls.withLock(func(n mut int) int { return n })
}

func check(mode int, message string) {
    let sink = Faulty{calls: mutex(0), data: mutex(Array<byte>{}), limit: 0, mode: mode}
    let logger = log.New(sink, log.LevelInfo).WithoutTime()
    logger.Info("x")
    logger.Info("y")
    println(errors.Message(logger.Err()) == message)
    println(calls(sink))
}

func main() {
    let sink = Faulty{calls: mutex(0), data: mutex(Array<byte>{}), limit: 40, mode: 0}
    let logger = log.New(sink, log.LevelInfo).WithoutTime()
    logger.Info("short writes")
    println(logger.Err() == nil)
    println(calls(sink))
    let copy = logger.WithLevel(log.LevelDebug)
    copy.Info("cut here")
    println(logger.Err() == errFull)
    let before = calls(sink)
    copy.Error("dropped")
    logger.Info("dropped too")
    logger.Debug("disabled")
    println(calls(sink) == before)
    println(copy.Err() == errFull)
    println(captured(sink.data))
    check(1, "log: no write progress")
    check(2, "log: invalid write count")
    check(3, "log: invalid write count")
    let empty = channel<log.Logger>(1)
    empty.close()
    let zero, _ = empty.receive()
    zero.Error("nowhere")
    println(zero.Enabled(log.LevelError))
    println(errors.Message(zero.Err()))
}
"#,
    );
    prints(
        &source,
        concat!(
            "true\n10\ntrue\ntrue\ntrue\n",
            "level=INFO msg=\"short writes\"\nlevel=INFO\n",
            "true\n1\ntrue\n1\ntrue\n1\n",
            "false\nlog: no output\n",
        ),
    );
}

#[test]
fn log_sink_is_dropped_once_after_the_last_logger_copy() {
    let source = program(
        "import \"std/os\"",
        r#"
type Tracked struct {
    drops Mutex<int>
}

func (t Tracked) Write(p []byte) (int, error) { return p.len(), nil }

func (t mut Tracked) drop() { t.drops.withLock(func(n mut int) { n += 1 }) }

func count(drops Mutex<int>) int {
    return drops.withLock(func(n mut int) int { return n })
}

func keep(logger log.Logger, ready channel<bool>, release channel<bool>) {
    ready.send(true)
    let _, _ = release.receive()
    logger.Info("from task")
}

func main() {
    let drops = mutex(0)
    let ready = channel<bool>()
    let release = channel<bool>()
    var task = go keep(log.New(Tracked{drops: drops}, log.LevelInfo).WithoutTime(), ready, release)
    let _, _ = ready.receive()
    println(count(drops))
    release.send(true)
    task.wait()
    println(count(drops))

    let path = "log-cleanup.txt"
    let file, err = os.Create(path)
    if err != nil { panic("create") }
    write(file)
    let data, readErr = os.ReadFile(path)
    if readErr != nil { panic("read") }
    let text, _ = strings.FromBytes(data[:])
    let _, _ = os.Stdout().WriteString(text)
    if os.Remove(path) != nil { panic("remove") }
}

func write(file own os.File) {
    let logger = log.New(file, log.LevelInfo).WithoutTime()
    logger.With(Array<log.Field>{log.String("sink", "file")}[:]).Info("saved")
    if logger.Err() != nil { panic("file write") }
}
"#,
    );
    prints(&source, "0\n1\nlevel=INFO msg=saved sink=file\n");
}

#[test]
fn log_sink_panic_poisons_every_copy_of_the_logger() {
    let source = program(
        "",
        r#"
type Exploding struct {
    entered channel<bool>
}

func (e Exploding) Write(p []byte) (int, error) {
    e.entered.send(true)
    panic("sink exploded")
}

func attempt(logger log.Logger) {
    logger.Info("boom")
}

func main() {
    let entered = channel<bool>(1)
    let logger = log.New(Exploding{entered: entered}, log.LevelInfo)
    go attempt(logger.WithoutTime())
    let _, _ = entered.receive()
    println("entered")
    logger.Info("after")
}
"#,
    );
    let output = run(&source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "entered\n");
    let errors = stderr(&output);
    assert!(errors.contains("sink exploded"), "{errors}");
    assert!(
        errors.contains("panic in the main task: withLock on a poisoned mutex"),
        "{errors}"
    );
}
