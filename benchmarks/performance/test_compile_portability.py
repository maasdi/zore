import errno
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from common import BenchmarkError, ROOT
from compile import acquire_windows_lock, build, prepare_tool_wrappers
from run import load_average, supports_suites
from rustc import instrumented_arguments


class CompilePortabilityTests(unittest.TestCase):
    def test_only_compile_measurements_are_available_on_windows(self):
        self.assertTrue(supports_suites("win32", ["compile"]))
        for suites in [["rest"], ["compile", "rest"], ["frames"], ["json"], ["legacy"]]:
            self.assertFalse(supports_suites("win32", suites))
        self.assertTrue(supports_suites("linux", ["compile", "rest", "frames"]))
        self.assertFalse(supports_suites("darwin", ["compile"]))

    def test_host_metadata_accepts_absent_or_unavailable_load_average(self):
        with patch("run.os", SimpleNamespace()):
            self.assertIsNone(load_average())
        with patch("run.os", SimpleNamespace(getloadavg=Mock(side_effect=OSError()))):
            self.assertIsNone(load_average())

    def test_windows_rustc_instruments_only_the_entry_link(self):
        host = SimpleNamespace(name="nt", environ={"ZORE_BENCH_WRAPPER_DIRECTORY": "wrapper directory"}, path=os.path)
        with patch("rustc.os", host):
            for arguments in [["-vV"], ["--version"], ["--crate-type=rlib", "runtime.rs"]]:
                self.assertEqual(instrumented_arguments(arguments), arguments)
            arguments = ["--edition=2024", "entry with spaces.rs", "-o", "program.exe"]
            self.assertEqual(instrumented_arguments(arguments), [*arguments, "-C", f"linker={os.path.join('wrapper directory', 'link.exe')}"])

    def test_windows_lease_retries_beyond_the_native_ten_second_limit(self):
        locking = Mock(side_effect=[OSError(errno.EACCES, "busy"), None])
        api = SimpleNamespace(locking=locking, LK_NBLCK=2)
        lock = Mock()
        with patch.dict(sys.modules, {"msvcrt": api}), patch("compile.time.monotonic", side_effect=[0, 12]), patch("compile.time.sleep") as sleep:
            acquire_windows_lock(lock)
        self.assertEqual(locking.call_count, 2)
        sleep.assert_called_once_with(0.05)

    def test_windows_lease_wait_is_bounded(self):
        api = SimpleNamespace(locking=Mock(side_effect=OSError(errno.EACCES, "busy")), LK_NBLCK=2)
        with patch.dict(sys.modules, {"msvcrt": api}), patch("compile.time.monotonic", side_effect=[0, 901]):
            with self.assertRaisesRegex(BenchmarkError, "timed out"):
                acquire_windows_lock(Mock())

    def test_simultaneous_tools_keep_independent_complete_command_records(self):
        with tempfile.TemporaryDirectory() as temporary:
            capture = Path(temporary) / "capture"
            environment = {**os.environ, "ZORE_BENCH_CAPTURE": str(capture)}
            script = Path(__file__).with_name("cc.py")
            expression = "from cc import recorded_command; import sys; sys.exit(recorded_command(sys.executable, ['-c', 'raise SystemExit(17)'], 'link'))"
            children = [subprocess.Popen([sys.executable, "-c", expression], cwd=script.parent, env=environment) for _ in range(6)]
            self.assertEqual([child.wait(timeout=15) for child in children], [17] * 6)
            records = [json.loads(path.read_text(encoding="utf-8")) for path in capture.parent.glob("capture.command-*.json")]
            self.assertEqual(len(records), 6)
            self.assertTrue(all(record["exit_code"] == 17 and record["tool"] == "link" and record["role"] == "link" for record in records))

    def test_capture_preserves_exact_ir_and_object_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            ir = directory / "program.ll"
            obj = directory / "program.obj"
            capture = directory / "capture"
            ir.write_bytes(b"exact IR\n")
            obj.write_bytes(b"exact object\0")
            with patch.dict(os.environ, {"ZORE_BENCH_CAPTURE": str(capture), "ZORE_BENCH_CC": "real clang"}), patch("cc.sys.argv", ["cc.py", "-c", str(ir), "-o", str(obj)]), patch("cc.recorded_command", return_value=0):
                from cc import main

                self.assertEqual(main(), 0)
            profile = json.loads(capture.with_suffix(".json").read_text(encoding="utf-8"))
            self.assertEqual(profile["ir_sha256"], hashlib.sha256(ir.read_bytes()).hexdigest())
            self.assertEqual(profile["object_sha256"], hashlib.sha256(obj.read_bytes()).hexdigest())
            self.assertEqual(profile["object_bytes"], len(obj.read_bytes()))


@unittest.skipUnless(os.name == "nt", "Windows executable launcher coverage")
class WindowsLauncherTests(unittest.TestCase):
    def test_native_launcher_forwards_arguments_and_exit_codes_without_shell_parsing(self):
        linker = os.environ.get("ZORE_BENCH_LINK") or shutil.which("link.exe")
        if not linker or "\\Git\\" in linker:
            self.skipTest("MSVC linker is unavailable")
        with tempfile.TemporaryDirectory(prefix="zore launcher spaces ") as temporary:
            directory = Path(temporary)
            with patch.dict(os.environ, {"ZORE_BENCH_LINK": linker}):
                environment = {**os.environ, **prepare_tool_wrappers(directory / "wrapper binaries")}
            scripts = directory / "script directory"
            scripts.mkdir()
            (scripts / "cc.py").write_text("import json,sys\nprint(json.dumps(sys.argv[1:]))\nraise SystemExit(17)\n", encoding="utf-8")
            environment["ZORE_BENCH_SCRIPT_DIRECTORY"] = str(scripts)
            arguments = ["path with spaces", 'embedded"quote', "", "trailing\\", "literal&value", "$(literal)"]
            result = subprocess.run([str(directory / "wrapper binaries/cc.exe"), *arguments], env=environment, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 17, result.stderr)
            self.assertEqual(json.loads(result.stdout), arguments)

    def test_windows_native_capture_observes_nested_msvc_linking(self):
        compiler = Path(os.environ.get("ZORE_SCALING_COMPILER", ROOT / "target/debug/zore.exe"))
        linker = os.environ.get("ZORE_BENCH_LINK") or shutil.which("link.exe")
        if not compiler.is_file() or not linker or "\\Git\\" in linker or not shutil.which(os.environ.get("ZORE_CC", "clang")):
            self.skipTest("native Windows compiler, clang and MSVC linker are required")
        with tempfile.TemporaryDirectory(prefix="zore native capture ") as temporary:
            directory = Path(temporary)
            with patch.dict(os.environ, {"ZORE_BENCH_LINK": linker}):
                environment = prepare_tool_wrappers(directory / "wrappers")
                with patch.dict(os.environ, environment):
                    source = ROOT / "examples/hello/main.ore"
                    executable, _ = build(compiler, source, directory / "build", directory / "cache", directory / "capture")
            profile = json.loads((directory / "capture.build.json").read_text(encoding="utf-8"))
            self.assertTrue(any(item["tool"] == "link" and item["role"] == "link" for item in profile["commands"]))
            self.assertTrue(executable.is_file())
            result = subprocess.run([str(executable)], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
