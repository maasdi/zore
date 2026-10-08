#!/usr/bin/env python3

import argparse
import json
import math
import os
from pathlib import Path
import platform
import selectors
import statistics
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(__file__).with_name("workload.ore")


class BenchmarkError(RuntimeError):
    pass


def command_output(command):
    try:
        return subprocess.run(
            command,
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as error:
        return f"unavailable: {error}"


def run_build(command, cwd):
    try:
        subprocess.run(command, cwd=cwd, check=True, capture_output=True, text=True)
    except OSError as error:
        raise BenchmarkError(f"could not run {' '.join(map(str, command))}: {error}") from error
    except subprocess.CalledProcessError as error:
        details = error.stderr.strip() or error.stdout.strip()
        raise BenchmarkError(
            f"build command failed ({' '.join(map(str, command))}):\n{details}"
        ) from error


def build_executable(directory, skip_compiler_build):
    if not skip_compiler_build:
        run_build(["cargo", "build", "--release", "--locked"], ROOT)
    compiler = ROOT / "target" / "release" / "zore"
    if not compiler.is_file():
        raise BenchmarkError(f"compiler does not exist: {compiler}")
    run_build([str(compiler), "build", str(SOURCE)], directory)
    executable = directory / SOURCE.stem
    if not executable.is_file():
        raise BenchmarkError(f"benchmark executable was not produced: {executable}")
    return executable


def read_status(pid):
    values = {}
    try:
        lines = Path(f"/proc/{pid}/status").read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise BenchmarkError(f"cannot read /proc/{pid}/status: {error}") from error
    for line in lines:
        name, separator, value = line.partition(":")
        if separator and name in {"VmRSS", "Threads"}:
            values[name] = int(value.split()[0])
    if set(values) != {"VmRSS", "Threads"}:
        raise BenchmarkError(f"/proc/{pid}/status lacks VmRSS or Threads")
    return values["VmRSS"], values["Threads"]


def wait_for_line(process, expected, deadline):
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise BenchmarkError(f"timed out waiting for {expected!r}")
            if not selector.select(min(remaining, 0.1)):
                if process.poll() is not None:
                    raise BenchmarkError(
                        f"process exited with {process.returncode} before {expected!r}"
                    )
                continue
            line = process.stdout.readline()
            if not line:
                raise BenchmarkError(f"process closed stdout before {expected!r}")
            line = line.rstrip("\r\n")
            if line != expected:
                raise BenchmarkError(f"expected {expected!r}, received {line!r}")
            return line
    finally:
        selector.close()


def terminate(process):
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def stable_window(samples, size, rss_percent, rss_floor_kib, minimum_seconds):
    if len(samples) < size:
        return None
    window = samples[-size:]
    if window[-1]["elapsed_ms"] - window[0]["elapsed_ms"] < minimum_seconds * 1000:
        return None
    threads = {sample["threads"] for sample in window}
    rss = [sample["rss_kib"] for sample in window]
    median = statistics.median(rss)
    tolerance = max(rss_floor_kib, median * rss_percent / 100)
    if len(threads) != 1 or max(rss) - min(rss) > tolerance:
        return None
    return window


def measure(executable, task_count, repetition, arguments):
    process = subprocess.Popen(
        [str(executable)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    deadline = time.monotonic() + arguments.timeout
    try:
        process.stdin.write(f"{task_count}\n")
        process.stdin.flush()
        ready = wait_for_line(process, f"READY {task_count}", deadline)

        effective_window = max(
            arguments.stable_samples,
            math.ceil(arguments.minimum_stable_seconds / arguments.interval) + 1,
        )
        started = time.monotonic()
        samples = []
        settled = None
        while settled is None:
            if time.monotonic() >= deadline:
                raise BenchmarkError("timed out before RSS and thread count stabilized")
            rss_kib, threads = read_status(process.pid)
            samples.append(
                {
                    "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
                    "rss_kib": rss_kib,
                    "threads": threads,
                }
            )
            settled = stable_window(
                samples,
                effective_window,
                arguments.rss_tolerance_percent,
                arguments.rss_tolerance_kib,
                arguments.minimum_stable_seconds,
            )
            if settled is None:
                time.sleep(arguments.interval)

        process.stdin.write("release\n")
        process.stdin.flush()
        process.stdin.close()
        process.stdin = None
        remaining = max(0.001, deadline - time.monotonic())
        stdout, stderr = process.communicate(timeout=remaining)
        if process.returncode != 0:
            raise BenchmarkError(
                f"benchmark exited with {process.returncode}: {stderr.strip()}"
            )
        lines = [line for line in stdout.splitlines() if line]
        done = f"DONE {task_count}"
        if lines != [done]:
            raise BenchmarkError(f"expected final line {done!r}, received {lines!r}")
        if stderr:
            raise BenchmarkError(f"benchmark wrote to stderr: {stderr.strip()}")

        return {
            "task_count": task_count,
            "repetition": repetition,
            "pid": process.pid,
            "ready_line": ready,
            "done_line": done,
            "sampled_peak_rss_kib": max(sample["rss_kib"] for sample in samples),
            "sampled_peak_threads": max(sample["threads"] for sample in samples),
            "settled_rss_kib": statistics.median(
                sample["rss_kib"] for sample in settled
            ),
            "settled_threads": settled[-1]["threads"],
            "raw_waiting_samples": samples,
        }
    except (BrokenPipeError, OSError, subprocess.TimeoutExpired) as error:
        raise BenchmarkError(f"benchmark process failed: {error}") from error
    finally:
        terminate(process)


def cpu_model():
    try:
        for line in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines():
            if line.startswith("model name"):
                return line.partition(":")[2].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def metadata(arguments, effective_window):
    status = command_output(["git", "status", "--porcelain"])
    clang = os.environ.get("ZORE_CC", "clang")
    rustc = os.environ.get("ZORE_RUSTC", "rustc")
    return {
        "schema_version": 1,
        "commit": command_output(["git", "rev-parse", "HEAD"]),
        "working_tree_dirty": bool(status),
        "toolchain": {
            "rustc": command_output([rustc, "--version"]),
            "clang": command_output([clang, "--version"]).splitlines()[0],
            "zore": command_output([str(ROOT / "target" / "release" / "zore"), "--version"]),
        },
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "cpu_model": cpu_model(),
            "logical_core_count": os.cpu_count(),
        },
        "configuration": {
            "task_counts": arguments.counts,
            "repetitions": arguments.repetitions,
            "sample_interval_seconds": arguments.interval,
            "timeout_seconds_per_run": arguments.timeout,
            "stability": {
                "requested_samples": arguments.stable_samples,
                "effective_samples": effective_window,
                "minimum_seconds": arguments.minimum_stable_seconds,
                "rss_tolerance_percent": arguments.rss_tolerance_percent,
                "rss_tolerance_floor_kib": arguments.rss_tolerance_kib,
                "thread_count_must_be_constant": True,
            },
            "measurement": "Linux /proc/<pid>/status sampled after READY and before release",
        },
    }


def parse_arguments():
    parser = argparse.ArgumentParser(
        description="Measure Linux RSS and threads while Zore async tasks are suspended."
    )
    parser.add_argument("--counts", nargs="+", type=int, default=[100, 1000, 5000])
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--interval", type=float, default=0.05)
    parser.add_argument("--stable-samples", type=int, default=6)
    parser.add_argument("--minimum-stable-seconds", type=float, default=0.25)
    parser.add_argument("--rss-tolerance-percent", type=float, default=1.0)
    parser.add_argument("--rss-tolerance-kib", type=int, default=64)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--skip-compiler-build", action="store_true")
    arguments = parser.parse_args()
    if (
        any(count < 1 for count in arguments.counts)
        or arguments.repetitions < 1
        or arguments.interval <= 0
        or arguments.stable_samples < 2
        or arguments.minimum_stable_seconds < 0
        or arguments.rss_tolerance_percent < 0
        or arguments.rss_tolerance_kib < 0
        or arguments.timeout <= 0
    ):
        parser.error("counts and timing values must be positive; tolerances cannot be negative")
    return arguments


def main():
    if sys.platform != "linux":
        print("suspended-tasks benchmark requires Linux /proc", file=sys.stderr)
        return 2
    arguments = parse_arguments()
    effective_window = max(
        arguments.stable_samples,
        math.ceil(arguments.minimum_stable_seconds / arguments.interval) + 1,
    )
    try:
        with tempfile.TemporaryDirectory(prefix="zore-suspended-tasks-") as temporary:
            executable = build_executable(
                Path(temporary), arguments.skip_compiler_build
            )
            results = metadata(arguments, effective_window)
            results["runs"] = []
            for task_count in arguments.counts:
                for repetition in range(1, arguments.repetitions + 1):
                    results["runs"].append(
                        measure(executable, task_count, repetition, arguments)
                    )
    except (BenchmarkError, KeyboardInterrupt) as error:
        print(f"suspended-tasks benchmark failed: {error}", file=sys.stderr)
        return 1

    rendered = json.dumps(results, indent=2) + "\n"
    if arguments.output:
        arguments.output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
