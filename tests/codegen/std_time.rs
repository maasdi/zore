use super::*;

#[test]
fn durations_print_convert_and_instants_saturate() {
    prints(
        r##"package main

import "std/strconv"
import "std/time"

func main() {
    let durations = Array<time.Duration>{0, 1, 999, 1000, 1500, 999999, 1000000, 250 * time.Millisecond, time.Second, 1500 * time.Millisecond, time.Minute, 90 * time.Minute + 1, -2 * time.Microsecond, 25 * time.Hour + 3 * time.Second, time.Duration(9223372036854775807), time.Duration(-9223372036854775807 - 1)}
    for d in durations {
        println(d.String())
    }
    let d = 2 * time.Hour + 30 * time.Minute + 1500 * time.Millisecond
    println(strconv.Itoa(d.Hours()) + " " + strconv.Itoa(d.Minutes()) + " " + strconv.Itoa(d.Seconds()) + " " + strconv.Itoa(d.Milliseconds()) + " " + strconv.Itoa(d.Microseconds()) + " " + strconv.Itoa(d.Nanoseconds()))
    let back = -1500 * time.Millisecond
    println(strconv.Itoa(back.Seconds()) + " " + strconv.Itoa(back.Milliseconds()) + " " + strconv.Itoa(int(back)))
    let start = time.Now()
    let far = start.Add(time.Duration(9223372036854775807))
    println(far.After(start) && far.Equal(far.Add(time.Hour)))
    let past = start.Add(time.Duration(-9223372036854775807 - 1)).Add(-time.Hour)
    println(past.Before(start) && past.Equal(past.Add(-time.Hour)))
    println(far.Sub(past).String())
    println(past.Sub(far).String())
    println(start.Add(time.Second).Sub(start) == time.Second)
    println(time.Until(start) <= 0 && time.Since(start) >= 0 && start.Elapsed() >= 0)
    let later = time.Now()
    println(!later.Before(start) && start.Equal(start))
}
"##,
        r##"0s
1ns
999ns
1µs
1.5µs
999.999µs
1ms
250ms
1s
1.5s
1m0s
1h30m0.000000001s
-2µs
25h0m3s
2562047h47m16.854775807s
-2562047h47m16.854775808s
2 150 9001 9001500 9001500000 9001500000000
-1 -1500 -1500000000
true
true
2562047h47m16.854775807s
-2562047h47m16.854775808s
true
true
true
"##,
    );
}

