#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def main():
    arguments = sys.argv[1:]
    capture = Path(os.environ["ZORE_BENCH_CAPTURE"])
    for argument in arguments:
        if argument.endswith(".ll") and Path(argument).is_file():
            shutil.copyfile(argument, capture.with_suffix(".ll"))
    started = time.perf_counter()
    result = subprocess.run([os.environ["ZORE_BENCH_CC"], *arguments])
    if any(argument.endswith(".ll") for argument in arguments):
        object_path = Path(arguments[arguments.index("-o") + 1])
        capture.with_suffix(".json").write_text(
            json.dumps(
                {
                    "command": [os.environ["ZORE_BENCH_CC"], *arguments],
                    "seconds": time.perf_counter() - started,
                    "exit_code": result.returncode,
                    "object_bytes": (
                        object_path.stat().st_size if object_path.is_file() else None
                    ),
                }
            )
            + "\n",
            encoding="utf-8",
        )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
