import importlib.util
import math
import os
from pathlib import Path
import selectors
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "suspended_tasks", ROOT / "benchmarks/suspended-tasks/run.py"
)
suspended = importlib.util.module_from_spec(spec)
spec.loader.exec_module(suspended)
BenchmarkError = suspended.BenchmarkError


def command(args, directory=ROOT, environment=None, timeout=300):
    try:
        return subprocess.run(
            list(map(str, args)),
            cwd=directory,
            env=environment,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=True,
        )
    except (OSError, subprocess.TimeoutExpired, subprocess.CalledProcessError) as error:
        details = getattr(error, "stderr", "") or getattr(error, "stdout", "")
        raise BenchmarkError(f"command failed: {args}: {error}\n{details}") from error


def start(executable):
    return subprocess.Popen(
        [str(executable)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env={**os.environ, "ZORE_CHECK_LEAKS": "1"},
    )


def send(process, text):
    process.stdin.write(text + "\n")
    process.stdin.flush()


def line(process, timeout):
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        if not selector.select(timeout):
            raise BenchmarkError("child protocol timed out")
        result = process.stdout.readline().rstrip("\r\n")
        if not result:
            raise BenchmarkError("child exited before protocol response")
        return result


def finish(process, final_input, expected, timeout):
    send(process, final_input)
    process.stdin.close()
    process.stdin = None
    stdout, stderr = process.communicate(timeout=timeout)
    if process.returncode != 0 or stdout != expected or stderr:
        raise BenchmarkError(
            f"child exit={process.returncode}, stdout={stdout!r}, stderr={stderr!r}"
        )


def settle(process, timeout):
    started = time.monotonic()
    samples = []
    while time.monotonic() - started < timeout:
        rss, threads = suspended.read_status(process.pid)
        descriptors = list(Path(f"/proc/{process.pid}/fd").iterdir())
        sockets = 0
        for descriptor in descriptors:
            try:
                sockets += os.readlink(descriptor).startswith("socket:")
            except FileNotFoundError:
                pass
        samples.append(
            {
                "elapsed_ms": (time.monotonic() - started) * 1000,
                "rss_kib": rss,
                "threads": threads,
                "open_fds": len(descriptors),
                "socket_fds": sockets,
            }
        )
        window = suspended.stable_window(samples, 6, 1.0, 64, 0.25)
        if window and len({(s["open_fds"], s["socket_fds"]) for s in window}) == 1:
            return {
                "settled_rss_kib": statistics.median(s["rss_kib"] for s in window),
                "settled_threads": window[-1]["threads"],
                "settled_open_fds": window[-1]["open_fds"],
                "settled_socket_fds": window[-1]["socket_fds"],
                "raw_samples": samples,
            }
        time.sleep(0.05)
    raise BenchmarkError("process memory/resources did not settle")


def percentile(values, percent):
    return sorted(values)[max(0, math.ceil(len(values) * percent / 100) - 1)]
