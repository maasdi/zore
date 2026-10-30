# Durations, instants, calendar times, and timers conformance cases

Authority: spec §37.3. Native cases are in `tests/codegen/std_time.rs` and run
with leak checking; checking cases are in `tests/packages/packages.rs`
(`time_types_do_not_mix`). Older sleep, `After`, and local-offset cases are in
`io.md`.

## Durations and instants

| Scenario | Expected result |
| --- | --- |
| `String` of `0`, `1`, `999`, `1000`, `1500`, `999999`, `1000000`, `250 * Millisecond`, `Second`, `1500 * Millisecond`, `Minute`, `90 * Minute + 1`, `-2 * Microsecond`, `25 * Hour + 3 * Second` | `0s`, `1ns`, `999ns`, `1µs`, `1.5µs`, `999.999µs`, `1ms`, `250ms`, `1s`, `1.5s`, `1m0s`, `1h30m0.000000001s`, `-2µs`, `25h0m3s` |
| `String` of the largest and smallest `Duration` | `2562047h47m16.854775807s`, `-2562047h47m16.854775808s` |
| `Hours` through `Nanoseconds` of `2h30m1.5s`; `Seconds`, `Milliseconds` of `-1.5s` | `2 150 9001 9001500 9001500000 9001500000000`; `-1 -1500`, rounded toward zero |
| `Now().Add` of the largest `Duration`, then `Add(Hour)` again | Stops at the largest reading; the second add changes nothing |
| `Add` far past the smallest reading, then `Add(-Hour)` | Stops at the smallest reading |
| `Sub` of the two ends | The largest and smallest `Duration`, not an overflow |
| `Until` of a past reading, `Since` and `Elapsed` of it | Not positive; not negative |
| `time.Now() + time.Second`, `time.Now() < time.Now()`, `a == a` for a `Timestamp` | Rejected: no operators on `Instant` or `Timestamp` |
| `time.Second * n` and `time.Sleep(n)` for an `int n` | Rejected: `Duration` and `int` do not mix |
| `time.Instant{reading: 1}` | Rejected: private field |
| A `Duration` where an `Instant` is expected, an `Instant` where a `Timestamp` is | Rejected: mismatched types |

## Calendar

| Scenario | Expected result |
| --- | --- |
| `Date(2000, 2, 29, 23, 59, 59, 999999999, UTCZone)` | `2000-02-29T23:59:59.999999999Z`, Unix `951868799`, Tuesday (`2`), day `60` of the year |
| `Date(1969, 12, 31, 23, 59, 59, 500000000, UTCZone)` | Unix `-1` with `500000000` ns, Wednesday, day `365` |
| `Date(1, 1, 1, ...)` and `Date(9999, 12, 31, 23, 59, 59, 999999999, ...)` | The two ends of the range, Monday and Friday |
| `Date` in `+05:45` and `-09:30` zones | Fields as given, offset kept in the text and in `Offset()`, Unix time shifted by the offset |
| February 29 of 1900, month 13, April 31, hour 24, minute 60, second 60, nanosecond `1000000000`, year 0 | Each an error naming the field; the result is `1970-01-01T00:00:00Z` |
| `Date(1, 1, 1, ...)` in `+05:45`, `Date(9999, 12, 31, 23, 59, 59, 0, ...)` in `-09:30` | `outside years 0001..9999 UTC` |
| `Unix(-1, 0)`; `Unix(0, -1)`; `Unix` one second past either end | `1969-12-31T23:59:59Z`; nanoseconds error; range error |
| The last moment shown in `+05:45`, the first in `-09:30` | `10000-01-01T05:44:59.999999999+05:45`, `0000-12-31T14:30:00-09:30` |
| `Add` past either end of the range | Stops at the end |
| `Add(Nanosecond)` to the last nanosecond of 2000-02-29; `Add(-1500 * Millisecond)` to 1969-12-31T23:59:59.5Z | `2000-03-01T00:00:00Z`; `1969-12-31T23:59:58Z` |
| `Sub` between those two, between the two ends of the range | `264408h0m0.499999999s`; the largest and smallest `Duration` |
| `Before`, `After`, `Equal` across zones | Compare moments, not shown fields |
| `Offset(24 * Hour)`, `Offset(30 * Second)`, `Offset(-23h59m)` | Error, error, accepted |

## RFC 3339

| Scenario | Expected result |
| --- | --- |
| `2026-10-30T12:34:56Z` and lowercase `t`/`z` | Accepted; written back with `T` and `Z` |
| A fraction of one, three, or nine digits; offsets `+07:00`, `-09:30`, `+00:00` | Accepted; `RFC3339Nano` writes the fraction without trailing zeros and `Z` for a zero offset; `UTC()` shifts by the offset |
| `0001-01-01T00:00:00Z`, `9999-12-31T23:59:59.999999999Z`, `2024-02-29T00:00:00Z` | Accepted |
| `0001-01-01T00:00:00+00:01`, `9999-12-31T23:59:59-00:01` | `outside years 0001..9999 UTC` |
| February 29 of 2023, second 60, hour 24, year 0000 | The field's error |
| A space instead of `T`, no zone, `+24:00`, `+07:60`, `+0700`, `.` without digits, ten fraction digits, trailing text, a leading `+`, a one-digit month, the empty text | `not an RFC 3339 timestamp` |
| Any rejection | The message is `time.ParseRFC3339: parsing "<text>": <reason>` with the text quoted |

## Time zones

| Scenario | Expected result |
| --- | --- |
| `Local()` one second before and at each 2026 New York switch, under `TZ=EST5EDT,M3.2.0,M11.1.0` | `01:59:59-05:00`, `03:00:00-04:00`; `01:59:59-04:00`, `01:00:00-05:00`; `UTC()` gives back the same moment |
| `Local().Add(Second)` across the November switch, against `Add(Second).Local()` | `02:00:00-04:00` (the offset stays fixed) and `01:00:00-05:00` |
| `http.FormatDate` of the epoch, 2000-02-29, both ends of the range, a time shown in `+07:00` | `Thu, 01 Jan 1970 00:00:00 GMT`, `Tue, 29 Feb 2000 00:00:00 GMT`, `Mon, 01 Jan 0001 00:00:00 GMT`, `Fri, 31 Dec 9999 23:59:59 GMT` (`codegen/http.rs`) |

## Timers and tickers

| Scenario | Expected result |
| --- | --- |
| `NewTimer(20 * Millisecond)` | Fires after at least 20 ms; then `Stop` reports `false` |
| `Reset(Hour)` then `Stop` | `Reset` reports `false`, `Stop` reports `true`; nothing arrives |
| A timer that fired unread, then `Reset(Hour)` | The old value is gone from `C()` |
| `NewTimer(0)`, `NewTimer(-Second)` | Fire at once |
| `NewTicker(10 * Millisecond)` received three times | Three values, at least 25 ms in all |
| `Stop` on a ticker, then `Reset(5 * Millisecond)` | Nothing arrives after `Stop`; values come again after `Reset` |
| `Reset(0)` on a ticker, `NewTicker(0)` | Errors; the failed ticker never fires |
| 1000 one-hour timers created and dropped | The program finishes at once, with no leak |
| Dropped and stopped one-hour timers and tickers, then a receive that can never finish | Reported as a deadlock at once: no task still waits for a dropped alarm |
