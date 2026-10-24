from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import time

from common import BenchmarkError, ROOT, command


def cache_snapshot(cache):
    return {
        str(path.relative_to(cache)): {
            "bytes": path.stat().st_size,
            "mtime_ns": path.stat().st_mtime_ns,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
        for path in sorted(cache.rglob("*.rlib"))
    }


def validate_runtime_cache(profile, cold):
    before, after = profile["cache_before"], profile["cache_after"]
    compiled = any(
        item["tool"] == "rustc" and item["role"] == "runtime"
        for item in profile["commands"]
    )
    if not after or (cold and (before or not compiled)) or (
        not cold and (not before or before != after or compiled)
    ):
        raise BenchmarkError("runtime artifact cache was not populated/reused as expected")


def build(compiler, source, directory, cache, capture=None):
    directory.mkdir(parents=True, exist_ok=True)
    environment = {**os.environ, "ZORE_CACHE_DIR": str(cache)}
    if capture:
        environment.update(
            {
                "ZORE_CC": str(Path(__file__).with_name("cc.py")),
                "ZORE_BENCH_CC": os.environ.get("ZORE_CC", "clang"),
                "ZORE_BENCH_CAPTURE": str(capture),
                "ZORE_RUSTC": str(Path(__file__).with_name("rustc.py")),
                "ZORE_BENCH_RUSTC": os.environ.get("ZORE_RUSTC", "rustc"),
            }
        )
    cache.mkdir(parents=True, exist_ok=True)
    probe = cache / ".write-probe"
    probe.write_text("writable", encoding="utf-8")
    probe.unlink()
    before = cache_snapshot(cache) if capture else None
    started = time.perf_counter()
    command([compiler, "build", source], directory, environment)
    elapsed = time.perf_counter() - started
    if capture:
        commands = [
            json.loads(line)
            for line in capture.with_suffix(".commands.jsonl").read_text().splitlines()
        ]

        def tool_seconds(tool, role):
            return sum(
                item["seconds"]
                for item in commands
                if item["tool"] == tool and item["role"] == role
            )

        object_started = next(
            item["started"]
            for item in commands
            if item["tool"] == "clang" and item["role"] == "object"
        )
        profile = {
            "cache_writable": True,
            "build_seconds": elapsed,
            "cache_before": before,
            "cache_after": cache_snapshot(cache),
            "commands": commands,
            "frontend_and_emit_seconds": object_started - started,
            "clang_object_seconds": tool_seconds("clang", "object"),
            "native_link_seconds": tool_seconds("clang", "link"),
            "runtime_build_seconds": tool_seconds("rustc", "runtime"),
            "rustc_link_seconds": tool_seconds("rustc", "link"),
        }
        capture.with_suffix(".build.json").write_text(
            json.dumps(profile) + "\n", encoding="utf-8"
        )
    return directory / source.stem, elapsed


@contextmanager
def compile_lease(arguments):
    if arguments.compile_lock_file is None:
        yield
        return
    with arguments.compile_lock_file.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


def measure_compile(compiler, workloads, directory, arguments):
    measurements = []
    variants = [("candidate", compiler, arguments.compiler_revision)]
    if arguments.baseline_compiler:
        variants.insert(
            0, ("baseline", arguments.baseline_compiler, arguments.baseline_revision)
        )
    for name, source in workloads.items():
        for variant, executable, _ in variants:
            workspace = directory / name / variant
            with compile_lease(arguments):
                build(executable, source, workspace, workspace / "warm-cache")
        for repetition in range(1, arguments.repetitions + 1):
            order = variants if repetition % 2 else list(reversed(variants))
            for variant, executable, revision in order:
                workspace = directory / name / variant
                with compile_lease(arguments):
                    started = time.perf_counter()
                    command([executable, "check", source])
                    checked = time.perf_counter() - started
                    capture = workspace / f"clang-{repetition}"
                    _, warm = build(
                        executable, source, workspace, workspace / "warm-cache", capture
                    )
                    measurement = {
                        "variant": variant,
                        "compiler_revision": revision or "unverified compiler",
                        "workload": name,
                        "repetition": repetition,
                        "variant_order": [item[0] for item in order],
                        "check_seconds": checked,
                        "warm_build_seconds": warm,
                        "clang": json.loads(capture.with_suffix(".json").read_text()),
                        "ir_bytes": capture.with_suffix(".ll").stat().st_size,
                        "warm_profile": json.loads(
                            capture.with_suffix(".build.json").read_text()
                        ),
                    }
                    validate_runtime_cache(measurement["warm_profile"], cold=False)
                    profiler = (
                        arguments.baseline_stage_profiler
                        if variant == "baseline"
                        else arguments.stage_profiler
                    )
                    if profiler:
                        measurement["stages"] = json.loads(
                            command([profiler, source]).stdout
                        )
                    if not arguments.warm_builds_only:
                        cold_capture = workspace / f"cold-clang-{repetition}"
                        _, cold = build(
                            executable,
                            source,
                            workspace,
                            workspace / f"cold-cache-{repetition}",
                            cold_capture,
                        )
                        measurement["cold_runtime_cache_build_seconds"] = cold
                        measurement["cold_profile"] = json.loads(
                            cold_capture.with_suffix(".build.json").read_text()
                        )
                        validate_runtime_cache(measurement["cold_profile"], cold=True)
                    measurements.append(measurement)
                    (workspace / f"measurement-{repetition}.json").write_text(
                        json.dumps(measurement, indent=2) + "\n", encoding="utf-8"
                    )
    return measurements


def frame_layouts(ir, directory):
    frames = re.findall(r'^(%"[^"\n]+\.AsyncFrame\.\d+") = type (.+)$', ir, re.M)
    probes = []
    for index, (name, _) in enumerate(frames):
        probes.append(
            f"define i64 @zore_bench_size_{index}() {{\n"
            f"  ret i64 ptrtoint (ptr getelementptr ({name}, ptr null, i64 1) to i64)\n}}\n"
        )
    if not frames:
        raise BenchmarkError("no async frame types in captured IR")
    source = directory / "frame-probes.ll"
    source.write_text(ir + "\n" + "\n".join(probes), encoding="utf-8")
    optimized = command(
        [
            os.environ.get("ZORE_CC", "clang"),
            "-O2",
            "-S",
            "-emit-llvm",
            "-x",
            "ir",
            source,
            "-o",
            "-",
        ]
    ).stdout
    constructors = re.findall(
        r'define private ptr (@"[^"\n]+\$async\$new")\([^\n]*\) \{(.*?)^}',
        ir,
        re.M | re.S,
    )
    result = []
    for index, (name, fields) in enumerate(frames):
        match = re.search(
            rf"define[^\n]*@zore_bench_size_{index}\(\)[^{{]*{{[^}}]*ret i64 (\d+)",
            optimized,
        )
        if not match:
            raise BenchmarkError(f"clang did not fold frame size for {name}")
        functions = [function for function, body in constructors if name in body]
        result.append(
            {
                "llvm_type": name,
                "constructors": functions,
                "fields": fields,
                "bytes": int(match[1]),
            }
        )
    return result


def source_identity(source):
    return {
        "path": str(source.relative_to(ROOT)),
        "package_files": {
            str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(source.parent.glob("*.ore"))
            if not path.name.endswith("_test.ore")
        },
    }
