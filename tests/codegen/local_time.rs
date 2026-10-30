use super::*;

#[cfg(windows)]
const INSTANTS: [i64; 5] = [
    0,
    1_772_953_199,
    1_772_953_200,
    1_793_512_799,
    1_793_512_800,
];

const OFFSETS: &str = r#"package main

import "std/log"
import "std/strconv"
import "std/time"

func main() {
    for t in Array<int>{0, 1772953199, 1772953200, 1793512799, 1793512800}[:] {
        println(strconv.Itoa(t) + " " + strconv.Itoa(time.LocalOffset(t)) + " " + log.FormatLocalTime(t))
    }
}
"#;

fn build(source: &str, dir: &TempDir) -> std::path::PathBuf {
    let executable = dir.executable("program");
    build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    executable
}

#[cfg(unix)]
fn run_in_zone(executable: &Path, zone: &str) -> String {
    let output = Command::new(executable)
        .env("TZ", zone)
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    stdout(&output)
}

#[cfg(unix)]
#[test]
fn local_offsets_follow_the_zone_across_daylight_saving() {
    let dir = TempDir::new().unwrap();
    let executable = build(OFFSETS, &dir);
    for (zone, lines) in [
        (
            "UTC0",
            [
                "0 0 1970-01-01T00:00:00+00:00",
                "1772953199 0 2026-03-08T06:59:59+00:00",
                "1772953200 0 2026-03-08T07:00:00+00:00",
                "1793512799 0 2026-11-01T05:59:59+00:00",
                "1793512800 0 2026-11-01T06:00:00+00:00",
            ],
        ),
        (
            "<+07>-7",
            [
                "0 25200 1970-01-01T07:00:00+07:00",
                "1772953199 25200 2026-03-08T13:59:59+07:00",
                "1772953200 25200 2026-03-08T14:00:00+07:00",
                "1793512799 25200 2026-11-01T12:59:59+07:00",
                "1793512800 25200 2026-11-01T13:00:00+07:00",
            ],
        ),
        (
            "EST5EDT,M3.2.0,M11.1.0",
            [
                "0 -18000 1969-12-31T19:00:00-05:00",
                "1772953199 -18000 2026-03-08T01:59:59-05:00",
                "1772953200 -14400 2026-03-08T03:00:00-04:00",
                "1793512799 -14400 2026-11-01T01:59:59-04:00",
                "1793512800 -18000 2026-11-01T01:00:00-05:00",
            ],
        ),
        (
            "<+0530>-5:30",
            [
                "0 19800 1970-01-01T05:30:00+05:30",
                "1772953199 19800 2026-03-08T12:29:59+05:30",
                "1772953200 19800 2026-03-08T12:30:00+05:30",
                "1793512799 19800 2026-11-01T11:29:59+05:30",
                "1793512800 19800 2026-11-01T11:30:00+05:30",
            ],
        ),
        (
            "<-0330>3:30",
            [
                "0 -12600 1969-12-31T20:30:00-03:30",
                "1772953199 -12600 2026-03-08T03:29:59-03:30",
                "1772953200 -12600 2026-03-08T03:30:00-03:30",
                "1793512799 -12600 2026-11-01T02:29:59-03:30",
                "1793512800 -12600 2026-11-01T02:30:00-03:30",
            ],
        ),
    ] {
        let expected: String = lines.iter().map(|line| format!("{line}\n")).collect();
        assert_eq!(run_in_zone(&executable, zone), expected, "TZ={zone}");
    }
}