#[test]
fn calendar_dates_check_fields_ranges_and_zones() {
    prints(
        r##"package main

import "std/errors"
import "std/strconv"
import "std/time"

func show(t time.Timestamp, err error) {
    if err != nil {
        println(errors.Message(err) + " | " + t.RFC3339())
        return
    }
    println(t.RFC3339Nano() + " unix=" + strconv.Itoa(t.Unix()) + " ns=" + strconv.Itoa(t.Nanosecond()) + " ymd=" + strconv.Itoa(t.Year()) + "-" + strconv.Itoa(t.Month()) + "-" + strconv.Itoa(t.Day()) + " hms=" + strconv.Itoa(t.Hour()) + ":" + strconv.Itoa(t.Minute()) + ":" + strconv.Itoa(t.Second()) + " wd=" + strconv.Itoa(t.Weekday()) + " yd=" + strconv.Itoa(t.YearDay()) + " off=" + t.Offset().String())
}

func main() {
    let utc = time.UTCZone
    let plus, _ = time.Offset(5 * time.Hour + 45 * time.Minute)
    let minus, _ = time.Offset(-(9 * time.Hour + 30 * time.Minute))
    let a, ae = time.Date(2000, 2, 29, 23, 59, 59, 999999999, utc)
    show(a, ae)
    let b, be = time.Date(1969, 12, 31, 23, 59, 59, 500000000, utc)
    show(b, be)
    let c, ce = time.Date(1, 1, 1, 0, 0, 0, 0, utc)
    show(c, ce)
    let d, de = time.Date(9999, 12, 31, 23, 59, 59, 999999999, utc)
    show(d, de)
    let e, ee = time.Date(1900, 3, 1, 0, 0, 0, 0, plus)
    show(e, ee)
    let f, fe = time.Date(2026, 10, 30, 12, 0, 0, 0, minus)
    show(f, fe)
    let t1, e1 = time.Date(1900, 2, 29, 0, 0, 0, 0, utc)
    show(t1, e1)
    let t2, e2 = time.Date(2026, 13, 1, 0, 0, 0, 0, utc)
    show(t2, e2)
    let t3, e3 = time.Date(2026, 4, 31, 0, 0, 0, 0, utc)
    show(t3, e3)
    let t4, e4 = time.Date(2026, 1, 1, 24, 0, 0, 0, utc)
    show(t4, e4)
    let t5, e5 = time.Date(2026, 1, 1, 0, 60, 0, 0, utc)
    show(t5, e5)
    let t6, e6 = time.Date(2026, 1, 1, 0, 0, 60, 0, utc)
    show(t6, e6)
    let t7, e7 = time.Date(2026, 1, 1, 0, 0, 0, 1000000000, utc)
    show(t7, e7)
    let t8, e8 = time.Date(0, 12, 31, 0, 0, 0, 0, utc)
    show(t8, e8)
    let t9, e9 = time.Date(1, 1, 1, 0, 0, 0, 0, plus)
    show(t9, e9)
    let t10, e10 = time.Date(9999, 12, 31, 23, 59, 59, 0, minus)
    show(t10, e10)
    let t11, e11 = time.Unix(-1, 0)
    show(t11, e11)
    let t12, e12 = time.Unix(0, -1)
    show(t12, e12)
    let t13, e13 = time.Unix(253402300800, 0)
    show(t13, e13)
    let t14, e14 = time.Unix(-62135596801, 0)
    show(t14, e14)
    let last, _ = time.Unix(253402300799, 999999999)
    println(last.In(plus).RFC3339Nano())
    let first, _ = time.Unix(-62135596800, 0)
    println(first.In(minus).RFC3339())
    println(last.Add(time.Nanosecond).RFC3339Nano())
    println(first.Add(-time.Nanosecond).RFC3339Nano())
    println(last.Sub(first).String())
    println(first.Sub(last).String())
    println(a.Add(time.Nanosecond).RFC3339Nano())
    println(b.Add(-1500 * time.Millisecond).RFC3339Nano())
    println(a.Sub(b).String())
    println(e.Before(f) && f.After(e) && !e.Equal(f) && e.In(minus).Equal(e))
    let _, z1 = time.Offset(24 * time.Hour)
    let _, z2 = time.Offset(30 * time.Second)
    let z3, z3e = time.Offset(-(23 * time.Hour + 59 * time.Minute))
    println(errors.Message(z1) + " / " + errors.Message(z2) + " / " + z3.Offset().String() + " " + strconv.FormatBool(z3e == nil))
}
"##,
        r##"2000-02-29T23:59:59.999999999Z unix=951868799 ns=999999999 ymd=2000-2-29 hms=23:59:59 wd=2 yd=60 off=0s
1969-12-31T23:59:59.5Z unix=-1 ns=500000000 ymd=1969-12-31 hms=23:59:59 wd=3 yd=365 off=0s
0001-01-01T00:00:00Z unix=-62135596800 ns=0 ymd=1-1-1 hms=0:0:0 wd=1 yd=1 off=0s
9999-12-31T23:59:59.999999999Z unix=253402300799 ns=999999999 ymd=9999-12-31 hms=23:59:59 wd=5 yd=365 off=0s
1900-03-01T00:00:00+05:45 unix=-2203911900 ns=0 ymd=1900-3-1 hms=0:0:0 wd=4 yd=60 off=5h45m0s
2026-10-30T12:00:00-09:30 unix=1793395800 ns=0 ymd=2026-10-30 hms=12:0:0 wd=5 yd=303 off=-9h30m0s
time.Date: day outside the month | 1970-01-01T00:00:00Z
time.Date: month outside 1..12 | 1970-01-01T00:00:00Z
time.Date: day outside the month | 1970-01-01T00:00:00Z
time.Date: hour outside 0..23 | 1970-01-01T00:00:00Z
time.Date: minute outside 0..59 | 1970-01-01T00:00:00Z
time.Date: second outside 0..59 | 1970-01-01T00:00:00Z
time.Date: nanosecond outside 0..999999999 | 1970-01-01T00:00:00Z
time.Date: year outside 1..9999 | 1970-01-01T00:00:00Z
time.Date: outside years 0001..9999 UTC | 1970-01-01T00:00:00Z
time.Date: outside years 0001..9999 UTC | 1970-01-01T00:00:00Z
1969-12-31T23:59:59Z unix=-1 ns=0 ymd=1969-12-31 hms=23:59:59 wd=3 yd=365 off=0s
time.Unix: nanoseconds outside 0..999999999 | 1970-01-01T00:00:00Z
time.Unix: outside years 0001..9999 UTC | 1970-01-01T00:00:00Z
time.Unix: outside years 0001..9999 UTC | 1970-01-01T00:00:00Z
10000-01-01T05:44:59.999999999+05:45
0000-12-31T14:30:00-09:30
9999-12-31T23:59:59.999999999Z
0001-01-01T00:00:00Z
2562047h47m16.854775807s
-2562047h47m16.854775808s
2000-03-01T00:00:00Z
1969-12-31T23:59:58Z
264408h0m0.499999999s
true
time.Offset: an offset is whole minutes within 24 hours / time.Offset: an offset is whole minutes within 24 hours / -23h59m0s true
"##,
    );
}

