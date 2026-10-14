from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest

from common import BenchmarkError, ROOT, line
from compile import frame_layouts
from retention import hook_ir, profiled_source
from rest_requests import batch


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
        compiler = ROOT / "target/debug/zore"
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
