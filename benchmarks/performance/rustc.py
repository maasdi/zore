#!/usr/bin/env python3
import os
import sys

from cc import recorded_command


if __name__ == "__main__":
    raise SystemExit(recorded_command(os.environ["ZORE_BENCH_RUSTC"], sys.argv[1:], "rustc"))
