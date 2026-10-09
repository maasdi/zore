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
| Name resolution and type checking | Implemented for the supported synchronous and async subset: primitives, `error`, structs, collections, slices, functions and methods, closures, tasks, channels, mutexes, control flow, and Go-style untyped constants |
| Native code generation | Implemented for that subset: LLVM IR with checked arithmetic and bounds, persistent async state machines, and a linked Rust runtime |
| Ownership, borrowing, and cleanup | Implemented: Copy/Move classification, partial moves, shared and `mut` borrows, inferred regions for stored views, and cleanup on normal, error, and panic paths; task inputs use conservative borrow restrictions |
| Errors and `?` | Implemented for synchronous and async code, including propagation after awaiting async calls or task results |
| Strings | Length, indexing, slicing, loops by character, `+`, and `string(rune)` work |
| Collections | `[T; N]`, `Array<T>`, `map[K]V`, and `[]T` work; `clone` works for structs and collections; `len`, `push`, `pop`, and `for … in` loops work; borrowed map entries are planned |
| Closures and function types | Implemented, including closures that are returned or stored, call-once closures, declared functions and methods as values, and `go` on closures and function-typed locals; async function values are awaited or spawned; a callee stored in a field, element, or map value cannot be spawned directly |
| Packages and imports | Folders are packages; `import "project/folder"` and qualified names work, with exports, import checks, and cycle detection; bundled packages include `zore/strings`, `zore/strconv`, `zore/time`, `zore/io`, `zore/os`, `zore/net`, and `zore/cancel` |
| `async`/`await`, tasks, channels, mutexes | Implemented for the supported operations: polled async tasks, channel send/receive and `select`, mutex acquisition, timers, TCP I/O, files, standard input, and cooperative cancellation. A waiting plain function occupies an OS worker; the runtime starts replacement workers |
| Self-hosting | Long-term goal |

Features outside the implemented subset are reported as errors, never silently
accepted. Recursive owned structs through `Array<T>` and maps are supported,
including cloning and deterministic cleanup; infinite-size by-value cycles are
rejected. Package-level variables are not yet supported. Async frames keep values needed
across a suspension in the pinned heap frame; other locals use poll-local storage,
and some same-type locals share a frame field, but frames are not shrunk by
general liveness. Cooperative budgets yield long-running async loops, but scheduling
has no forced preemption. Full MVP coverage still needs an audit; see the
[roadmap](docs/roadmap.md).

## Getting started

You need:

- **Rust**, installed through [rustup](https://rustup.rs/). The repository pins
  the toolchain in `rust-toolchain.toml`, so rustup selects it automatically.
  Native builds also invoke `rustc` (1.98 or newer) to compile and link the Rust
  runtime; set `ZORE_RUSTC` to choose its executable. The compiled runtime is kept in
  your cache folder (`~/.cache/zore`, `~/Library/Caches/zore` on macOS, or
  `ZORE_CACHE_DIR`), so only the first build pays for compiling it.
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

Each command takes a source file. The file's folder is the program's `main`
package, so every `.ore` file in it is compiled together, and `import` finds
other packages as folders of the project (see `examples/packages`).
Diagnostics are printed to standard error with source locations, and the
command exits with status 1. A program that panics reports the panic on
standard error and exits with status 2; `zore run` passes the program's exit
status through. Invalid command-line arguments also exit with status 2. `fmt`
and `test` are not implemented yet.

## Repository layout

| Path | Contents |
| --- | --- |
| [`spec/`](spec/language-spec.md) | The language specification (authoritative) |
| [`compiler/`](compiler) | The single Rust compiler crate, organized by stage |
| [`runtime/`](runtime) | The Rust runtime linked into native programs |
| [`examples/`](examples/README.md) | One tested, runnable program per supported feature |
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
