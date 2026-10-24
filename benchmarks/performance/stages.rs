use std::path::Path;
use std::time::Instant;

use zore::driver::project::{Disk, load_project};
use zore::source::{Layered, SourceMap};
use zore::{async_lowering, codegen, driver, dropck, mir};

fn main() {
    let source = std::env::args_os().nth(1).expect("source path required");
    let mut sources = SourceMap::new();
    let start = Instant::now();
    let project = load_project(&mut sources, &Disk, Path::new(&source))
        .unwrap_or_else(|_| panic!("project failed to load"));
    let loaded = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let checked = driver::check::check_project(&project, &sources);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let package = checked.package.expect("checked package");
    let checked = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let mut program = mir::lower::lower(&package);
    dropck::insert(&package, &mut program);
    let machines = async_lowering::lower(&package, &program);
    let lowered = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let layered = Layered(&sources, project.std_sources());
    let ir = codegen::emit(&package, &program, &machines, &layered).expect("IR emission succeeds");
    let emitted = start.elapsed().as_secs_f64();
    println!(
        "{{\"load_seconds\":{loaded},\"check_seconds\":{checked},\"lower_seconds\":{lowered},\"emit_seconds\":{emitted},\"ir_bytes\":{}}}",
        ir.len()
    );
}
