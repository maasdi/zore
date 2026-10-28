#!/usr/bin/env python3
import os
import sys

from cc import recorded_command


def instrumented_arguments(arguments):
    if os.name == "nt" and not any(
        argument in arguments
        for argument in ["--crate-type=rlib", "--version", "-vV"]
    ):
        linker = os.path.join(os.environ["ZORE_BENCH_WRAPPER_DIRECTORY"], "link.exe")
        return [*arguments, "-C", f"linker={linker}"]
    return arguments


if __name__ == "__main__":
    raise SystemExit(recorded_command(
        os.environ["ZORE_BENCH_RUSTC"], instrumented_arguments(sys.argv[1:]), "rustc"
    ))
