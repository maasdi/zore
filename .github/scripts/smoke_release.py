import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

from package_release import ROOT, TARGETS, verify_and_extract


MAIN = '''package main

import "std/errors"
import "std/net/http"
import "std/os"
import "std/strings"

func greeting() string {
    return strings.ToUpper("alpha ready")
}

func health(w mut http.ResponseWriter, r mut http.Request) error {
    return w.Text(200, greeting())
}

func serve() error {
    var routes = http.NewRouterBuilder()
    routes.Handle("GET /health", health)
    let router = routes.Build()?
    return http.ListenAndServe("ADDRESS", router)
}

func main() {
    let err = serve()
    if err != nil {
        println(errors.Message(err))
        os.Exit(1)
    }
}
'''
TEST = '''package main

func TestBundledStandardLibrary() {
    if greeting() != "ALPHA READY" {
        panic("bundled standard library failed")
    }
}
'''


def command(arguments, directory, environment):
    result = subprocess.run(arguments, cwd=directory, env=environment, capture_output=True, text=True, timeout=600)
    print(result.stdout, end="", flush=True)
    if result.returncode:
        raise RuntimeError(f"{arguments!r} failed ({result.returncode}):\n{result.stderr}")
    return result.stdout


def clean_environment(directory):
    environment = {name: value for name, value in os.environ.items()
                   if not name.startswith(("ZORE_", "CARGO_", "RUST_"))}
    environment["ZORE_CACHE_DIR"] = str(directory / "cache")
    environment["TMPDIR"] = str(directory / "temporary")
    environment["TMP"] = environment["TMPDIR"]
    environment["TEMP"] = environment["TMPDIR"]
    (directory / "temporary").mkdir()
    return environment


def smoke_http(executable, directory, environment, port):
    with (directory / "server.log").open("w+", encoding="utf-8") as output:
        process = subprocess.Popen([str(executable)], cwd=directory, env=environment,
                                   stdout=output, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 30
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    break
                try:
                    with opener.open(f"http://127.0.0.1:{port}/health", timeout=1) as response:
                        if response.status != 200 or response.read() != b"ALPHA READY":
                            raise RuntimeError("native HTTP smoke returned an unexpected response")
                    print("native HTTP smoke passed", flush=True)
                    return
                except (urllib.error.URLError, TimeoutError):
                    time.sleep(0.1)
            output.seek(0)
            raise RuntimeError(f"native HTTP smoke failed:\n{output.read()}")
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=15)


def smoke(archive, version, target):
    with tempfile.TemporaryDirectory(prefix="zore-installed-smoke-") as temporary:
        directory = Path(temporary).resolve()
        if directory.is_relative_to(ROOT):
            raise ValueError("release smoke must run outside the source checkout")
        compiler = verify_and_extract(archive, directory / "installed", version, target)
        project = directory / "independent-project"
        project.mkdir()
        with socket.socket() as reserved:
            reserved.bind(("127.0.0.1", 0))
            port = reserved.getsockname()[1]
        (project / "zore.toml").write_text('name = "alpha_smoke"\nversion = "0.1.0"\n', encoding="utf-8")
        (project / "main.ore").write_text(MAIN.replace("ADDRESS", f"127.0.0.1:{port}"), encoding="utf-8")
        (project / "main_test.ore").write_text(TEST, encoding="utf-8")
        environment = clean_environment(directory)
        environment["RUSTUP_TOOLCHAIN"] = json.loads((compiler.parent / "artifact.json").read_text(encoding="utf-8"))["toolchain"]
        native_compiler = os.environ.get("ZORE_CC")
        frontend_environment = environment.copy()
        tool_free_path = directory / "empty-path"
        tool_free_path.mkdir()
        frontend_environment["PATH"] = str(tool_free_path)
        if any(shutil.which(tool, path=frontend_environment["PATH"]) for tool in ("clang", "rustc")):
            raise ValueError("frontend smoke must exclude native tools")
        if command([str(compiler), "--version"], project, frontend_environment).strip() != f"zore {version}":
            raise ValueError("installed compiler version does not match release")
        command([str(compiler), "check", "main.ore"], project, frontend_environment)
        if native_compiler:
            environment["ZORE_CC"] = native_compiler
        test_output = command([str(compiler), "test", "."], project, environment)
        if "Tests: 1 passed, 0 failed" not in test_output or "::TestBundledStandardLibrary" not in test_output:
            raise RuntimeError("installed compiler did not run the standard library regression test")
        command([str(compiler), "build", "main.ore"], project, environment)
        executable = project / ("main.exe" if target.endswith("windows-msvc") else "main")
        smoke_http(executable, project, environment, port)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=TARGETS, required=True)
    arguments = parser.parse_args()
    smoke(arguments.archive.resolve(), arguments.version, arguments.target)


if __name__ == "__main__":
    main()
