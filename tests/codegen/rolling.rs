use super::*;

const HELPERS: &str = r#"
func read(path string) string {
    let data, err = os.ReadFile(path)
    if err != nil { return "<missing>" }
    let text, _ = strings.FromBytes(data[:])
    return text
}

func show(path string) {
    let _, _ = os.Stdout().WriteString("== " + path + "\n" + read(path))
}

func config(path string, maxBytes int, backups int, buffer int) rolling.Config {
    var config = rolling.DefaultConfig(path)
    config.MaxBytes = maxBytes
    config.MaxBackups = backups
    config.BufferBytes = buffer
    return config
}
"#;

fn root(dir: &TempDir) -> String {
    dir.path().display().to_string().replace('\\', "/")
}

fn program(dir: &TempDir, imports: &[&str], body: &str) -> String {
    let mut header = String::from(
        "package main\n\nimport \"std/log/rolling\"\nimport \"std/os\"\nimport \"std/strings\"\n",
    );
    for import in imports {
        header.push_str(&format!("import \"std/{import}\"\n"));
    }
    format!("{header}\nlet dir = \"{}\"\n{HELPERS}\n{body}", root(dir))
}

#[test]
fn os_append_keeps_contents_and_rename_replaces_the_target() {
    let dir = TempDir::new().unwrap();
    let source = program(
        &dir,
        &["errors"],
        r#"
func main() {
    let path = dir + "/a.txt"
    let first, err = os.Append(path, 0o644)
    if err != nil { panic(errors.Message(err)) }
    let _, _ = first.WriteString("one\n")
    drop(first)
    let second, again = os.Append(path, 0o644)
    if again != nil { panic(errors.Message(again)) }
    let _, _ = second.WriteString("two\n")
    drop(second)
    if os.WriteFile(dir + "/b.txt", strings.Bytes("old\n")[:], 0o644) != nil { panic("write") }
    if os.Rename(path, dir + "/b.txt") != nil { panic("rename") }
    show(dir + "/a.txt")
    show(dir + "/b.txt")
    println(strings.HasPrefix(errors.Message(os.Rename(dir + "/none", dir + "/c.txt")), "os.Rename: "))
    let _, missing = os.Append(dir + "/no/such/x.txt", 0o644)
    println(strings.HasPrefix(errors.Message(missing), "os.Append: "))
}
"#,
    );
    let root = root(&dir);
    prints(
        &source,
        &format!("== {root}/a.txt\n<missing>== {root}/b.txt\none\ntwo\ntrue\ntrue\n"),
    );
}

#[test]
fn rolling_moves_files_aside_and_keeps_the_newest_backups() {
    let dir = TempDir::new().unwrap();
    let source = program(
        &dir,
        &["errors", "log"],
        r#"
func main() {
    let path = dir + "/app.log"
    var output, err = rolling.Open(config(path, 100, 2, 0))
    if err != nil { panic(errors.Message(err)) }
    let logger = log.New(output.Sink(), log.LevelInfo).WithoutTime()
    for var i = 0; i < 10; i += 1 {
        logger.Info("line {i}", i)
    }
    if output.Close() != nil { panic("close") }
    println(logger.Err() == nil)
    show(path)
    show(path + ".1")
    show(path + ".2")
    show(path + ".3")

    let single = dir + "/single.log"
    var only, openErr = rolling.Open(config(single, 40, 0, 0))
    if openErr != nil { panic(errors.Message(openErr)) }
    let quiet = log.New(only.Sink(), log.LevelInfo).WithoutTime()
    quiet.Info("first record")
    quiet.Info("second record")
    quiet.Info("a record that is longer than the whole limit")
    if only.Close() != nil { panic("close single") }
    show(single)
    show(single + ".1")
}
"#,
    );
    let root = root(&dir);
    let line = |i: usize| format!("level=INFO msg=\"line {i}\" i={i}\n");
    let expected = format!(
        "true\n== {root}/app.log\n{}== {root}/app.log.1\n{}{}{}== {root}/app.log.2\n{}{}{}== {root}/app.log.3\n<missing>== {root}/single.log\nlevel=INFO msg=\"a record that is longer than the whole limit\"\n== {root}/single.log.1\n<missing>",
        line(9),
        line(6),
        line(7),
        line(8),
        line(3),
        line(4),
        line(5),
    );
    prints(&source, &expected);
}