#[cfg(unix)]
#[test]
fn local_log_stamps_carry_the_zone_offset() {
    let dir = TempDir::new().unwrap();
    let source = r#"package main

import "std/log"
import "std/os"
import "std/time"

func main() {
    let before = time.UnixSeconds()
    log.New(os.Stdout(), log.LevelInfo).WithLocalTime().Info("local")
    log.New(os.Stdout(), log.LevelInfo).Info("utc")
    let after = time.UnixSeconds()
    println(log.FormatLocalTime(before))
    println(log.FormatLocalTime(after))
}
"#;
    let executable = build(source, &dir);
    let output = run_in_zone(&executable, "<+07>-7");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 4, "{output}");
    let local = lines[0]
        .strip_suffix(" level=INFO msg=local")
        .and_then(|rest| rest.strip_prefix("time="))
        .unwrap_or_else(|| panic!("{output}"));
    assert!(local == lines[2] || local == lines[3], "{output}");
    assert!(local.ends_with("+07:00"), "{output}");
    assert!(
        lines[1].starts_with("time=") && lines[1].contains("Z level=INFO msg=utc"),
        "{output}"
    );
}

#[cfg(unix)]
fn set_modified(path: &Path, unix_seconds: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix_seconds))
        .unwrap();
}

#[cfg(unix)]
#[test]
fn local_daily_rolling_names_backups_by_local_date() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().display().to_string();
    let source = format!(
        r#"package main

import "std/errors"
import "std/log"
import "std/log/rolling"
import "std/os"
import "std/time"

func session(name string, local bool) {{
    var config = rolling.DefaultConfig("{root}/" + name)
    config.Daily = true
    config.Local = local
    config.MaxBytes = 1000000
    var output, err = rolling.Open(config)
    if err != nil {{ panic(errors.Message(err)) }}
    let logger = log.New(output.Sink(), log.LevelInfo).WithoutTime()
    logger.Info("today")
    if output.Close() != nil || logger.Err() != nil {{ panic("close") }}
}}

func main() {{
    let before = log.FormatLocalTime(time.UnixSeconds())[:10]
    session("local.log", true)
    session("utc.log", false)
    let after = log.FormatLocalTime(time.UnixSeconds())[:10]
    if before != after {{ panic("the test crossed local midnight; run it again") }}
    let _, _ = os.Stdout().WriteString(before)
}}
"#
    );
    let executable = build(&source, &dir);
    let evening_before_daylight_saving_ends = 1_793_503_800;
    for name in ["local.log", "utc.log"] {
        let path = dir.path().join(name);
        std::fs::write(&path, "old\n").unwrap();
        set_modified(&path, evening_before_daylight_saving_ends);
    }
    let today = run_in_zone(&executable, "EST5EDT,M3.2.0,M11.1.0");
    let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).ok();
    assert_eq!(read("local.log.2026-10-31").as_deref(), Some("old\n"));
    assert_eq!(read("utc.log.2026-11-01").as_deref(), Some("old\n"));
    assert_eq!(read("local.log.2026-11-01"), None);
    assert_eq!(read("local.log").as_deref(), Some("level=INFO msg=today\n"));
    assert_eq!(today.len(), 10, "{today}");
}

#[cfg(windows)]
#[test]
fn local_offsets_match_the_windows_time_zone() {
    let dir = TempDir::new().unwrap();
    let executable = build(OFFSETS, &dir);
    let output = Command::new(&executable).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    let script = INSTANTS
        .iter()
        .map(|t| {
            format!(
                "[int][System.TimeZoneInfo]::Local.GetUtcOffset([DateTimeOffset]::FromUnixTimeSeconds({t})).TotalSeconds"
            )
        })
        .collect::<Vec<_>>()
        .join(";");
    let windows = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .unwrap();
    assert!(windows.status.success(), "{}", stderr(&windows));
    let expected: Vec<i64> = stdout(&windows)
        .lines()
        .map(|line| line.trim().parse().unwrap())
        .collect();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), INSTANTS.len(), "{text}");
    for ((line, t), offset) in lines.iter().zip(INSTANTS).zip(expected) {
        let fields: Vec<&str> = line.split(' ').collect();
        assert_eq!(fields[0], t.to_string(), "{text}");
        assert_eq!(fields[1], offset.to_string(), "{text}");
        let minutes = offset / 60;
        let suffix = format!(
            "{}{:02}:{:02}",
            if minutes < 0 { '-' } else { '+' },
            minutes.abs() / 60,
            minutes.abs() % 60
        );
        assert!(fields[2].ends_with(&suffix), "{text}");
    }
}
