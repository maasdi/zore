#!/usr/bin/env python3
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys
import tempfile
import time

from common import (
    BenchmarkError,
    ROOT,
    command,
    finish,
    line,
    send,
    settle,
    start,
    suspended,
)
from compile import build
from rest_requests import CASES, batch
from run import metadata


def function_fragment(path, signature, indent):
    source = path.read_text(encoding="utf-8")
    pattern = rf"^{indent}{re.escape(signature)}[^\n]*\{{.*?^{indent}\}}"
    match = re.search(pattern, source, re.M | re.S)
    if not match:
        raise BenchmarkError(f"missing instrumentation function {signature}")
    return match[0] + "\n"


def runtime_sources(directory, revision):
    names = command(
        ["git", "ls-tree", "-r", "--name-only", revision, "runtime/src"]
    ).stdout.splitlines()
    if not names:
        raise BenchmarkError("runtime revision has no sources")
    directory.mkdir()
    for name in names:
        if name.endswith(".rs"):
            (directory / Path(name).name).write_text(
                command(["git", "show", f"{revision}:{name}"]).stdout, encoding="utf-8"
            )
    reactor_path = directory / "reactor.rs"
    reactor = reactor_path.read_text()
    current_reactor = ROOT / "runtime/src/reactor.rs"
    if "fn snapshot()" not in reactor:
        fragment = function_fragment(
            current_reactor, "pub(super) fn snapshot()", "    "
        )
        marker = "    fn serve(reactor:"
        if marker not in reactor:
            raise BenchmarkError("runtime reactor instrumentation anchor changed")
        reactor = reactor.replace(marker, fragment + "\n" + marker, 1)
        reactor += "\n" + function_fragment(
            current_reactor, "pub(super) fn snapshot()", ""
        )
        reactor_path.write_text(reactor)
    scheduler_path = directory / "scheduler.rs"
    scheduler = scheduler_path.read_text()
    current_scheduler = ROOT / "runtime/src/scheduler.rs"
    fragments = []
    for signature in ["pub fn wait_idle(", "pub fn snapshot()"]:
        if signature not in scheduler:
            fragments.append(function_fragment(current_scheduler, signature, "    "))
    if fragments:
        marker = "    pub fn install_pool("
        if marker not in scheduler:
            raise BenchmarkError("runtime scheduler instrumentation anchor changed")
        scheduler_path.write_text(
            scheduler.replace(marker, "\n".join(fragments) + "\n" + marker, 1)
        )
    return {
        path.name: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(directory.glob("*.rs"))
    }


def profiled_source():
    source = (ROOT / "benchmarks/performance/rest/workload.ore").read_text()
    old = '    if !input.Scan() || input.Text() != "stop" {\n        panic("missing stop")\n    }'
    new = """    for input.Scan() {
        if input.Text() == "snapshot" { probe(0) } else if input.Text() == "stop" { break } else { panic("invalid control") }
    }"""
    if old not in source:
        raise BenchmarkError("REST control instrumentation anchor changed")
    source = source.replace(old, new)
    old_drain = """    let ctx, cancel = context.WithTimeout(context.Background(), 5 * time.Second)
    let err = server.Shutdown(ctx)
    cancel()"""
    if old_drain not in source:
        raise BenchmarkError("REST drain instrumentation anchor changed")
    source = source.replace(
        old_drain, "    let err = server.Shutdown(context.Background())"
    )
    source = source.replace('import "std/time"\n', "")
    source = source.replace(
        '    println("DONE")', '    quiesce(0)\n    println("DONE")'
    )
    return source + "\nfunc probe(n int) {}\nfunc quiesce(n int) {}\n"


def hook_ir(source):
    for name in ["probe", "quiesce"]:
        pattern = (
            rf'^define (?:internal |private )?void @"main\.{name}"\([^\n]*\) \{{.*?^\}}'
        )
        replacement = f'define internal void @"main.{name}"(i64 %n) {{\nentry:\n  call void @zore_profile_{name}(i64 %n)\n  ret void\n}}'
        source, count = re.subn(
            pattern, lambda _match: replacement, source, flags=re.M | re.S
        )
        if count != 1:
            raise BenchmarkError(f"expected one {name} definition, found {count}")
        source += f"\ndeclare void @zore_profile_{name}(i64)\n"
    return source


