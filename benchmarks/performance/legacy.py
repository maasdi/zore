import json
import sys

from common import ROOT, command


def measure_legacy(arguments):
    budget = command(
        [
            sys.executable,
            ROOT / "benchmarks/cooperative-budget/run.py",
            "--baseline",
            arguments.compiler,
            "--candidate",
            arguments.compiler,
            "--repetitions",
            arguments.repetitions,
        ]
    )
    scaling = command(
        [
            "cargo",
            "run",
            "--release",
            "--locked",
            "--example",
            "async_frame_scaling",
            "--",
            "--json",
            arguments.repetitions,
            100,
            200,
            400,
            800,
        ]
    )
    return {
        "cooperative_budget": {
            "comparison": "same supplied compiler in both positions; no optimization comparison",
            "compiler_revision": arguments.compiler_revision or "unverified compiler",
            "measurements": json.loads(budget.stdout),
        },
        "frame_reuse_scaling": {
            "compiler": "release library built from runner checkout, not supplied compiler",
            "measurements": [json.loads(line) for line in scaling.stdout.splitlines()],
        },
    }
