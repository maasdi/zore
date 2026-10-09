#!/usr/bin/env python3

import argparse
import json
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
WORKLOADS = {
    "compute": (Path(__file__).with_name("compute") / "workload.ore", "true\n"),
    "io": (Path(__file__).with_name("io") / "workload.ore", "300\n"),
}


def command(args, directory):
    return subprocess.run(
        args,
        cwd=directory,
        capture_output=True,
        text=True,
        timeout=60,
        check=True,
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--repetitions", type=int, default=5)
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("repetitions must be positive")
    compilers = {
        "baseline": args.baseline.resolve(),
        "candidate": args.candidate.resolve(),
    }
    results = {}
    with tempfile.TemporaryDirectory(prefix="zore-budget-benchmark-") as temporary:
        temporary = Path(temporary)
        for workload, (source, expected) in WORKLOADS.items():
            executables = {}
            for variant, compiler in compilers.items():
                directory = temporary / workload / variant
                directory.mkdir(parents=True)
                command([str(compiler), "build", str(source)], directory)
                executables[variant] = directory / source.stem
            samples = {variant: [] for variant in compilers}
            for repetition in range(args.repetitions):
                variants = list(compilers)
                if repetition % 2:
                    variants.reverse()
                for variant in variants:
                    started = time.perf_counter()
                    output = command([str(executables[variant])], temporary)
                    elapsed = time.perf_counter() - started
                    if output.stdout != expected or output.stderr:
                        raise RuntimeError(
                            f"{workload} {variant}: unexpected output {output.stdout!r}, {output.stderr!r}"
                        )
                    samples[variant].append(round(elapsed, 6))
            results[workload] = {
                variant: {
                    "seconds": values,
                    "median_seconds": statistics.median(values),
                }
                for variant, values in samples.items()
            }
    print(
        json.dumps(
            {
                "runner_commit": command(["git", "rev-parse", "HEAD"], ROOT).stdout.strip(),
                "compilers": {key: str(value) for key, value in compilers.items()},
                "repetitions": args.repetitions,
                "results": results,
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