#[test]
fn rolling_resumes_after_a_restart_and_counts_existing_bytes() {
    let dir = TempDir::new().unwrap();
    let source = program(
        &dir,
        &["errors"],
        r#"
func session(path string, text string) {
    var output, err = rolling.Open(config(path, 30, 3, 0))
    if err != nil { panic(errors.Message(err)) }
    let sink = output.Sink()
    let _, writeErr = sink.Write(strings.Bytes(text)[:])
    if writeErr != nil { panic(errors.Message(writeErr)) }
}

func main() {
    let path = dir + "/restart.log"
    session(path, "first run\n")
    session(path, "second run\n")
    session(path, "third run is long\n")
    show(path)
    show(path + ".1")
}
"#,
    );
    let root = root(&dir);
    prints(
        &source,
        &format!(
            "== {root}/restart.log\nthird run is long\n== {root}/restart.log.1\nfirst run\nsecond run\n"
        ),
    );
}

#[test]
fn rolling_buffers_until_full_flushed_closed_or_dropped() {
    let dir = TempDir::new().unwrap();
    let source = program(
        &dir,
        &["errors", "log", "time"],
        r#"
func dropped(path string) {
    var output, err = rolling.Open(config(path, 1000000, 1, 4096))
    if err != nil { panic(errors.Message(err)) }
    let logger = log.New(output.Sink(), log.LevelInfo).WithoutTime()
    logger.Info("written when the writer is dropped")
}

func waitFor(path string, text string) bool {
    let deadline = time.Now() + 5 * time.Second
    for time.Now() < deadline {
        if read(path) == text { return true }
        time.Sleep(5 * time.Millisecond)
    }
    return false
}

func main() {
    let path = dir + "/buffered.log"
    var slow = config(path, 1000000, 1, 64)
    slow.FlushInterval = time.Hour
    var output, err = rolling.Open(slow)
    if err != nil { panic(errors.Message(err)) }
    let logger = log.New(output.Sink(), log.LevelInfo).WithoutTime()
    logger.Info("held")
    show(path)
    println("")
    logger.Info("this record fills the sixty four byte buffer")
    show(path)
    logger.Info("held again")
    if output.Flush() != nil { panic("flush") }
    show(path)
    logger.Info("written on close")
    if output.Close() != nil { panic("close") }
    show(path)
    logger.Info("after close")
    println(errors.Message(logger.Err()))

    let ticking = dir + "/ticking.log"
    var fast = config(ticking, 1000000, 1, 4096)
    fast.FlushInterval = 10 * time.Millisecond
    var background, openErr = rolling.Open(fast)
    if openErr != nil { panic(errors.Message(openErr)) }
    let ticker = log.New(background.Sink(), log.LevelInfo).WithoutTime()
    ticker.Info("flushed by the background task")
    println(waitFor(ticking, "level=INFO msg=\"flushed by the background task\"\n"))
    drop(background)

    let droppedPath = dir + "/dropped.log"
    dropped(droppedPath)
    show(droppedPath)
}
"#,
    );
    let root = root(&dir);
    prints(
        &source,
        &format!(
            concat!(
                "== {root}/buffered.log\n\n",
                "== {root}/buffered.log\nlevel=INFO msg=held\nlevel=INFO msg=\"this record fills the sixty four byte buffer\"\n",
                "== {root}/buffered.log\nlevel=INFO msg=held\nlevel=INFO msg=\"this record fills the sixty four byte buffer\"\nlevel=INFO msg=\"held again\"\n",
                "== {root}/buffered.log\nlevel=INFO msg=held\nlevel=INFO msg=\"this record fills the sixty four byte buffer\"\nlevel=INFO msg=\"held again\"\nlevel=INFO msg=\"written on close\"\n",
                "rolling: closed\n",
                "true\n",
                "== {root}/dropped.log\nlevel=INFO msg=\"written when the writer is dropped\"\n",
            ),
            root = root
        ),
    );
}

