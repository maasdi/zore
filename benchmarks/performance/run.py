#!/usr/bin/env python3
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

from common import BenchmarkError, ROOT, command, suspended
from compile import build, frame_layouts, measure_compile, source_identity
from json_batches import measure_json
from legacy import measure_legacy
from rest_requests import measure_rest

HERE = Path(__file__).resolve().parent
WORKLOADS = {
    "hello": ROOT / "examples/hello/main.ore",
    "json": HERE / "json/workload.ore",
    "rest": HERE / "rest/workload.ore",
    "rest_sample": ROOT / "examples/rest-api/main.ore",
    "suspension": HERE / "suspension/workload.ore",
}
FRAME_WORKLOADS = {
    "channel": ROOT / "benchmarks/suspended-tasks/workload.ore",
    "dead_array": ROOT / "benchmarks/frame-storage/workload.ore",
    "loop": ROOT / "benchmarks/frame-storage/loop/workload.ore",
    "reuse": ROOT / "benchmarks/frame-storage/reuse/workload.ore",
    "request_payload": WORKLOADS["suspension"],
}


def metadata(arguments):
    compiler = arguments.compiler
    result = {
        "schema_version": 1,
        "runner_commit": command(["git", "rev-parse", "HEAD"]).stdout.strip(),
        "working_tree_dirty": bool(command(["git", "status", "--porcelain"]).stdout),
        "compiler_revision": arguments.compiler_revision or "unverified compiler",
        "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
        "toolchain": {
            "zore": command([compiler, "--version"]).stdout.strip(),
            "rustc": command(
                [os.environ.get("ZORE_RUSTC", "rustc"), "--version"]
            ).stdout.strip(),
            "clang": command(
                [os.environ.get("ZORE_CC", "clang"), "--version"]
            ).stdout.splitlines()[0],
            "python": platform.python_version(),
        },
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "cpu_model": suspended.cpu_model(),
            "logical_core_count": os.cpu_count(),
            "cpu_affinity_count": len(os.sched_getaffinity(0)),
            "load_average": os.getloadavg(),
            "cgroup_limits": {
                name: Path(f"/sys/fs/cgroup/{name}").read_text().strip()
                for name in ["cpu.max", "memory.max"]
                if Path(f"/sys/fs/cgroup/{name}").is_file()
            },
        },
        "configuration": {
            key: str(value) if isinstance(value, Path) else value
            for key, value in vars(arguments).items()
        },
        "sources": {
            key: source_identity(value)
            for key, value in {**WORKLOADS, **FRAME_WORKLOADS}.items()
        },
        "runner_sources": {
            path.name: hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(HERE.glob("*.py"))
        },
        "existing_measurement_sources": {
            str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in [
                ROOT / "benchmarks/suspended-tasks/run.py",
                ROOT / "benchmarks/cooperative-budget/run.py",
                ROOT / "benchmarks/frame-storage/scaling/main.rs",
                *sorted(
                    (ROOT / "benchmarks/cooperative-budget").glob("*/workload.ore")
                ),
            ]
        },
    }
    if getattr(arguments, "baseline_compiler", None):
        result["baseline_compiler"] = {
            "revision": arguments.baseline_revision or "unverified compiler",
            "sha256": hashlib.sha256(
                arguments.baseline_compiler.read_bytes()
            ).hexdigest(),
            "version": command(
                [arguments.baseline_compiler, "--version"]
            ).stdout.strip(),
        }
    return result


