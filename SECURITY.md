# Security policy

## Supported versions

Zore is pre-release. Only the latest commit on the `main` branch is supported;
there are no released versions yet.

## Scope

Please report privately anything that could undermine the guarantees Zore
programs rely on, for example:

- the compiler accepting a program that violates the language's safety rules
  (spec §23) or generating code that does,
- a missing or wrong runtime check (integer overflow, division by zero, shift
  counts, conversions; spec §6.6),
- memory-safety bugs in the runtime (`runtime/zore_runtime.c`) or in the
  compiler itself,
- problems in how `zore build`/`zore run` handle files, temporary directories,
  or the external C compiler.

Ordinary compiler crashes and wrong diagnostics are regular bugs; please open
an issue for those.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting:
<https://github.com/maasdi/zore/security/advisories/new>.
Do not open a public issue for a suspected vulnerability.

Include the Zore commit (`zore --version` and `git rev-parse HEAD`), the host
platform, the clang version for build problems, and a minimal program or steps
that reproduce the issue. You should receive a response within 7 days. Once a
fix is available, the advisory will be published with credit to the reporter
unless you prefer otherwise.
