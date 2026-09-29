# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Zore has no releases
yet; version numbers will follow [Semantic Versioning](https://semver.org/)
once it does.

## [Unreleased]

### Language specification

- Locked the program entry point: `package main` with exactly one
  `func main()`; exit status 0 on return, nonzero after a panic (§3.19).
- Locked `println`: one printable argument, one line per call (§37.1).
- Locked untyped constants following Go's model: integer and float kinds,
  exact arithmetic, representability rules, and at least 256-bit precision
  (§6.7).

### Compiler

- `zore` command-line driver with `check`, `build`, and `run`; `fmt` and
  `test` report that they are not implemented.
- Source manager with stable file IDs, byte spans, and source-aware diagnostics.
- Lexer for the locked lexical rules, including automatic semicolon insertion,
  string/rune escapes, numeric literals, and error recovery.
- Parser for packages, functions, structs, bindings, statements, and the full
  operator table, with recovery; later-milestone syntax is reported as
  unsupported.
- Name resolution with stable IDs, and type checking into typed HIR for a
  single-file, synchronous, all-Copy subset: `bool`, integer and float types,
  `rune`, `string`, and structs of those.
- Exact constant evaluation with hand-written arbitrary-precision arithmetic
  and correctly rounded float conversion.
- MIR lowering and an LLVM IR backend compiled by clang, with runtime checks
  for overflow, division by zero, shift counts, and conversions; a minimal C
  runtime (decision record 0001).
- The first semantic target (spec §42) builds and prints `John`.

### Not yet supported

Ownership and borrow checking, drop insertion, `error` and `?`,
collections, closures, async/await, tasks, channels, imports, runtime string
concatenation, and printing floats. See [the roadmap](docs/roadmap.md).

[Unreleased]: https://github.com/maasdi/zore/commits/main
