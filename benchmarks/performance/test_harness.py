from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from common import BenchmarkError, ROOT, line
from compile import frame_layouts, measure_compile, validate_runtime_cache
from retention import hook_ir, profiled_source
from rest_requests import batch
from run import metadata


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    valid = True

    def do_GET(self):
        value = {"id": 1, "name": "Ada Lovelace"} if self.valid else {"wrong": True}
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_arguments):
        pass


class HarnessTests(unittest.TestCase):
    def test_shared_metadata_supports_retention_arguments_without_stage_profilers(self):
        with tempfile.TemporaryDirectory() as directory:
            compiler = Path(directory) / "compiler"
            compiler.write_bytes(b"compiler identity")
            arguments = SimpleNamespace(compiler=compiler, compiler_revision="revision")
            with patch("run.command", return_value=SimpleNamespace(stdout="tool output\n")):
                result = metadata(arguments)
        self.assertEqual(result["stage_profilers"], {})
        self.assertEqual(result["compiler_revision"], "revision")

    def test_metadata_distinguishes_cpu_affinity_from_logical_core_count(self):
        affinity_cases = [
            ({"sched_getaffinity": Mock(return_value={2, 7})}, 2),
            ({}, None),
            ({"sched_getaffinity": Mock(side_effect=OSError("unavailable"))}, None),
        ]
        with tempfile.TemporaryDirectory() as directory:
            compiler = Path(directory) / "compiler"
            compiler.write_bytes(b"compiler identity")
            arguments = SimpleNamespace(compiler=compiler, compiler_revision="revision")
            for affinity_api, expected_count in affinity_cases:
                with self.subTest(affinity_api=affinity_api):
                    host_os = SimpleNamespace(
                        environ=os.environ,
                        cpu_count=lambda: 64,
                        getloadavg=lambda: (0.0, 0.0, 0.0),
                        **affinity_api,
                    )
                    with patch("run.os", host_os), patch(
                        "run.command", return_value=SimpleNamespace(stdout="tool output\n")
                    ):
                        host = metadata(arguments)["host"]
                    self.assertEqual(host["logical_core_count"], 64)
                    self.assertEqual(host["cpu_affinity_count"], expected_count)
                    if "sched_getaffinity" in affinity_api:
                        affinity_api["sched_getaffinity"].assert_called_once_with(0)

    def test_compile_pairs_alternate_with_separate_stable_runtime_caches(self):
        def fake_build(compiler, source, directory, cache, capture=None):
            directory.mkdir(parents=True, exist_ok=True)
            if capture:
                capture.with_suffix(".ll").write_text("IR")
                capture.with_suffix(".json").write_text('{"object_bytes": 1}')
                artifact = {str(cache): {"sha256": "unchanged"}}
                capture.with_suffix(".build.json").write_text(json.dumps({
                    "cache_before": artifact,
                    "cache_after": artifact,
                    "commands": [],
                }))
            return directory / source.stem, 1.0

        arguments = SimpleNamespace(
            compiler_revision="candidate-revision",
            baseline_compiler=Path("baseline"),
            baseline_revision="baseline-revision",
            stage_profiler=None,
            baseline_stage_profiler=None,
            repetitions=3,
            warm_builds_only=True,
            compile_lock_file=None,
        )
        with tempfile.TemporaryDirectory() as directory:
            with patch("compile.build", side_effect=fake_build), patch("compile.command"):
                rows = measure_compile(
                    Path("candidate"), {"hello": Path("hello.ore")},
                    Path(directory), arguments,
                )
        self.assertEqual(
            [row["variant"] for row in rows],
            ["baseline", "candidate", "candidate", "baseline", "baseline", "candidate"],
        )
        self.assertEqual([row["repetition"] for row in rows], [1, 1, 2, 2, 3, 3])
        for offset in range(0, len(rows), 2):
            pair = rows[offset:offset + 2]
            self.assertEqual(pair[0]["variant_order"], [row["variant"] for row in pair])
            self.assertNotEqual(
                pair[0]["warm_profile"]["cache_before"],
                pair[1]["warm_profile"]["cache_before"],
            )

    def test_cache_evidence_rejects_fallback_and_changed_warm_artifacts(self):
        artifact = {"runtime/libzore_runtime.rlib": {"bytes": 7, "sha256": "hash", "mtime_ns": 1}}
        version = {"tool": "rustc", "role": "version"}
        runtime = {"tool": "rustc", "role": "runtime"}
        warm = {"cache_before": artifact, "cache_after": artifact, "commands": [version]}
        cold = {"cache_before": {}, "cache_after": artifact, "commands": [runtime]}
        validate_runtime_cache(warm, cold=False)
        validate_runtime_cache(cold, cold=True)
        for profile, is_cold in [
            ({**warm, "commands": [version, runtime]}, False),
            ({**warm, "cache_after": {}}, False),
            ({**warm, "cache_after": {"runtime/other.rlib": {}}}, False),
            ({**cold, "cache_after": {}}, True),
            ({**cold, "commands": [version]}, True),
        ]:
            with self.assertRaises(BenchmarkError):
                validate_runtime_cache(profile, cold=is_cold)

    def test_profile_hooks_handle_both_linkages_and_keep_the_entry_abi(self):
        for linkage in ["", "internal "]:
            source = (
                "\n".join(
                    f'define {linkage}void @"main.{name}"(i64 %n) {{\n  ret void\n}}'
                    for name in ["probe", "quiesce"]
                )
                + "\ndefine void @zore_entry() {\n  ret void\n}\n"
            )
            instrumented = hook_ir(source)
            self.assertIn("call void @zore_profile_probe(i64 %n)", instrumented)
            self.assertIn("call void @zore_profile_quiesce(i64 %n)", instrumented)
            self.assertIn("define void @zore_entry()", instrumented)
        with self.assertRaises(BenchmarkError):
            hook_ir("")

    def test_profile_control_program_passes_the_real_frontend(self):
        compiler = ROOT / "target/debug" / ("zore.exe" if os.name == "nt" else "zore")
        if not compiler.is_file():
            self.skipTest(f"{compiler} is missing; build it with cargo build --locked")
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "workload.ore"
            source.write_text(profiled_source())
            result = subprocess.run(
                [str(compiler), "check", str(source)],
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipIf(os.name == "nt", "pipe selectors require Unix")
    def test_partial_protocol_line_cannot_bypass_the_deadline(self):
        reader, writer = os.pipe()
        try:
            with os.fdopen(reader, "rb") as stdout:
                os.write(writer, b"READY")
                with self.assertRaisesRegex(BenchmarkError, "timed out"):
                    line(SimpleNamespace(stdout=stdout), 0.01)
        finally:
            os.close(writer)

    def test_clang_reports_padded_frame_size_and_constructor_identity(self):
        if not shutil.which(os.environ.get("ZORE_CC", "clang")):
            self.skipTest("clang is unavailable")
        ir = """%"main.AsyncFrame.17" = type { [3 x i8], i64, ptr }
define private ptr @"main.wait$async$new"() {
  %frame = alloca %"main.AsyncFrame.17"
  ret ptr null
}
"""
        with tempfile.TemporaryDirectory() as directory:
            frames = frame_layouts(ir, Path(directory))
        self.assertEqual(frames[0]["bytes"], 24)
        self.assertEqual(frames[0]["constructors"], ['@"main.wait$async$new"'])

    def test_missing_frames_are_rejected_instead_of_reporting_empty_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(BenchmarkError):
                frame_layouts("", Path(directory))

    def exercise_server(self, valid):
        handler = type("SelectedHandler", (Handler,), {"valid": valid})
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            return batch(f"127.0.0.1:{server.server_port}", "get", 2, 6, 5)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

    def test_rest_batches_reuse_warmed_connections_and_preserve_every_measurement(self):
        result = self.exercise_server(True)
        self.assertEqual(len(result["raw_requests"]), 6)
        self.assertEqual(result["opened_connections"], 0)
        self.assertTrue(
            all(sample["response_bytes"] > 0 for sample in result["raw_requests"])
        )

    def test_wrong_response_fails_the_batch_and_releases_all_workers(self):
        with self.assertRaises(BenchmarkError):
            self.exercise_server(False)

    def test_invalid_configuration_is_rejected_before_measurement(self):
        script = Path(__file__).with_name("run.py")
        for arguments in [
            ["--timeout", "nan"],
            ["--repetitions", "0"],
            ["--requests", "1", "--concurrency", "4"],
        ]:
            result = subprocess.run(
                [sys.executable, str(script), "--output", "/unused", *arguments],
                capture_output=True,
                text=True,
                timeout=5,
            )
            self.assertEqual(result.returncode, 2, result.stderr)


if __name__ == "__main__":
    unittest.main()
