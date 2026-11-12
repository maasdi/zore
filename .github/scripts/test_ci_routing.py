from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from ci_routing import classify_paths, distribution_gate_errors, gate_errors, route_event, write_routes


class RoutingTests(unittest.TestCase):
    def assert_routes(self, paths, full=False, docs=False, benchmarks=False, distribution=False):
        routes = classify_paths(paths)
        self.assertEqual(routes.outputs(), {
            "full": str(full).lower(), "docs": str(docs).lower(),
            "benchmarks": str(benchmarks).lower(), "distribution": str(distribution).lower(),
        })

    def test_allowlisted_documentation(self):
        self.assert_routes(["CHANGELOG.md", "CONTRIBUTING.md", "docs/README.md", "docs/alpha.md",
                            "docs/release-workflow.md"], docs=True)

    def test_executable_documentation_inputs_require_full_checks(self):
        for path in ("README.md", "docs/archives.md", "docs/csv.md", "docs/regexp.md", "docs/html.md",
                     "docs/random.md", "docs/gzip.md"):
            with self.subTest(path=path):
                self.assert_routes([path], full=True, benchmarks=True, distribution=True)

    def test_semantic_documents_and_unknown_paths_require_full_checks(self):
        for path in (
            "docs/design/runtime.md", "docs/conformance/mvp.md", "docs/spec-questions.md",
            "docs/roadmap.md", "tests/README.md", "spec/language-spec.md", "docs/data.json", "docs/new-guide.md",
            ".github/workflows/ci.yml", ".github/scripts/ci_routing.py", "Cargo.lock",
            "compiler/src/main.rs", "runtime/src/lib.rs", "std/strings/strings.ore",
            "examples/main.ore", "packages/example.ore", "unknown.txt", "benchmarks/data.json",
        ):
            with self.subTest(path=path):
                self.assert_routes([path], full=True, benchmarks=True, distribution=True)

    def test_benchmark_and_distribution_groups_combine(self):
        self.assert_routes(["benchmarks/run.py"], benchmarks=True)
        self.assert_routes(["benchmarks/nested/run.py", "docs/README.md"], docs=True, benchmarks=True)
        for path in (
            "distribution/linux/Dockerfile", ".github/scripts/package_release.py",
            ".github/scripts/smoke_release.py", ".github/scripts/smoke_builder.py",
            ".github/scripts/test_release.py",
        ):
            with self.subTest(path=path):
                self.assert_routes([path], distribution=True)
        self.assert_routes(["docs/alpha.md", "benchmarks/run.py", "distribution/test.py"],
                           docs=True, benchmarks=True, distribution=True)

    def test_full_route_keeps_documentation_flag(self):
        self.assert_routes(["CONTRIBUTING.md", "Cargo.toml"], full=True, docs=True, benchmarks=True, distribution=True)

    def test_empty_diff_requires_full_checks(self):
        self.assert_routes([], full=True, benchmarks=True, distribution=True)

    def test_non_pull_request_events_always_require_full_checks(self):
        for event in ("push", "schedule", "workflow_dispatch", "pull_request_target", "", None):
            with self.subTest(event=event), patch("ci_routing.subprocess.run") as run:
                routes = route_event({"GITHUB_EVENT_NAME": event})
                self.assertTrue(routes.full and routes.benchmarks and routes.distribution)
                run.assert_not_called()

    def test_missing_malformed_and_invalid_event_metadata_fail_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            event = Path(temporary) / "event.json"
            environment = {"GITHUB_EVENT_NAME": "pull_request", "GITHUB_EVENT_PATH": str(event)}
            self.assertTrue(route_event({"GITHUB_EVENT_NAME": "pull_request"}).full)
            self.assertTrue(route_event(environment).full)
            for contents in ("{", "null", "[]", "{}", '{"pull_request": null}'):
                event.write_text(contents, encoding="utf-8")
                self.assertTrue(route_event(environment).full)
            for sha in (None, 2, "-" + "a" * 39, "a" * 39, "g" * 40, "a" * 41, "a\n" * 20):
                event.write_text(json.dumps({"pull_request": {"base": {"sha": sha},
                                                             "head": {"sha": "b" * 40}}}), encoding="utf-8")
                with self.subTest(sha=sha), patch("ci_routing.subprocess.run") as run:
                    self.assertTrue(route_event(environment).full)
                    run.assert_not_called()

    def test_git_failure_and_incomplete_output_fail_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            event = Path(temporary) / "event.json"
            event.write_text(json.dumps({"pull_request": {"base": {"sha": "a" * 40},
                                                         "head": {"sha": "b" * 40}}}), encoding="utf-8")
            environment = {"GITHUB_EVENT_NAME": "pull_request", "GITHUB_EVENT_PATH": str(event)}
            for error in (OSError("git missing"), subprocess.CalledProcessError(128, "git")):
                with self.subTest(error=error), patch("ci_routing.subprocess.run", side_effect=error):
                    self.assertTrue(route_event(environment).full)
            with patch("ci_routing.subprocess.run", return_value=subprocess.CompletedProcess([], 0, b"README.md")):
                self.assertTrue(route_event(environment).full)

    def test_diff_uses_exact_event_shas_and_preserves_newlines_in_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            event = Path(temporary) / "event.json"
            base, head = "a" * 64, "b" * 64
            event.write_text(json.dumps({"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}),
                             encoding="utf-8")
            environment = {"GITHUB_EVENT_NAME": "pull_request", "GITHUB_EVENT_PATH": str(event)}
            result = subprocess.CompletedProcess([], 0, b"CONTRIBUTING.md\nCHANGELOG.md\0")
            with patch("ci_routing.subprocess.run", return_value=result) as run:
                self.assertTrue(route_event(environment).full)
                self.assertEqual(run.call_args.args[0],
                                 ["git", "diff", "--name-only", "-z", "--no-renames", f"{base}...{head}"])

    def test_outputs_and_summary_expose_selection_and_reason(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "output"
            summary = Path(temporary) / "summary"
            routes = route_event({"GITHUB_EVENT_NAME": "push"})
            with redirect_stdout(io.StringIO()) as printed:
                write_routes(routes, {"GITHUB_OUTPUT": str(output), "GITHUB_STEP_SUMMARY": str(summary)})
            self.assertEqual(output.read_text(encoding="utf-8"),
                             "full=true\ndocs=false\nbenchmarks=true\ndistribution=true\n")
            self.assertIn(routes.reasons[0], summary.read_text(encoding="utf-8"))
            self.assertIn(routes.reasons[0], printed.getvalue())


class GitRoutingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.git("init", "--quiet")
        self.git("config", "user.name", "Routing Test")
        self.git("config", "user.email", "routing@example.invalid")
        self.write("compiler/source.md", "content\n")
        self.git("add", ".")
        self.git("-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "base")
        self.base = self.git("rev-parse", "HEAD")

    def git(self, *arguments):
        return subprocess.run(["git", *arguments], cwd=self.directory, check=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout.strip()

    def write(self, name, contents):
        path = self.directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")

    def route(self):
        event = self.directory / "event.json"
        event.write_text(json.dumps({"pull_request": {"base": {"sha": self.base},
                                                     "head": {"sha": self.git("rev-parse", "HEAD")}}}), encoding="utf-8")
        return route_event({"GITHUB_EVENT_NAME": "pull_request", "GITHUB_EVENT_PATH": str(event)}, self.directory)

    def test_rename_into_docs_still_checks_deleted_source_path(self):
        (self.directory / "docs").mkdir()
        self.git("mv", "compiler/source.md", "docs/README.md")
        self.git("-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "rename")
        routes = self.route()
        self.assertTrue(routes.full)
        self.assertTrue(routes.docs)

    def test_valid_metadata_without_changes_still_runs_full_checks(self):
        self.assertTrue(self.route().full)


class GateTests(unittest.TestCase):
    def environment(self, full=False, docs=False, benchmarks=False, distribution=False):
        return {
            "ROUTES_RESULT": "success", "FULL": str(full).lower(), "DOCS": str(docs).lower(),
            "BENCHMARKS": str(benchmarks).lower(), "DISTRIBUTION": str(distribution).lower(),
            "VALIDATION_RESULT": "success" if full or benchmarks else "skipped",
            "TEST_RESULT": "success" if full else "skipped",
            "DOCS_RESULT": "success" if docs and not full else "skipped", "IMPORTS_RESULT": "success",
        }

    def test_each_supported_route_accepts_only_expected_results(self):
        for options in ({"full": True, "benchmarks": True, "distribution": True}, {"docs": True},
                        {"benchmarks": True}, {"distribution": True}, {"docs": True, "benchmarks": True},
                        {"full": True, "docs": True, "benchmarks": True, "distribution": True}):
            with self.subTest(options=options):
                self.assertEqual(gate_errors(self.environment(**options)), [])

    def test_missing_invalid_or_unsuccessful_route_fails_gate(self):
        for name in ("ROUTES_RESULT", "FULL", "DOCS", "BENCHMARKS", "DISTRIBUTION"):
            for value in (None, "", "cancelled", "TRUE", "failure"):
                environment = self.environment(docs=True)
                if value is None:
                    del environment[name]
                else:
                    environment[name] = value
                with self.subTest(name=name, value=value):
                    self.assertTrue(gate_errors(environment))

    def test_empty_or_inconsistent_full_selection_fails_gate(self):
        self.assertTrue(gate_errors(self.environment()))
        for benchmarks, distribution in ((False, False), (True, False), (False, True)):
            with self.subTest(benchmarks=benchmarks, distribution=distribution):
                self.assertTrue(gate_errors(self.environment(full=True, benchmarks=benchmarks,
                                                            distribution=distribution)))

    def test_selected_job_failure_cancellation_or_skip_fails_gate(self):
        for options, names in (({"full": True, "benchmarks": True, "distribution": True},
                                ("VALIDATION_RESULT", "TEST_RESULT", "IMPORTS_RESULT")),
                               ({"docs": True}, ("DOCS_RESULT", "IMPORTS_RESULT"))):
            for name in names:
                for value in ("failure", "cancelled", "skipped", "", None):
                    environment = self.environment(**options)
                    environment[name] = value
                    with self.subTest(name=name, value=value):
                        self.assertTrue(gate_errors(environment))

    def test_unselected_jobs_must_be_intentionally_skipped(self):
        for name in ("VALIDATION_RESULT", "TEST_RESULT", "DOCS_RESULT"):
            for value in ("failure", "cancelled", "success", None):
                environment = self.environment(distribution=True)
                environment[name] = value
                with self.subTest(name=name, value=value):
                    self.assertTrue(gate_errors(environment))

    def test_distribution_result_is_independent(self):
        environment = self.environment(distribution=True)
        environment["DISTRIBUTION_RESULT"] = "failure"
        self.assertEqual(gate_errors(environment), [])


class DistributionGateTests(unittest.TestCase):
    def environment(self, selected=True):
        result = "success" if selected else "skipped"
        return {"ROUTES_RESULT": "success", "DISTRIBUTION": str(selected).lower(),
                "METADATA_RESULT": result, "ARCHIVES_RESULT": result, "BUILDER_RESULT": result}

    def test_selected_and_unselected_routes_pass(self):
        for selected in (True, False):
            with self.subTest(selected=selected):
                self.assertEqual(distribution_gate_errors(self.environment(selected)), [])

    def test_routing_failure_and_invalid_output_fail(self):
        for name in ("ROUTES_RESULT", "DISTRIBUTION"):
            for value in ("failure", "cancelled", "skipped", "TRUE", "", None):
                environment = self.environment()
                environment[name] = value
                with self.subTest(name=name, value=value):
                    self.assertTrue(distribution_gate_errors(environment))

    def test_selected_jobs_must_succeed_and_unselected_jobs_must_skip(self):
        for selected in (True, False):
            for name in ("METADATA_RESULT", "ARCHIVES_RESULT", "BUILDER_RESULT"):
                for value in ("success", "skipped", "failure", "cancelled", "", None):
                    if value == ("success" if selected else "skipped"):
                        continue
                    environment = self.environment(selected)
                    environment[name] = value
                    with self.subTest(selected=selected, name=name, value=value):
                        self.assertTrue(distribution_gate_errors(environment))


if __name__ == "__main__":
    unittest.main()
