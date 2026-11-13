import argparse
from dataclasses import dataclass, field
import json
import os
from pathlib import Path
import re
import subprocess


ROUTE_NAMES = ("full", "docs", "benchmarks", "distribution")
DISTRIBUTION_SCRIPTS = frozenset({
    ".github/scripts/package_release.py",
    ".github/scripts/smoke_release.py",
    ".github/scripts/smoke_builder.py",
    ".github/scripts/test_release.py",
})
EDITORIAL_DOCUMENTS = frozenset({
    "CHANGELOG.md", "CONTRIBUTING.md", "docs/README.md", "docs/alpha.md",
    "docs/release-workflow.md",
})


@dataclass
class Routes:
    full: bool = False
    docs: bool = False
    benchmarks: bool = False
    distribution: bool = False
    reasons: list[str] = field(default_factory=list)

    def require_full(self, reason):
        self.full = True
        self.benchmarks = True
        self.distribution = True
        if reason not in self.reasons:
            self.reasons.append(reason)

    def outputs(self):
        return {name: str(getattr(self, name)).lower() for name in ROUTE_NAMES}


def classify_paths(paths):
    routes = Routes()
    if not paths:
        routes.require_full("The pull request diff is empty; running all checks conservatively.")
        return routes
    for path in paths:
        if path in DISTRIBUTION_SCRIPTS or path.startswith("distribution/"):
            routes.distribution = True
        elif path.startswith("benchmarks/") and path.endswith(".py"):
            routes.benchmarks = True
        elif path in EDITORIAL_DOCUMENTS:
            routes.docs = True
        else:
            routes.require_full("The diff includes a source, semantic, workflow, or unrecognized path.")
    if not routes.full:
        routes.reasons.append("Every changed path belongs to an explicitly supported check group.")
    return routes


def route_event(environment, directory=None):
    routes = Routes()
    if environment.get("GITHUB_EVENT_NAME") != "pull_request":
        routes.require_full("Only pull request events use conditional routing.")
        return routes
    try:
        event_path = environment.get("GITHUB_EVENT_PATH")
        if not event_path:
            raise ValueError("GITHUB_EVENT_PATH is missing")
        event = json.loads(Path(event_path).read_text(encoding="utf-8"))
        pull_request = event["pull_request"]
        base = pull_request["base"]["sha"]
        head = pull_request["head"]["sha"]
        if not all(isinstance(sha, str) and re.fullmatch(r"(?:[0-9a-fA-F]{40}|[0-9a-fA-F]{64})", sha)
                   for sha in (base, head)):
            raise ValueError("pull request base/head SHAs are missing or invalid")
        result = subprocess.run(
            ["git", "diff", "--name-only", "-z", "--no-renames", f"{base}...{head}"],
            cwd=directory, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        if result.stdout and not result.stdout.endswith(b"\0"):
            raise ValueError("git diff returned an incomplete path list")
        paths = [os.fsdecode(path) for path in result.stdout.split(b"\0") if path]
        return classify_paths(paths)
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        detail = "git diff failed or the required commits are unavailable" if isinstance(
            error, subprocess.CalledProcessError) else f"{type(error).__name__}: {error}"
        routes.require_full(f"Pull request diff could not be established ({detail}); running all checks.")
        return routes


def write_routes(routes, environment):
    outputs = routes.outputs()
    output_text = "".join(f"{name}={value}\n" for name, value in outputs.items())
    print(output_text, end="")
    for reason in routes.reasons:
        print(reason)
    if environment.get("GITHUB_OUTPUT"):
        with Path(environment["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
            output.write(output_text)
    if environment.get("GITHUB_STEP_SUMMARY"):
        with Path(environment["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as summary:
            summary.write("## CI routing\n\n| Check group | Selected |\n| --- | --- |\n")
            for name, value in outputs.items():
                summary.write(f"| {name} | {value} |\n")
            summary.write("\n" + "\n".join(routes.reasons) + "\n")


def gate_errors(environment):
    errors = []
    if environment.get("ROUTES_RESULT") != "success":
        errors.append("The routing job did not succeed.")
    routes = {}
    for name in ROUTE_NAMES:
        value = environment.get(name.upper(), environment.get(f"ROUTES_{name.upper()}"))
        if value not in ("true", "false"):
            errors.append(f"The {name} routing output is missing or invalid.")
        routes[name] = value == "true"
    if not any(routes.values()):
        errors.append("The routing outputs select no check group.")
    if routes["full"] and not (routes["benchmarks"] and routes["distribution"]):
        errors.append("Full routing must also select benchmarks and distribution.")
    selected = {
        "VALIDATION_RESULT": routes["full"] or routes["benchmarks"],
        "TEST_RESULT": routes["full"],
        "DOCS_RESULT": routes["docs"] and not routes["full"],
        "IMPORTS_RESULT": True,
    }
    for name, required in selected.items():
        expected = "success" if required else "skipped"
        actual = environment.get(name)
        if actual != expected:
            errors.append(f"{name} must be {expected}, received {actual or 'missing'}.")
    return errors


def distribution_gate_errors(environment):
    errors = []
    if environment.get("ROUTES_RESULT") != "success":
        errors.append("The routing job did not succeed.")
    distribution = environment.get("DISTRIBUTION")
    if distribution not in ("true", "false"):
        errors.append("The distribution routing output is missing or invalid.")
    expected = "success" if distribution == "true" else "skipped"
    for name in ("METADATA_RESULT", "ARCHIVES_RESULT", "BUILDER_RESULT"):
        actual = environment.get(name)
        if actual != expected:
            errors.append(f"{name} must be {expected}, received {actual or 'missing'}.")
    return errors


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--gate", action="store_true")
    mode.add_argument("--distribution-gate", action="store_true")
    arguments = parser.parse_args()
    if arguments.gate or arguments.distribution_gate:
        errors = distribution_gate_errors(os.environ) if arguments.distribution_gate else gate_errors(os.environ)
        for error in errors:
            print(error)
        if errors:
            return 1
        print("Every selected CI check succeeded and every unselected check was skipped.")
        return 0
    write_routes(route_event(os.environ), os.environ)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
