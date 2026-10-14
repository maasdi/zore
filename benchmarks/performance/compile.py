import hashlib
import json
import os
from pathlib import Path
import re
import time

from common import BenchmarkError, ROOT, command


def build(compiler, source, directory, cache, capture=None):
    directory.mkdir(parents=True, exist_ok=True)
    environment = {**os.environ, "ZORE_CACHE_DIR": str(cache)}
    if capture:
        environment.update(
            {
                "ZORE_CC": str(Path(__file__).with_name("cc.py")),
                "ZORE_BENCH_CC": os.environ.get("ZORE_CC", "clang"),
                "ZORE_BENCH_CAPTURE": str(capture),
            }
        )
    started = time.perf_counter()
    command([compiler, "build", source], directory, environment)
    return directory / source.stem, time.perf_counter() - started


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
            build(executable, source, workspace, workspace / "warm-cache")
        for repetition in range(1, arguments.repetitions + 1):
            order = variants if repetition % 2 else list(reversed(variants))
            for variant, executable, revision in order:
                workspace = directory / name / variant
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
                    "check_seconds": checked,
                    "warm_build_seconds": warm,
                    "clang": json.loads(capture.with_suffix(".json").read_text()),
                    "ir_bytes": capture.with_suffix(".ll").stat().st_size,
                }
                if not arguments.warm_builds_only:
                    _, cold = build(
                        executable,
                        source,
                        workspace,
                        workspace / f"cold-cache-{repetition}",
                    )
                    measurement["cold_runtime_cache_build_seconds"] = cold
                measurements.append(measurement)
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
