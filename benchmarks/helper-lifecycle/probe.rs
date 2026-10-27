use std::io::{self, BufRead, Write};

use zore_runtime::scheduler::test_support::{self, HelperBurst};

fn snapshot() {
    println!("{}", test_support::snapshot());
    io::stdout().flush().unwrap();
}

fn main() {
    test_support::install_pool(2);
    let mut burst: Option<HelperBurst> = None;
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
                burst = Some(started);
            }
            Some("release") => {
                let released = burst.take().expect("a helper burst is active");
                released.release();
                released.wait_completed();
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
