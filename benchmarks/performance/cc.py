#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def recorded_command(executable, arguments, tool):
    capture = Path(os.environ["ZORE_BENCH_CAPTURE"])
    started = time.perf_counter()
    result = subprocess.run([executable, *arguments])
    if tool == "clang":
        role = "object" if "-c" in arguments else "link"
    elif "--crate-type=rlib" in arguments:
        role = "runtime"
    elif "--version" in arguments or "-vV" in arguments:
        role = "version"
    else:
        role = "link"
    record = {
        "tool": tool,
        "role": role,
        "command": [executable, *arguments],
        "started": started,
        "seconds": time.perf_counter() - started,
        "exit_code": result.returncode,
    }
    with capture.with_suffix(".commands.jsonl").open("a", encoding="utf-8") as output:
        output.write(json.dumps(record) + "\n")
    return result.returncode


def main():
    arguments = sys.argv[1:]
    capture = Path(os.environ["ZORE_BENCH_CAPTURE"])
    for argument in arguments:
        if argument.endswith(".ll") and Path(argument).is_file():
            shutil.copyfile(argument, capture.with_suffix(".ll"))
    started = time.perf_counter()
    result = recorded_command(os.environ["ZORE_BENCH_CC"], arguments, "clang")
    if any(argument.endswith(".ll") for argument in arguments):
        object_path = Path(arguments[arguments.index("-o") + 1])
        capture.with_suffix(".json").write_text(
            json.dumps(
                {
                    "command": [os.environ["ZORE_BENCH_CC"], *arguments],
                    "seconds": time.perf_counter() - started,
                    "exit_code": result,
                    "object_bytes": (
                        object_path.stat().st_size if object_path.is_file() else None
                    ),
                }
            )
            + "\n",
            encoding="utf-8",
        )
    return result


if __name__ == "__main__":
    raise SystemExit(main())