#[test]
fn rfc3339_parsing_accepts_the_form_and_names_rejected_text() {
    prints(
        r##"package main

import "std/errors"
import "std/time"

func main() {
    let inputs = Array<string>{
        "2026-10-30T12:34:56Z",
        "2026-10-30t12:34:56z",
        "2026-10-30T12:34:56.5+07:00",
        "2026-10-30T12:34:56.123456789-09:30",
        "2026-10-30T12:34:56.000+00:00",
        "1969-12-31T23:59:59.999999999Z",
        "0001-01-01T00:00:00Z",
        "9999-12-31T23:59:59.999999999Z",
        "2024-02-29T00:00:00Z",
        "2026-11-01T01:00:00-05:00",
        "0001-01-01T00:00:00+00:01",
        "9999-12-31T23:59:59-00:01",
        "2023-02-29T00:00:00Z",
        "2026-10-30T12:34:60Z",
        "2026-10-30T24:00:00Z",
        "2026-10-30 12:34:56Z",
        "2026-10-30T12:34:56",
        "2026-10-30T12:34:56+24:00",
        "2026-10-30T12:34:56+07:60",
        "2026-10-30T12:34:56+0700",
        "2026-10-30T12:34:56.Z",
        "2026-10-30T12:34:56.1234567890Z",
        "2026-10-30T12:34:56Zjunk",
        "+2026-10-30T12:34:56Z",
        "2026-1-30T12:34:56Z",
        "0000-12-31T00:00:00Z",
        "",
    }
    for text in inputs {
        let t, err = time.ParseRFC3339(text)
        if err != nil {
            println(text + " -> " + errors.Message(err))
        } else {
            println(text + " -> " + t.RFC3339Nano() + " = " + t.UTC().RFC3339Nano())
        }
    }
}
"##,
        r##"2026-10-30T12:34:56Z -> 2026-10-30T12:34:56Z = 2026-10-30T12:34:56Z
2026-10-30t12:34:56z -> 2026-10-30T12:34:56Z = 2026-10-30T12:34:56Z
2026-10-30T12:34:56.5+07:00 -> 2026-10-30T12:34:56.5+07:00 = 2026-10-30T05:34:56.5Z
2026-10-30T12:34:56.123456789-09:30 -> 2026-10-30T12:34:56.123456789-09:30 = 2026-10-30T22:04:56.123456789Z
2026-10-30T12:34:56.000+00:00 -> 2026-10-30T12:34:56Z = 2026-10-30T12:34:56Z
1969-12-31T23:59:59.999999999Z -> 1969-12-31T23:59:59.999999999Z = 1969-12-31T23:59:59.999999999Z
0001-01-01T00:00:00Z -> 0001-01-01T00:00:00Z = 0001-01-01T00:00:00Z
9999-12-31T23:59:59.999999999Z -> 9999-12-31T23:59:59.999999999Z = 9999-12-31T23:59:59.999999999Z
2024-02-29T00:00:00Z -> 2024-02-29T00:00:00Z = 2024-02-29T00:00:00Z
2026-11-01T01:00:00-05:00 -> 2026-11-01T01:00:00-05:00 = 2026-11-01T06:00:00Z
0001-01-01T00:00:00+00:01 -> time.ParseRFC3339: parsing "0001-01-01T00:00:00+00:01": outside years 0001..9999 UTC
9999-12-31T23:59:59-00:01 -> time.ParseRFC3339: parsing "9999-12-31T23:59:59-00:01": outside years 0001..9999 UTC
2023-02-29T00:00:00Z -> time.ParseRFC3339: parsing "2023-02-29T00:00:00Z": day outside the month
2026-10-30T12:34:60Z -> time.ParseRFC3339: parsing "2026-10-30T12:34:60Z": second outside 0..59
2026-10-30T24:00:00Z -> time.ParseRFC3339: parsing "2026-10-30T24:00:00Z": hour outside 0..23
2026-10-30 12:34:56Z -> time.ParseRFC3339: parsing "2026-10-30 12:34:56Z": not an RFC 3339 timestamp
2026-10-30T12:34:56 -> time.ParseRFC3339: parsing "2026-10-30T12:34:56": not an RFC 3339 timestamp
2026-10-30T12:34:56+24:00 -> time.ParseRFC3339: parsing "2026-10-30T12:34:56+24:00": not an RFC 3339 timestamp
2026-10-30T12:34:56+07:60 -> time.ParseRFC3339: parsing "2026-10-30T12:34:56+07:60": not an RFC 3339 timestamp
2026-10-30T12:34:56+0700 -> time.ParseRFC3339: parsing "2026-10-30T12:34:56+0700": not an RFC 3339 timestamp
2026-10-30T12:34:56.Z -> time.ParseRFC3339: parsing "2026-10-30T12:34:56.Z": not an RFC 3339 timestamp
2026-10-30T12:34:56.1234567890Z -> time.ParseRFC3339: parsing "2026-10-30T12:34:56.1234567890Z": not an RFC 3339 timestamp
2026-10-30T12:34:56Zjunk -> time.ParseRFC3339: parsing "2026-10-30T12:34:56Zjunk": not an RFC 3339 timestamp
+2026-10-30T12:34:56Z -> time.ParseRFC3339: parsing "+2026-10-30T12:34:56Z": not an RFC 3339 timestamp
2026-1-30T12:34:56Z -> time.ParseRFC3339: parsing "2026-1-30T12:34:56Z": not an RFC 3339 timestamp
0000-12-31T00:00:00Z -> time.ParseRFC3339: parsing "0000-12-31T00:00:00Z": year outside 1..9999
 -> time.ParseRFC3339: parsing "": not an RFC 3339 timestamp
"##,
    );
}