#[test]
fn rolling_rejects_bad_configs_and_keeps_the_first_failure() {
    let dir = TempDir::new().unwrap();
    let source = program(
        &dir,
        &["errors", "log"],
        r#"
func attempt(config rolling.Config) {
    var output, err = rolling.Open(config)
    println(errors.Message(err))
    let _, writeErr = output.Sink().Write(strings.Bytes("x")[:])
    println(errors.Message(writeErr))
}

func main() {
    let path = dir + "/bad.log"
    attempt(config("", 10, 1, 0))
    attempt(config(path, 0, 1, 0))
    attempt(config(path, 10, -1, 0))
    attempt(config(path, 10, 1, -1))
    var noInterval = config(path, 10, 1, 16)
    noInterval.FlushInterval = 0
    attempt(noInterval)
    println(read(path))

    let blocked = dir + "/blocked.log"
    if os.MkdirAll(blocked + ".1/inside", 0o755) != nil { panic("mkdir") }
    var output, err = rolling.Open(config(blocked, 20, 1, 0))
    if err != nil { panic(errors.Message(err)) }
    let logger = log.New(output.Sink(), log.LevelInfo).WithoutTime()
    logger.Info("fits in the file")
    logger.Info("needs a roll")
    let failure = logger.Err()
    println(strings.HasPrefix(errors.Message(failure), "os.Remove: "))
    println(output.Flush() == failure)
    show(blocked)
    println(output.Close() == failure)
}
"#,
    );
    let root = root(&dir);
    let mut expected = String::new();
    for message in [
        "rolling.Open: empty path",
        "rolling.Open: MaxBytes must be positive",
        "rolling.Open: MaxBackups must not be negative",
        "rolling.Open: BufferBytes must not be negative",
        "rolling.Open: FlushInterval must be positive when buffering",
    ] {
        expected.push_str(&format!("{message}\n{message}\n"));
    }
    expected.push_str(&format!(
        "<missing>\ntrue\ntrue\n== {root}/blocked.log\nlevel=INFO msg=\"fits in the file\"\ntrue\n"
    ));
    prints(&source, &expected);
}

#[test]
fn rolling_records_from_many_tasks_and_loggers_stay_whole_across_rolls() {
    let dir = TempDir::new().unwrap();
    let source = program(
        &dir,
        &["errors", "log"],
        r#"
func plain(logger log.Logger, id int, done channel<bool>) {
    for var i = 0; i < 50; i += 1 {
        logger.Info("plain {task} {i}", id, i)
    }
    done.send(true)
}

async func pending(logger log.Logger, id int) int {
    for var i = 0; i < 50; i += 1 {
        logger.Warn("async {task} {i}", id, i)
    }
    return id
}

func main() {
    let path = dir + "/shared.log"
    var output, err = rolling.Open(config(path, 2000, 40, 256))
    if err != nil { panic(errors.Message(err)) }
    let first = log.New(output.Sink(), log.LevelInfo).WithoutTime()
    let second = log.New(output.Sink(), log.LevelInfo).WithoutTime()
    let done = channel<bool>(4)
    var tasks = Array<Task<int>>{}
    for var t = 0; t < 4; t += 1 {
        go plain(first, t, done)
        tasks.push(go pending(second, t + 4))
    }
    for var t = 0; t < 4; t += 1 { let _, _ = done.receive() }
    for tasks.len() > 0 {
        let _, task = tasks.pop()
        _ = task.wait()
    }
    if output.Close() != nil { panic("close") }
    if first.Err() != nil || second.Err() != nil { panic("log failure") }
    println("done")
}
"#,
    );
    prints(&source, "done\n");
    let mut seen = std::collections::BTreeSet::new();
    let mut files = 0;
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.len() <= 2000,
            "{} holds {} bytes",
            path.display(),
            text.len()
        );
        assert!(text.ends_with('\n'), "{}", path.display());
        files += 1;
        for line in text.lines() {
            let (kind, level) = if line.starts_with("level=INFO") {
                ("plain", "INFO")
            } else {
                ("async", "WARN")
            };
            let numbers = line
                .strip_prefix(&format!("level={level} msg=\"{kind} "))
                .and_then(|rest| rest.split_once('"'))
                .map(|(numbers, _)| numbers)
                .unwrap_or_else(|| panic!("split record: {line:?}"));
            let (task, i) = numbers.split_once(' ').unwrap();
            assert_eq!(
                line,
                format!("level={level} msg=\"{kind} {task} {i}\" task={task} i={i}"),
                "split record"
            );
            assert!(
                seen.insert((task.to_string(), i.to_string())),
                "duplicate {line}"
            );
        }
    }
    assert_eq!(seen.len(), 400);
    assert!(files > 5, "expected several rolled files, found {files}");
}
