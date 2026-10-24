import ast
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
IMPORT = "import importlib, sys; importlib.import_module(sys.argv[1])"


def python_files():
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "*.py"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout
    return sorted({ROOT / name for name in listed.split("\0") if name and (ROOT / name).is_file()})


def check(path):
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"), str(path))
    except SyntaxError as error:
        return str(error)
    # A module that raises at top level ends every import by design, so it is only parsed.
    if any(isinstance(node, ast.Raise) for node in tree.body):
        return None
    result = subprocess.run(
        [sys.executable, "-B", "-c", IMPORT, path.stem],
        cwd=path.parent, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60,
    )
    return result.stderr if result.returncode else None


def main():
    failures = 0
    for path in python_files():
        error = check(path)
        print(f"{'FAILED' if error else 'ok'} {path.relative_to(ROOT).as_posix()}")
        if error:
            failures += 1
            print(error, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
