from common import BenchmarkError, finish, line, send, settle, start, suspended


def measure_json(executable, arguments):
    results = []
    for mode, multiplier in [("encode", 60), ("decode", 7), ("reject", 1)]:
        process = start(executable)
        try:
            send(process, mode)
            if line(process, arguments.timeout) != "READY":
                raise BenchmarkError("JSON ready protocol")
            send(process, "1")
            warmup = line(process, arguments.timeout)
            fields = warmup.split()
            if (
                len(fields) != 4
                or fields[:2] != ["DONE", "1"]
                or int(fields[3]) != multiplier
            ):
                raise BenchmarkError(f"JSON warmup result: {warmup}")
            before = settle(process, arguments.timeout)
            batches = []
            for repetition in range(1, arguments.repetitions + 1):
                send(process, str(arguments.iterations))
                response = line(process, arguments.timeout).split()
                if len(response) != 4 or response[:2] != [
                    "DONE",
                    str(arguments.iterations),
                ]:
                    raise BenchmarkError(f"JSON batch protocol: {response}")
                elapsed, checksum = map(int, response[2:])
                if elapsed <= 0 or checksum != arguments.iterations * multiplier:
                    raise BenchmarkError(f"JSON batch result: {response}")
                batches.append(
                    {
                        "repetition": repetition,
                        "iterations": arguments.iterations,
                        "elapsed_ns": elapsed,
                        "checksum": checksum,
                        "operations_per_second": arguments.iterations * 1e9 / elapsed,
                        "mean_ns_per_operation": elapsed / arguments.iterations,
                        "after": settle(process, arguments.timeout),
                    }
                )
            finish(process, "0", "", arguments.timeout)
            results.append(
                {
                    "mode": mode,
                    "warmup": warmup,
                    "before": before,
                    "batches": batches,
                    "leak_check_exit": 0,
                }
            )
        finally:
            suspended.terminate(process)
    return results