#[test]
fn timers_and_tickers_fire_stop_reset_and_drain() {
    prints(
        r##"package main

import "std/errors"
import "std/time"

func ready(c channel<bool>) bool {
    select {
        case c.receive() { return true }
        default { return false }
    }
}

func arrives(c channel<bool>, within time.Duration) bool {
    select {
        case c.receive() { return true }
        case time.After(within).receive() { return false }
    }
}

func main() {
    let started = time.Now()
    var timer = time.NewTimer(20 * time.Millisecond)
    println(arrives(timer.C(), 5 * time.Second))
    println(time.Since(started) >= 20 * time.Millisecond)
    println(timer.Stop() == false)
    println(timer.Reset(time.Hour) == false)
    println(timer.Stop())
    println(arrives(timer.C(), 50 * time.Millisecond) == false)

    println(timer.Reset(10 * time.Millisecond) == false)
    time.Sleep(40 * time.Millisecond)
    println(timer.Reset(time.Hour) == false)
    println(ready(timer.C()) == false)

    var immediate = time.NewTimer(0)
    println(arrives(immediate.C(), 5 * time.Second))
    var negative = time.NewTimer(-time.Second)
    println(arrives(negative.C(), 5 * time.Second))

    var ticker, err = time.NewTicker(10 * time.Millisecond)
    println(err == nil)
    let tickStart = time.Now()
    for var i = 0; i < 3; i += 1 {
        println(arrives(ticker.C(), 5 * time.Second))
    }
    println(time.Since(tickStart) >= 25 * time.Millisecond)
    ticker.Stop()
    println(arrives(ticker.C(), 50 * time.Millisecond) == false)
    println(ticker.Reset(5 * time.Millisecond) == nil)
    println(arrives(ticker.C(), 5 * time.Second))
    println(errors.Message(ticker.Reset(0)))

    let bad, badErr = time.NewTicker(0)
    println(errors.Message(badErr))
    println(arrives(bad.C(), 30 * time.Millisecond) == false)

    for var i = 0; i < 1000; i += 1 {
        let waiting = time.NewTimer(time.Hour)
        _ = waiting.C()
    }
    println("dropped")
}
"##,
        r##"true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
true
time.Ticker.Reset: the period must be positive
time.NewTicker: the period must be positive
true
dropped
"##,
    );
}

