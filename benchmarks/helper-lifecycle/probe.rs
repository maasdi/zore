use std::io::{self, BufRead, Write};
use std::time::Instant;

use zore_runtime::scheduler::test_support::{self, HelperBurst};

fn snapshot() {
    println!("{}", test_support::snapshot());
    io::stdout().flush().unwrap();
}

fn main() {
    let epoch = Instant::now();
    test_support::install_pool(2);
    let mut burst: Option<(usize, HelperBurst)> = None;
    snapshot();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let mut command = line.split_whitespace();
        match command.next() {
            Some("start") => {
                assert!(burst.is_none());
                let count = command.next().unwrap().parse().unwrap();
                let started = HelperBurst::start(count);
                started.wait_active();
                burst = Some((count, started));
            }
            Some("release") => {
                let (_, released) = burst.take().expect("a helper burst is active");
                released.release();
                released.wait_completed();
            }
            Some("restart") => {
                assert!(command.next().is_none());
                let (count, released) = burst.take().expect("a helper burst is active");
                let released_ns = epoch.elapsed().as_nanos();
                released.release();
                released.wait_completed();
                let completed_ns = epoch.elapsed().as_nanos();
                let submitted_ns = epoch.elapsed().as_nanos();
                let restarted = HelperBurst::start(count);
                let active_ns = epoch.elapsed().as_nanos();
                burst = Some((count, restarted));
                println!(
                    "{{\"released_ns\":{released_ns},\"completed_ns\":{completed_ns},\"submitted_ns\":{submitted_ns},\"active_ns\":{active_ns},\"runtime\":{}}}",
                    test_support::snapshot()
                );
                io::stdout().flush().unwrap();
                continue;
            }
            Some("nudge") => {
                assert!(burst.is_none());
                test_support::wake_idle_helpers();
            }
            Some("snapshot") => {}
            Some("stop") => {
                assert!(burst.is_none());
                zore_runtime::finish();
                return;
            }
            _ => panic!("unknown helper probe command"),
        }
        assert!(command.next().is_none());
        snapshot();
    }
    assert!(burst.is_none());
    zore_runtime::finish();
}