def build_profile(directory, arguments):
    runtime = directory / "runtime"
    identities = runtime_sources(runtime, arguments.runtime_revision)
    rustc = os.environ.get("ZORE_RUSTC", "rustc")
    library = directory / "libzore_runtime.rlib"
    command(
        [
            rustc,
            "--edition=2024",
            "--crate-name=zore_runtime",
            "--crate-type=rlib",
            "--cfg=zore_test_hooks",
            "-Copt-level=2",
            "-Cpanic=abort",
            runtime / "lib.rs",
            "-o",
            library,
        ]
    )
    source = directory / "workload.ore"
    text = profiled_source()
    source.write_text(text)
    capture = directory / "captured"
    build(arguments.compiler, source, directory, directory / "cache", capture)
    ir = directory / "instrumented.ll"
    ir.write_text(hook_ir(capture.with_suffix(".ll").read_text()))
    object_path = directory / "instrumented.o"
    command(
        [
            os.environ.get("ZORE_CC", "clang"),
            "-Wno-override-module",
            "-O2",
            "-fPIC",
            "-c",
            ir,
            "-o",
            object_path,
        ]
    )
    entry = directory / "entry.rs"
    entry.write_text("""unsafe extern "C" { fn zore_entry(); }
#[unsafe(no_mangle)]
extern "C" fn zore_profile_probe(_: i64) {
    println!("{}", zore_runtime::scheduler::test_support::snapshot());
}
#[unsafe(no_mangle)]
extern "C" fn zore_profile_quiesce(count: i64) {
    zore_runtime::scheduler::test_support::wait_idle(usize::try_from(count).unwrap());
}
fn main() {
    zore_runtime::scheduler::test_support::install_pool(2);
    unsafe { zore_entry(); }
    zore_runtime::finish();
}
""")
    executable = directory / "profiled"
    command(
        [
            rustc,
            "--edition=2024",
            "-Copt-level=2",
            "-Cpanic=abort",
            "--extern",
            f"zore_runtime={library}",
            f"-Clink-arg={object_path}",
            entry,
            "-o",
            executable,
        ]
    )
    return executable, {
        "instrumented_runtime_sources": identities,
        "profiled_source_sha256": hashlib.sha256(text.encode()).hexdigest(),
        "worker_capacity": 2,
    }


def measure(executable, arguments):
    process = start(executable)
    observations = []
    started = time.monotonic()
    requests = 0
    measured_requests = 0
    try:
        ready = line(process, arguments.timeout)
        if not ready.startswith("READY 127.0.0.1:"):
            raise BenchmarkError(f"REST protocol: {ready}")
        address = ready.removeprefix("READY ")

        def checkpoint(phase, index):
            process_samples = settle(process, arguments.timeout)
            send(process, "snapshot")
            counters = json.loads(line(process, arguments.timeout))
            observations.append(
                {
                    "phase": phase,
                    "index": index,
                    "requests": requests,
                    "measured_requests": measured_requests,
                    "elapsed_seconds": time.monotonic() - started,
                    "runtime": counters,
                    "process": process_samples,
                }
            )

        checkpoint("before", 0)
        for index in range(1, arguments.rounds + 1):
            measured = batch(
                address,
                arguments.case,
                arguments.concurrency,
                arguments.requests,
                arguments.timeout,
            )
            measured_requests += arguments.requests
            requests += arguments.requests + arguments.concurrency
            checkpoint("load", index)
            observations[-1]["batch"] = measured
        idle_start = time.monotonic()
        index = 0
        while time.monotonic() - idle_start < arguments.idle_seconds:
            time.sleep(
                min(
                    arguments.idle_interval,
                    max(0, arguments.idle_seconds - (time.monotonic() - idle_start)),
                )
            )
            index += 1
            checkpoint("idle", index)
        finish(process, "stop", "DONE\n", arguments.timeout)
        return {
            "observations": observations,
            "exit_code": 0,
            "quiesced_before_exit": True,
        }
    finally:
        suspended.terminate(process)


def main():
    parser = argparse.ArgumentParser(
        description="Profile long fixed-case REST retention with runtime state counters."
    )
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--compiler-revision", required=True)
    parser.add_argument("--runtime-revision", required=True)
    parser.add_argument("--case", choices=list(CASES), default="get")
    parser.add_argument("--rounds", type=int, default=20)
    parser.add_argument("--requests", type=int, default=500)
    parser.add_argument("--concurrency", type=int, default=1)
    parser.add_argument("--idle-seconds", type=float, default=35)
    parser.add_argument("--idle-interval", type=float, default=1)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if (
        min(args.rounds, args.requests, args.concurrency) < 1
        or args.requests < args.concurrency
    ):
        parser.error(
            "positive rounds/requests/concurrency required, with requests >= concurrency"
        )
    if any(
        not math.isfinite(value) or value <= 0
        for value in [args.idle_seconds, args.idle_interval, args.timeout]
    ):
        parser.error("finite positive timing values required")
    if sys.platform != "linux":
        parser.error("retention profiling requires Linux /proc")
    args.compiler = args.compiler.resolve()
    try:
        report = metadata(args)
        with tempfile.TemporaryDirectory(prefix="zore-retention-") as temporary:
            executable, instrumentation = build_profile(Path(temporary), args)
            report["instrumentation"] = instrumentation
            report["retention"] = measure(executable, args)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    except (BenchmarkError, OSError, ValueError, KeyboardInterrupt) as error:
        print(f"retention profile failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
