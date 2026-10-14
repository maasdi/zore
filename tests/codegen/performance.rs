use super::{run_with_input, stderr, stdout};

#[test]
fn json_benchmark_batches_validate_values_and_clean_success_and_error_paths() {
    for (mode, multiplier) in [("encode", 60), ("decode", 7), ("reject", 1)] {
        let input = format!("{mode}\n1\n3\n0\n");
        let output = run_with_input(
            include_str!("../../benchmarks/performance/json/workload.ore"),
            input.as_bytes(),
        );
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(output.stderr.is_empty(), "{}", stderr(&output));
        let text = stdout(&output);
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(lines[0], "READY");
        for (line, count) in lines[1..].iter().zip([1, 3]) {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 4, "{line}");
            assert_eq!(fields[0], "DONE");
            assert_eq!(fields[1].parse::<usize>().unwrap(), count);
            fields[2].parse::<u64>().unwrap();
            assert_eq!(fields[3].parse::<usize>().unwrap(), count * multiplier);
        }
    }
}

#[test]
fn suspended_request_benchmark_payloads_survive_borrowed_child_calls_and_are_destroyed() {
    let output = run_with_input(
        include_str!("../../benchmarks/performance/suspension/workload.ore"),
        b"16\nrelease\n",
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "READY 16\nDONE 16\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}
