use std::path::Path;
use std::time::Instant;

use zore::driver::build::{Stage, native_ir};
use zore::driver::project::{Disk, load_project};
use zore::source::SourceMap;

fn main() {
    let source = std::env::args_os().nth(1).expect("source path required");
    let mut sources = SourceMap::new();
    let start = Instant::now();
    let project = load_project(&mut sources, &Disk, Path::new(&source))
        .unwrap_or_else(|_| panic!("project failed to load"));
    let mut fields = vec![format!(
        "\"load_seconds\":{}",
        start.elapsed().as_secs_f64()
    )];
    let mut start = Instant::now();
    let native = native_ir(&project, &sources, &mut |stage| {
        let elapsed = start.elapsed().as_secs_f64();
        match stage {
            Stage::Checked => fields.push(format!("\"check_seconds\":{elapsed}")),
            Stage::Lowered => fields.push(format!("\"lower_seconds\":{elapsed}")),
            Stage::Emitted(ir) => {
                fields.push(format!("\"emit_seconds\":{elapsed}"));
                fields.push(format!("\"ir_bytes\":{}", ir.len()));
            }
        }
        start = Instant::now();
    })
    .unwrap_or_else(|error| panic!("{error:?}"));
    fields.push(format!(
        "\"prune_seconds\":{}",
        start.elapsed().as_secs_f64()
    ));
    fields.push(format!("\"native_ir_bytes\":{}", native.len()));
    println!("{{{}}}", fields.join(","));
}