def measure_frames(directory, arguments):
    results = []
    for name, source in FRAME_WORKLOADS.items():
        workspace = directory / name
        capture = workspace / "capture"
        build(
            arguments.compiler, source, workspace, directory / "runtime-cache", capture
        )
        ir = capture.with_suffix(".ll").read_text(encoding="utf-8")
        output = workspace / "waiting.json"
        command(
            [
                sys.executable,
                ROOT / "benchmarks/suspended-tasks/run.py",
                "--source",
                source,
                "--compiler",
                arguments.compiler,
                "--compiler-revision",
                arguments.compiler_revision or "unverified compiler",
                "--counts",
                *arguments.counts,
                "--repetitions",
                arguments.repetitions,
                "--timeout",
                arguments.timeout,
                "--output",
                output,
            ],
            environment={
                **os.environ,
                "ZORE_CHECK_LEAKS": "1",
                "ZORE_CACHE_DIR": str(directory / "runtime-cache"),
            },
            timeout=300
            + len(arguments.counts) * arguments.repetitions * arguments.timeout,
        )
        results.append(
            {
                "workload": name,
                "frames": frame_layouts(ir, workspace),
                "waiting": json.loads(output.read_text()),
            }
        )
    return results


def main():
    parser = argparse.ArgumentParser(
        description="Opt-in Linux compiler, REST/JSON and frame baselines."
    )
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/release/zore")
    parser.add_argument("--compiler-revision")
    parser.add_argument("--baseline-compiler", type=Path)
    parser.add_argument("--baseline-revision")
    parser.add_argument("--warm-builds-only", action="store_true")
    parser.add_argument(
        "--suites",
        nargs="+",
        choices=["compile", "json", "rest", "frames", "legacy"],
        default=["compile", "json", "rest", "frames", "legacy"],
    )
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--iterations", type=int, default=10000)
    parser.add_argument("--requests", type=int, default=200)
    parser.add_argument("--concurrency", nargs="+", type=int, default=[1, 4])
    parser.add_argument("--counts", nargs="+", type=int, default=[100, 1000])
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    if (
        min(
            arguments.repetitions,
            arguments.iterations,
            arguments.requests,
            *arguments.concurrency,
            *arguments.counts,
        )
        < 1
        or not math.isfinite(arguments.timeout)
        or arguments.timeout <= 0
    ):
        parser.error("counts and finite timeout must be positive")
    if arguments.requests < max(arguments.concurrency):
        parser.error("requests must be at least the largest concurrency")
    if arguments.baseline_compiler and "compile" not in arguments.suites:
        parser.error("baseline compiler requires the compile suite")
    if arguments.baseline_revision and not arguments.baseline_compiler:
        parser.error("baseline revision requires a baseline compiler")
    if sys.platform != "linux":
        parser.error("memory/resource sampling requires Linux /proc")
    arguments.compiler = arguments.compiler.resolve()
    if arguments.baseline_compiler:
        arguments.baseline_compiler = arguments.baseline_compiler.resolve()
    try:
        result = metadata(arguments)
        with tempfile.TemporaryDirectory(prefix="zore-performance-") as temporary:
            directory = Path(temporary)
            if "compile" in arguments.suites:
                print(
                    "Measuring compiler check and warm/cold builds",
                    file=sys.stderr,
                    flush=True,
                )
                result["compile"] = measure_compile(
                    arguments.compiler, WORKLOADS, directory / "compile", arguments
                )
            for suite, measure in [("json", measure_json), ("rest", measure_rest)]:
                if suite in arguments.suites:
                    print(f"Measuring {suite}", file=sys.stderr, flush=True)
                    executable, _ = build(
                        arguments.compiler,
                        WORKLOADS[suite],
                        directory / suite,
                        directory / "runtime-cache",
                    )
                    result[suite] = measure(executable, arguments)
            if "frames" in arguments.suites:
                print(
                    "Measuring pinned layouts and settled waiting memory",
                    file=sys.stderr,
                    flush=True,
                )
                result["frames"] = measure_frames(directory / "frames", arguments)
            if "legacy" in arguments.suites:
                print(
                    "Measuring existing cooperative and analysis workloads",
                    file=sys.stderr,
                    flush=True,
                )
                result["legacy"] = measure_legacy(arguments)
        arguments.output.write_text(
            json.dumps(result, indent=2) + "\n", encoding="utf-8"
        )
    except (
        BenchmarkError,
        OSError,
        ValueError,
        subprocess.TimeoutExpired,
        KeyboardInterrupt,
    ) as error:
        print(f"performance benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