#[test]
fn dropped_timers_and_tickers_leave_no_waiting_task() {
    let source = r#"package main

import "std/time"

func main() {
    for var i = 0; i < 100; i += 1 {
        var waiting = time.NewTimer(time.Hour)
        _ = waiting.Stop()
    }
    for var i = 0; i < 100; i += 1 {
        let waiting = time.NewTimer(time.Hour)
        _ = waiting.C()
    }
    let ticker, _ = time.NewTicker(time.Hour)
    drop(ticker)
    time.Sleep(20 * time.Millisecond)
    println("waiting")
    let never = channel<bool>()
    let _, _ = never.receive()
}
"#;
    let started = std::time::Instant::now();
    let output = run(source);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stdout(&output), "waiting\n");
    assert!(
        stderr(&output).contains("all tasks are asleep"),
        "{}",
        stderr(&output)
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}

#[cfg(unix)]
#[test]
fn local_timestamps_follow_the_zone_across_daylight_saving() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("program");
    let source = r#"package main

import "std/time"

func main() {
    for seconds in Array<int>{1772953199, 1772953200, 1793512799, 1793512800}[:] {
        let t, _ = time.Unix(seconds, 0)
        let local = t.Local()
        println(local.RFC3339() + " " + local.Offset().String() + " " + local.UTC().RFC3339())
    }
    let summer, _ = time.Unix(1793512799, 0)
    println(summer.Local().Add(time.Second).RFC3339())
    println(summer.Add(time.Second).Local().RFC3339())
}
"#;
    build_file(source, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let output = Command::new(&executable)
        .env("TZ", "EST5EDT,M3.2.0,M11.1.0")
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        concat!(
            "2026-03-08T01:59:59-05:00 -5h0m0s 2026-03-08T06:59:59Z\n",
            "2026-03-08T03:00:00-04:00 -4h0m0s 2026-03-08T07:00:00Z\n",
            "2026-11-01T01:59:59-04:00 -4h0m0s 2026-11-01T05:59:59Z\n",
            "2026-11-01T01:00:00-05:00 -5h0m0s 2026-11-01T06:00:00Z\n",
            "2026-11-01T02:00:00-04:00\n",
            "2026-11-01T01:00:00-05:00\n",
        )
    );
}
