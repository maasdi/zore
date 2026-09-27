# Zore

[![CI](https://github.com/maasdi/zore/actions/workflows/ci.yml/badge.svg)](https://github.com/maasdi/zore/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**Simple code. Strong guarantees.**

Zore is a natively compiled programming language with ownership-based memory
safety and no garbage collector. Parameters are borrowed by default, resources
are cleaned up deterministically, errors are explicit values, and one ownership
model covers synchronous code, `async`/`await`, lightweight tasks, and
channels.

```ore
package main

type User struct {
    Name string
}

func greet(user User) {
    println(user.Name)
}

func main() {
    let user = User{
        Name: "John",
    }

    greet(user)
}
```

```console
$ zore run examples/semantic-target/main.ore
John
```

> [!NOTE]
> Zore is in early development. The [language specification](spec/language-spec.md)
> is written; the bootstrap compiler implements a growing subset of it. Nothing
> is stable yet.

## Project status

| Area | Status |
| --- | --- |
| Language specification | MVP decisions locked in [`spec/language-spec.md`](spec/language-spec.md) |
| Lexer, parser, diagnostics | Implemented, with error recovery |
| Name resolution and type checking | Implemented for a subset: one file per package; `bool`, integer and float types, `rune`, `string`, and structs of those; functions, control flow, and Go-style untyped constants |
| Native code generation | Implemented for that subset: MIR lowering, LLVM IR, and runtime checks for overflow, division by zero, shifts, and conversions |
| Ownership and borrow checking, drop insertion | Planned (next major milestone) |
| Methods, errors and `?`, collections, closures | Planned |
| `async`/`await`, tasks, channels | Planned (part of the MVP) |
| Self-hosting | Long-term goal |

Features outside the implemented subset are reported as errors, never silently
accepted. The [roadmap](docs/roadmap.md) has the details.

## Getting started

You need:

- **Rust**, installed through [rustup](https://rustup.rs/). The repository pins
  the toolchain in `rust-toolchain.toml`, so rustup selects it automatically.
- **clang with LLVM 15 or newer**, for `zore build` and `zore run` only. macOS
  ships it with the Xcode Command Line Tools; on Linux, install your
  distribution's `clang` package. Set `ZORE_CC` to use a specific compiler.

```sh
git clone https://github.com/maasdi/zore.git
cd zore
cargo build --release
./target/release/zore run examples/hello/main.ore
```

## Usage

```sh
zore check main.ore   # check a file; prints nothing when it is valid
zore build main.ore   # compile to ./main
zore run main.ore     # build into a temporary directory and run
zore --help
```

Each command takes one source file, which is treated as a whole package.
Diagnostics are printed to standard error with source locations, and the
command exits with status 1. A program that panics reports the panic on
standard error and exits with status 2; `zore run` passes the program's exit
status through. Invalid command-line arguments also exit with status 2. `fmt`
and `test` are not implemented yet.

## Repository layout

| Path | Contents |
| --- | --- |
| [`spec/`](spec/language-spec.md) | The language specification (authoritative) |
| [`src/`](src) | The bootstrap compiler, written in Rust |
| [`runtime/`](runtime) | The minimal C runtime linked into programs |
| [`examples/`](examples) | Example programs |
| [`tests/`](tests) | Integration tests and conformance cases |
| [`docs/`](docs/README.md) | Architecture, roadmap, decision records, open questions |

## Documentation

- [Language specification](spec/language-spec.md)
- [Documentation index](docs/README.md): architecture, roadmap, decision
  records, and open specification questions
- [Testing guide](tests/README.md)
- [Changelog](CHANGELOG.md)

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and
follow the [Code of Conduct](CODE_OF_CONDUCT.md). Report security issues
privately as described in [SECURITY.md](SECURITY.md).

## License

Zore is licensed under the [Apache License, Version 2.0](LICENSE). Unless you
state otherwise, any contribution you submit is licensed under the same terms,
as described in section 5 of the license.
