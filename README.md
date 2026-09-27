# Zore

**Simple code. Strong guarantees.**

Zore is a native language with borrowing by default, deterministic cleanup, and
one ownership model for synchronous code, async computations, tasks, and channels.

The [language specification](spec/language-spec.md) defines the locked MVP.
The M0 command-line driver is implemented: help, version, argument validation,
and explicit unsupported-command errors. No Zore programs can be checked or
compiled yet; compiler commands report this and exit unsuccessfully. Async and concurrency remain part of the full MVP.

## Development setup

Install Rust through [rustup](https://rustup.rs/). The repository toolchain file
pins Rust 1.98.1 with rustfmt and Clippy. No LLVM installation or third-party
Rust dependencies are required for the workspace or initial frontend work.

If Cargo is not on your shell's PATH after installation, run
`source "$HOME/.cargo/env"` in your terminal.

```sh
cargo build --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

Commit `Cargo.lock`; update it deliberately when adding dependencies. CI runs the same
format, lint, build, and test checks on Linux and macOS.

## Start here

- [AGENTS.md](AGENTS.md): implementation constraints for coding agents.
- [CONTRIBUTING.md](CONTRIBUTING.md): workflow and validation expectations.
- [Architecture](docs/architecture.md): compiler boundaries and design direction.
- [Roadmap](docs/roadmap.md): milestones, dependencies, and acceptance criteria.
- [Spec questions](docs/spec-questions.md): unresolved decisions and affected work.
- [Testing](tests/README.md): planned test layers and fixture conventions.

`examples/hello` reproduces the basic program from spec §3.2.
`examples/semantic-target` reproduces the first semantic target from §42.
Both pass `zore check`; neither can be built or run yet.

## Current CLI

```sh
cargo run -- --help
cargo run -- --version
cargo run -- check --help
cargo run -- check examples/hello/main.ore
```

`check` lexes, parses, resolves, and type-checks one file as a whole package.
It prints nothing and exits 0 for a valid file, or prints diagnostics to stderr
and exits 1. Only a subset of the language is supported so far (see
`docs/architecture.md`); unsupported features are reported as errors, never
accepted. `build`, `run`, `fmt`, and `test` report unsupported operations;
their targets are not read or modified. Each command currently requires
exactly one target. Use `--` before a target starting with `-`.

Help (`-h`/`--help`) and version (`-V`/`--version`) write to stdout and exit 0.
Unsupported operations write to stderr and exit 1; invalid CLI arguments write
to stderr and exit 2. These are bootstrap driver conventions, not Zore program
exit semantics. Paths are retained as native OS paths, including non-UTF-8 paths
on Unix.

The frontend (source manager, diagnostics, lexer, parser, resolver, type
checker, and HIR) is available as library APIs and covered by integration
tests. There is no MIR, ownership analysis, backend, or runtime yet. The next
work is the minimal native path for the §42 semantic target; see
`docs/roadmap.md`.
