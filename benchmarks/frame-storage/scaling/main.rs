use std::time::{Duration, Instant};

use zore::source::SourceMap;
use zore::{async_lowering, check, dropck, mir};

fn probe(locals: usize) -> String {
    let mut source = String::from(
        "package main\nasync func run(ch channel<int>) int {\nch.send(0)\nvar sum = 0\n",
    );
    for i in 0..locals {
        source.push_str(&format!("let v{i} = {i}\n"));
    }
    for i in 0..locals {
        source.push_str(&format!("sum += v{i}\n"));
    }
    source.push_str("return sum\n}\nfunc main() {}\n");
    source
}

fn main() {
    let mut args = std::env::args().skip(1);
    let repetitions: usize = match args.next() {
        Some(count) => count.parse().expect("repetitions must be a number"),
        None => 3,
    };
    let counts: Vec<usize> = {
        let given: Vec<usize> = args
            .map(|count| count.parse().expect("local counts must be numbers"))
            .collect();
        if given.is_empty() {
            vec![100, 200, 400, 800]
        } else {
            given
        }
    };
    println!("declared_locals mir_locals mir_blocks reuse_work median_lower_ms");
    for count in counts {
        let mut sources = SourceMap::new();
        let id = sources.add("scaling.ore", probe(count)).unwrap();
        let checked = check::check_file(sources.file(id).unwrap());
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let package = checked.package.unwrap();
        let mut program = mir::lower::lower(&package);
        dropck::insert(&package, &mut program);
        let body = program
            .bodies
            .iter()
            .find(|body| package.function(body.function).name == "run")
            .unwrap();
        let mut work = 0;
        let mut times: Vec<Duration> = (0..repetitions)
            .map(|_| {
                let start = Instant::now();
                let plan = async_lowering::lower(&package, &program);
                let elapsed = start.elapsed();
                work = plan.machines[&body.function].frame_reuse_work;
                elapsed
            })
            .collect();
        times.sort();
        println!(
            "{count} {} {} {work} {:.1}",
            body.locals.len(),
            body.blocks.len(),
            times[times.len() / 2].as_secs_f64() * 1000.0
        );
    }
}
