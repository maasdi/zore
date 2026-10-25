# Contributing to Zore

Thanks for your interest in Zore. This guide explains how to set up the
project, what a change needs before it can be merged, and how language
decisions are made.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
Report security issues privately as described in [SECURITY.md](SECURITY.md).

## Setup

- Install Rust through [rustup](https://rustup.rs/). `rust-toolchain.toml` pins
  the toolchain (with rustfmt and Clippy), and rustup selects it
  automatically. If Cargo is not on your `PATH`, run `source "$HOME/.cargo/env"`.
  Native builds invoke rustc 1.98 or newer too; `ZORE_RUSTC` selects its executable. The compiled runtime is cached under
  `~/.cache/zore` (`ZORE_CACHE_DIR` overrides it); delete that folder to force a rebuild.
- Install clang with LLVM 15 or newer. It is needed by `zore build`/`zore run`
  and by `tests/codegen/native.rs`, which fails with a clear message when clang is
  missing. Set `ZORE_CC` to choose a specific compiler.

The compiler and runtime have no third-party Rust dependencies. Checking does
not invoke native tools; native builds require host-compatible rustc and clang
on Linux or macOS (cross-compilation and Windows native linking are unverified).
The compiler does not link against LLVM; see
[decision record 0001](docs/decisions/0001-native-backend.md).

## Before you open a pull request

Run the same checks as CI:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
cargo test --locked --all-targets
```

If you change Python files, also run `python .github/scripts/import_modules.py`
and `python -m unittest discover -s benchmarks/<harness> -p 'test_*.py'` for
each harness you touched. CI runs both on Linux, macOS and Windows.

If you could not run a check, say so in the pull request. Track `Cargo.lock`
and change it deliberately. A new dependency needs a justification in the pull
request, and the frontend must stay usable without LLVM.

## How work is organized

1. **The specification is authoritative.** Read the relevant sections of
   [`spec/language-spec.md`](spec/language-spec.md) before implementing
   anything. Normative rules override examples. Do not infer syntax or
   semantics from Go, Rust, or illustrative examples.
2. **Language changes come first in the spec.** Follow the change policy in
   spec §53: update the specification, mark the decision as locked, and add
   examples, ownership/error/async implications, compiler impact, and
   conformance cases before implementation. To start a discussion, open a
   *Language question or proposal* issue. Internal implementation choices need
   a rationale and tests, not a spec change; record substantial ones under
   [`docs/decisions/`](docs/decisions).
3. **Record open questions.** Ambiguities go in
   [`docs/spec-questions.md`](docs/spec-questions.md). Isolate blocked
   features and report them as unsupported rather than guessing.
4. **Pick work from the [roadmap](docs/roadmap.md)** and keep status honest: a
   scaffold is not an implemented stage, and unsupported programs must never
   be reported as successfully checked or built.
5. **Keep compiler stages separate** (parsing, resolution, typing, ownership,
   lowering, code generation), and preserve source spans throughout. See
   [the architecture](docs/architecture.md).

[AGENTS.md](AGENTS.md) condenses these rules. It is written for coding agents
but applies to every contributor.

## Tests

Add focused tests with every compiler change, and pair accepted behavior with
rejection cases. [`tests/README.md`](tests/README.md) describes the test
suites and the conformance documents. Never describe unrun tests or pending
conformance cases as passing.

## Commits and pull requests

- Keep each change focused on one milestone or topic.
- Write commit messages with a short imperative summary line (for example,
  "Add M2 tokens and lexer") and a body explaining what changed and why.
- Include spec references for semantic changes, and explain any conservative
  restrictions.
- Update the affected documentation (README, architecture, roadmap,
  `tests/README.md`, changelog) when behavior, commands, prerequisites, or
  status change.
- Fill in the pull request template's checklist.

## Stacked and parallel pull requests

A stacked pull request builds on another open pull request instead of `main`.

- Open it against its parent's branch so the diff shows only its own change,
  and name the parent pull request in the description.
- Once the parent merges, base the child on `main`. GitHub retargets it
  automatically when the parent's branch is deleted on merge; otherwise use
  **Edit** next to the title or `gh pr edit <number> --base main`. Then merge or
  rebase current `main` into it and let CI run again.
- Before merging any pull request, check that its base branch is `main`.
  Merging into another pull request's branch does not put the change on `main`.
- If a change was merged into a parent branch by mistake, open a new pull
  request from the same branch against `main` to re-land it, and link the
  original.
- Pull requests that pass CI separately can still fail together. Before merging
  one that was developed alongside others, update it with current `main` and
  wait for CI to pass.

## License

By contributing, you agree that your contributions are licensed under the
[Apache License, Version 2.0](LICENSE), as described in section 5 of the
license.
