from concurrent.futures import ThreadPoolExecutor
import http.client
import json
import threading
import time

from common import BenchmarkError, finish, line, percentile, settle, start, suspended

CASES = {
    "get": ("GET", "/users/1", None, {}, 200, {"id": 1, "name": "Ada Lovelace"}),
    "create": (
        "POST",
        "/users",
        b'{"name":"Ada Lovelace"}',
        {"Content-Type": "application/json"},
        201,
        {"id": 1, "name": "Ada Lovelace"},
    ),
    "invalid_json": (
        "POST",
        "/users",
        b'{"name":"Ada","unknown":true}',
        {"Content-Type": "application/json"},
        400,
        {"error": "invalid name"},
    ),
    "malformed_json": (
        "POST",
        "/users",
        b'{"name":',
        {"Content-Type": "application/json"},
        400,
        {"error": "invalid name"},
    ),
    "media_type": (
        "POST",
        "/users",
        b"",
        {},
        415,
        {"error": "expected application/json"},
    ),
    "not_found": ("GET", "/users/2", None, {}, 404, {"error": "user not found"}),
    "handler_error": ("GET", "/failure", None, {}, 500, None),
}


def request(connection, case):
    method, path, body, headers, expected_status, expected = CASES[case]
    opened = connection.sock is None
    started = time.perf_counter_ns()
    connection.request(method, path, body, headers)
    response = connection.getresponse()
    data = response.read()
    ended = time.perf_counter_ns()
    if response.status != expected_status:
        raise BenchmarkError(
            f"{case}: status {response.status}, expected {expected_status}"
        )
    if expected is not None:
        if (
            response.getheader("Content-Type") != "application/json"
            or json.loads(data) != expected
        ):
            raise BenchmarkError(f"{case}: invalid JSON response {data!r}")
    elif data or b"private detail" in data:
        raise BenchmarkError(f"{case}: unexpected handler-error body {data!r}")
    return {
        "started_ns": started,
        "ended_ns": ended,
        "latency_ns": ended - started,
        "response_bytes": len(data),
        "opened_connection": opened,
    }


def batch(address, case, concurrency, count, timeout):
    host, port = address.rsplit(":", 1)
    barrier = threading.Barrier(concurrency)
    failed = threading.Event()

    def worker(index):
        connection = http.client.HTTPConnection(host, int(port), timeout=timeout)
        try:
            request(connection, case)
            barrier.wait(timeout=timeout)
            samples = []
            for _ in range(index, count, concurrency):
                if failed.is_set():
                    raise BenchmarkError("another request worker failed")
                samples.append(request(connection, case))
            return samples
        except BaseException:
            failed.set()
            barrier.abort()
            raise
        finally:
            connection.close()

    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        futures = [pool.submit(worker, index) for index in range(concurrency)]
        samples = [sample for future in futures for sample in future.result()]
    elapsed = (
        max(s["ended_ns"] for s in samples) - min(s["started_ns"] for s in samples)
    ) / 1e9
    latencies = [s["latency_ns"] for s in samples]
    return {
        "elapsed_seconds": elapsed,
        "requests_per_second": count / elapsed,
        "p50_latency_ns": percentile(latencies, 50),
        "p95_latency_ns": percentile(latencies, 95),
        "p99_latency_ns": percentile(latencies, 99),
        "opened_connections": sum(s["opened_connection"] for s in samples),
        "raw_requests": samples,
    }


def measure_rest(executable, arguments):
    results = []
    for concurrency in arguments.concurrency:
        process = start(executable)
        try:
            ready = line(process, arguments.timeout)
            if not ready.startswith("READY 127.0.0.1:"):
                raise BenchmarkError(f"REST ready protocol: {ready}")
            address = ready.removeprefix("READY ")
            for case in CASES:
                batch(address, case, concurrency, concurrency, arguments.timeout)
                before = settle(process, arguments.timeout)
                batches = []
                for repetition in range(1, arguments.repetitions + 1):
                    measured = batch(
                        address,
                        case,
                        concurrency,
                        arguments.requests,
                        arguments.timeout,
                    )
                    measured.update(
                        {
                            "repetition": repetition,
                            "after": settle(process, arguments.timeout),
                        }
                    )
                    batches.append(measured)
                results.append(
                    {
                        "case": case,
                        "concurrency": concurrency,
                        "before": before,
                        "batches": batches,
                    }
                )
            finish(process, "stop", "DONE\n", arguments.timeout)
        finally:
            suspended.terminate(process)
    return results
