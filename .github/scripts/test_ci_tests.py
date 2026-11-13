import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import ci_tests


class PartitionTests(unittest.TestCase):
    def test_partition_is_complete_disjoint_and_order_independent(self):
        names = [f"module::test_{index}" for index in range(1000)]
        first = {name for name in names if ci_tests.shard_for("compiler/native", name, 2) == 1}
        second = {name for name in reversed(names) if ci_tests.shard_for("compiler/native", name, 2) == 2}
        self.assertFalse(first & second)
        self.assertEqual(first | second, set(names))
        self.assertGreater(len(first), 400)
        self.assertGreater(len(second), 400)
        self.assertEqual(ci_tests.shard_for("compiler/native", "module::test_1", 2), 2)

    def test_identity_ignores_artifact_hash_and_platform_extension(self):
        directory = ci_tests.ROOT / "compiler"
        first = ci_tests.executable_identity("native-0123456789abcdef.exe", directory)
        second = ci_tests.executable_identity("native-fedcba9876543210", directory)
        self.assertEqual(first, "compiler/native")
        self.assertEqual(first, second)

    def test_batches_use_exact_filters_and_obey_command_limit(self):
        names = [f"module::test_{index}" for index in range(20)]
        batches = ci_tests.filter_batches("executable with spaces.exe", names, limit=110)
        self.assertGreater(len(batches), 1)
        self.assertEqual([name for batch in batches for name in batch[3:]], names)
        for batch in batches:
            self.assertEqual(batch[:3], ["executable with spaces.exe", "--exact", "--nocapture"])
            self.assertLessEqual(len(subprocess.list2cmdline(batch)), 110)
        with self.assertRaises(ValueError):
            ci_tests.filter_batches("test", ["a" * 1000], limit=110)

    def test_cargo_harness_mode_arguments_are_preserved(self):
        self.assertEqual(ci_tests.filter_batches("exe", ["test"], harness_arguments=["--test"]),
                         [["exe", "--test", "--exact", "--nocapture", "test"]])
        result = subprocess.CompletedProcess([], 0, "test: test\n", "")
        with patch.object(ci_tests.subprocess, "run", return_value=result) as run:
            ci_tests.list_tests("exe", harness_arguments=["--test"])
        self.assertEqual(run.call_args.args[0], ["exe", "--test", "--list", "--format", "terse"])


class HarnessTests(unittest.TestCase):
    def test_listing_accepts_tests_and_benchmarks_and_rejects_unknown_output(self):
        result = subprocess.CompletedProcess([], 0, "module::a: test\nbench: benchmark\n", "")
        with patch.object(ci_tests.subprocess, "run", return_value=result) as run:
            self.assertEqual(ci_tests.list_tests("executable", ignored=True),
                             {"module::a": "test", "bench": "benchmark"})
        self.assertEqual(run.call_args.args[0], ["executable", "--list", "--format", "terse", "--ignored"])
        result.stdout = "unexpected output\n"
        with patch.object(ci_tests.subprocess, "run", return_value=result):
            with self.assertRaises(ValueError):
                ci_tests.list_tests("executable")

    def test_ignored_tests_remain_ignored_and_failure_does_not_stop_later_batches(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            inventory = {"alpha": "test", "alpha_extended": "test", "ignored": "test"}
            with patch.object(ci_tests, "list_tests", side_effect=[inventory, {"ignored": "test"}]), \
                    patch.object(ci_tests, "filter_batches", return_value=[
                        ["exe", "--exact", "--nocapture", "alpha", "ignored"],
                        ["exe", "--exact", "--nocapture", "alpha_extended"]]), \
                    patch.object(ci_tests.subprocess, "run", side_effect=[
                        subprocess.CompletedProcess([], 1), subprocess.CompletedProcess([], 0)]) as run:
                status = ci_tests.run_harness("exe", 1, 1, output)
            self.assertEqual(status, 1)
            self.assertEqual(run.call_count, 2)
            self.assertNotIn("--include-ignored", run.call_args_list[0].args[0])
            record = json.loads(next(output.glob("harness-*.json")).read_text())
            self.assertEqual(len(record["tests"]), 3)
            self.assertTrue(next(test for test in record["tests"] if test["name"] == "ignored")["ignored"])
            self.assertEqual([batch["exit_code"] for batch in record["batches"]], [1, 0])

    def test_empty_shard_does_not_invoke_unfiltered_harness(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(ci_tests, "list_tests", return_value={}), \
                    patch.object(ci_tests.subprocess, "run") as run:
                status = ci_tests.run_harness("exe", 1, 2, Path(temporary))
            self.assertEqual(status, 0)
            run.assert_not_called()

    def test_discovery_failure_still_writes_failed_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            with patch.object(ci_tests, "list_tests", side_effect=subprocess.CalledProcessError(7, ["exe"])):
                status = ci_tests.run_harness("exe", 1, 2, output)
            self.assertEqual(status, 1)
            record = json.loads(next(output.glob("harness-*.json")).read_text())
            self.assertEqual(record["exit_code"], 1)
            self.assertIn("error", record)

    def test_invalid_ignored_inventory_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(ci_tests, "list_tests", side_effect=[{}, {"absent": "test"}]):
                self.assertEqual(ci_tests.run_harness("exe", 1, 2, Path(temporary)), 1)

    def test_failure_after_listing_has_nonzero_status(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            with patch.object(ci_tests, "list_tests", side_effect=[{"test": "test"}, {}]), \
                    patch.object(ci_tests.subprocess, "run", side_effect=OSError("launch failed")):
                self.assertEqual(ci_tests.run_harness("exe", 1, 1, output), 1)
            record = json.loads(next(output.glob("harness-*.json")).read_text())
            self.assertEqual(record["exit_code"], 1)
            self.assertIn("launch failed", record["error"])

    def test_cargo_success_without_reports_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            with patch.object(ci_tests.subprocess, "run", side_effect=[
                    subprocess.CompletedProcess([], 0, "host: test-host\n", ""),
                    subprocess.CompletedProcess([], 0)]):
                self.assertEqual(ci_tests.main(["--shard", "1", "--output", str(output)]), 1)
            report = json.loads((output / "report.json").read_text())
            self.assertEqual(report["exit_code"], 1)
            self.assertIn("without invoking", report["error"])

    def test_cargo_failure_still_writes_summary_and_json(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            with patch.object(ci_tests.subprocess, "run", side_effect=[
                    subprocess.CompletedProcess([], 0, "host: test-host\n", ""),
                    subprocess.CompletedProcess([], 9)]) as run:
                status = ci_tests.main(["--shard", "2", "--output", str(output)])
            self.assertEqual(status, 9)
            command = run.call_args.args[0]
            self.assertEqual(command[:6], ["cargo", "test", "--locked", "--all-targets", "--no-fail-fast", "--config"])
            self.assertIn("--runner", command[6])
            self.assertEqual(json.loads((output / "report.json").read_text())["exit_code"], 9)
            self.assertTrue((output / "summary.md").exists())


if __name__ == "__main__":
    unittest.main()
