import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
COMMAND_LIMIT = 20000


def executable_identity(executable, directory):
    name = re.sub(r"-[0-9a-f]{16,}(?:\.exe)?$", "", Path(executable).name)
    name = name.removesuffix(".exe")
    return f"{Path(directory).resolve().relative_to(ROOT).as_posix()}/{name}"


def shard_for(identity, name, shards):
    digest = hashlib.sha256(f"{identity}\0{name}".encode()).digest()
    return int.from_bytes(digest, "big") % shards + 1


def list_tests(executable, ignored=False, harness_arguments=()):
    command = [executable, *harness_arguments, "--list", "--format", "terse"]
    if ignored:
        command.append("--ignored")
    result = subprocess.run(command, capture_output=True, text=True, check=True)
    inventory = {}
    for line in result.stdout.splitlines():
        name, separator, kind = line.rpartition(": ")
        if separator and kind in ("test", "benchmark"):
            if name in inventory:
                raise ValueError(f"Duplicate test: {name}")
            inventory[name] = kind
        elif line.strip():
            raise ValueError(f"Unexpected libtest listing: {line}")
    return inventory


def filter_batches(executable, names, limit=COMMAND_LIMIT, harness_arguments=()):
    base = [executable, *harness_arguments, "--exact", "--nocapture"]
    batches = []
    current = []
    for name in names:
        if len(subprocess.list2cmdline(base + current + [name])) > limit:
            if not current:
                raise ValueError(f"Test name exceeds command limit: {name}")
            batches.append(current)
            current = []
        if len(subprocess.list2cmdline(base + [name])) > limit:
            raise ValueError(f"Test name exceeds command limit: {name}")
        current.append(name)
    if current:
        batches.append(current)
    return [base + batch for batch in batches]


def run_harness(executable, shard, shards, output, harness_arguments=()):
    identity = executable_identity(executable, Path.cwd())
    record = {"identity": identity, "executable": executable, "tests": [], "batches": []}
    started = time.monotonic()
    status = 1
    try:
        inventory = list_tests(executable, harness_arguments=harness_arguments)
        ignored = list_tests(executable, ignored=True, harness_arguments=harness_arguments)
        if not ignored.keys() <= inventory.keys():
            raise ValueError("Ignored tests are absent from the full inventory")
        record["tests"] = [
            {"name": name, "kind": kind, "ignored": name in ignored,
             "shard": shard_for(identity, name, shards)}
            for name, kind in sorted(inventory.items())
        ]
        selected = [test["name"] for test in record["tests"] if test["shard"] == shard]
        status = 0
        for command in filter_batches(executable, selected, harness_arguments=harness_arguments):
            batch_started = time.monotonic()
            result = subprocess.run(command, check=False)
            record["batches"].append({"tests": command[3 + len(harness_arguments):], "seconds": time.monotonic() - batch_started,
                                      "exit_code": result.returncode})
            if result.returncode:
                status = 1
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        status = 1
        record["error"] = str(error)
        print(f"{identity}: {error}", file=sys.stderr)
    finally:
        record["seconds"] = time.monotonic() - started
        record["exit_code"] = status
        output.mkdir(parents=True, exist_ok=True)
        key = hashlib.sha256(identity.encode()).hexdigest()
        (output / f"harness-{key}.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    return status


def write_report(output, shard, shards, status, started, error=None):
    records = [json.loads(path.read_text(encoding="utf-8")) for path in sorted(output.glob("harness-*.json"))]
    report = {"shard": shard, "shards": shards, "exit_code": status,
              "seconds": time.monotonic() - started, "harnesses": records}
    if error:
        report["error"] = error
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    lines = [f"## Rust tests: shard {shard}/{shards}", "",
             f"Exit code: {status}; wall time: {report['seconds']:.2f}s", "",
             "| Executable | Selected / total | Ignored selected | Batches | Seconds | Exit |",
             "| --- | ---: | ---: | ---: | ---: | ---: |"]
    for record in sorted(records, key=lambda item: item["seconds"], reverse=True):
        selected = [test for test in record["tests"] if test["shard"] == shard]
        lines.append(f"| {record['identity']} | {len(selected)} / {len(record['tests'])} | "
                     f"{sum(test['ignored'] for test in selected)} | {len(record['batches'])} | "
                     f"{record['seconds']:.2f} | {record['exit_code']} |")
    if error:
        lines.extend(["", f"Runner error: {error}"])
    summary = "\n".join(lines) + "\n"
    (output / "summary.md").write_text(summary, encoding="utf-8")
    print(summary)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as stream:
            stream.write(summary)


def main(arguments=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--shard", type=int, required=True)
    parser.add_argument("--shards", type=int, default=2)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runner", action="store_true")
    parser.add_argument("executable", nargs="?")
    parser.add_argument("harness_arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args(arguments)
    if not 1 <= args.shard <= args.shards:
        parser.error("shard must be between 1 and shards")
    output = args.output.resolve()
    if args.runner:
        if not args.executable:
            parser.error("runner requires an executable")
        return run_harness(args.executable, args.shard, args.shards, output, args.harness_arguments)
    output.mkdir(parents=True, exist_ok=True)
    if any(output.glob("harness-*.json")):
        parser.error("output already contains harness reports; use a fresh output directory")
    started = time.monotonic()
    status = 1
    error = None
    try:
        host_output = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout
        host = next(line.removeprefix("host: ") for line in host_output.splitlines() if line.startswith("host: "))
        runner = [sys.executable, str(Path(__file__).resolve()), "--shard", str(args.shard),
                  "--shards", str(args.shards), "--output", str(output), "--runner"]
        config = f"target.{host}.runner={json.dumps(runner)}"
        status = subprocess.run(["cargo", "test", "--locked", "--all-targets", "--no-fail-fast",
                                 "--config", config], cwd=ROOT, check=False).returncode
        if status == 0 and not any(output.glob("harness-*.json")):
            raise ValueError("Cargo succeeded without invoking any test harnesses")
    except (OSError, ValueError, StopIteration, subprocess.SubprocessError) as failure:
        status = 1
        error = str(failure)
        print(error, file=sys.stderr)
    finally:
        write_report(output, args.shard, args.shards, status, started, error)
    return status


if __name__ == "__main__":
    raise SystemExit(main())
