"""Compare every match of the shared regex subset with Python's independent regex engine."""

import json
import os
from pathlib import Path
import random
import re
import subprocess
import tempfile


def fragment(rng, depth):
    if depth == 0 or rng.random() < 0.3:
        pattern = rng.choice(["a", "b", "é", "🦀", "[ab]", "[^b]", ".", r"\d", r"\w", r"\D"])
        nullable = False
    else:
        first, first_empty = fragment(rng, depth - 1)
        choice = rng.randrange(3)
        if choice == 0:
            pattern, nullable = "(" + first + ")", first_empty
        else:
            second, second_empty = fragment(rng, depth - 1)
            if choice == 1:
                pattern, nullable = "(?:" + first + "|" + second + ")", first_empty or second_empty
            else:
                pattern, nullable = first + second, first_empty and second_empty
    if not nullable and rng.random() < 0.45:
        quantifier = rng.choice(["*", "+", "?", "{1,3}", "{0,2}"])
        pattern = "(?:" + pattern + ")" + quantifier + rng.choice(["", "?"])
        nullable = quantifier in ("*", "?", "{0,2}")
    return pattern, nullable


def anchored(rng, pattern):
    choice = rng.randrange(6)
    if choice == 0:
        return "^" + pattern
    if choice == 1:
        return pattern + "$"
    if choice == 2:
        return "^" + pattern + "$"
    return pattern


def every_match(compiled, text):
    found = []
    at = 0
    last_nonempty_end = None
    while at <= len(text):
        match = compiled.search(text, at)
        if not match:
            break
        start, end = match.span()
        if start == end:
            if last_nonempty_end != end:
                found.append(match)
            at = end + 1
        else:
            at = end
            last_nonempty_end = end
            found.append(match)
    return found


def spans(match, groups, text):
    offsets = []
    for group in range(groups + 1):
        start, end = match.span(group)
        offsets.extend((-1, -1) if start < 0 else (len(text[:start].encode()), len(text[:end].encode())))
    return ",".join(map(str, offsets))


def cases():
    rng = random.Random(222)
    patterns = {anchored(rng, fragment(rng, 3)[0]) for _ in range(1200)}
    patterns.update([r"(a+)(a)", r"(a+?)(a)", r"\b(a|b)+\b", r"((a)|(b))+", r"^(?:a|b)*$", r"[α-ω]+", r"(a)?(b*)", r"a*", r"", r"(?:a|)"])
    result = []
    for pattern in sorted(patterns):
        compiled = re.compile(pattern[:-1] + r"\Z" if pattern.endswith("$") else pattern, re.ASCII)
        if compiled.groups > 16:
            continue
        for _ in range(20):
            text = "".join(rng.choices("aabé🦀012_\n", k=rng.randrange(13)))
            expected = ";".join(spans(match, compiled.groups, text) for match in every_match(compiled, text))
            result.append((pattern, text, expected))
    return result


def main():
    root = Path(__file__).resolve().parents[2]
    modules = "\n".join(
        f"#[path = {json.dumps((root / 'runtime/src' / (module + '.rs')).as_posix(), ensure_ascii=False)}]\nmod {module};"
        for module in ("regexp_parse", "regexp_vm")
    )
    probe = modules + r'''
use std::io::BufRead;
fn decode(value: &str) -> String {
    String::from_utf8((0..value.len()).step_by(2)
        .map(|at| u8::from_str_radix(&value[at..at+2], 16).unwrap()).collect()).unwrap()
}
fn main() {
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let (pattern, input) = line.split_once('\t').unwrap();
        let (pattern, input) = (decode(pattern), decode(input));
        let program = regexp_vm::Program::compile(&pattern).unwrap();
        let found = program.find_all(&input, regexp_vm::MAX_MATCHES, &mut regexp_vm::Budget::new(input.len())).unwrap();
        println!("{}", found.iter().map(|capture| program.offsets(capture)
            .map(|at| at.to_string()).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join(";"));
    }
}
'''
    generated = cases()
    with tempfile.TemporaryDirectory(prefix="zore-regexp-oracle-") as folder:
        source = Path(folder) / "probe.rs"
        executable = Path(folder) / ("probe.exe" if os.name == "nt" else "probe")
        source.write_text(probe, encoding="utf-8")
        subprocess.run([os.environ.get("ZORE_RUSTC", "rustc"), "--edition=2024", "-O", "-Adead_code", str(source), "-o", str(executable)], check=True, timeout=120)
        payload = "".join(pattern.encode().hex() + "\t" + text.encode().hex() + "\n" for pattern, text, _ in generated)
        output = subprocess.run([str(executable)], input=payload, text=True, capture_output=True, check=True, timeout=60)
    lines = output.stdout.splitlines()
    if len(lines) != len(generated):
        raise AssertionError(f"expected {len(generated)} oracle results, got {len(lines)}")
    differences = [(pattern, text, expected, actual) for (pattern, text, expected), actual in zip(generated, lines) if expected != actual]
    if differences:
        raise AssertionError(f"{len(differences)} regex oracle differences: {differences[:10]!r}")
    print(f"{len(generated)} regex oracle comparisons passed")


if __name__ == "__main__":
    main()
