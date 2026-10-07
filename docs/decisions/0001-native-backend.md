# 0001 — Native backend: LLVM objects linked with a Rust runtime

Status: accepted (implementation choice; not a language rule). Updated to
replace the original C runtime with Rust, following `compiler-structure.md`.
Validation of this migration is pending on a host with Rust and clang.

## Context

Spec §2.2 requires LLVM as the initial backend, and §45 requires `check` to work
without it. The original backend used textual LLVM IR and a small C runtime
compiled together by clang. The runtime is now Rust, as directed by the
structure guide. This changes implementation and build prerequisites, not
source-language semantics.

## Decision

- The compiler lowers HIR to MIR (§33) and emits textual LLVM IR (`.ll`).
- `zore build` and `zore run` invoke clang to compile that IR to a native
  object, then rustc to link a small entry shim and the object against the
  embedded Rust runtime, which is compiled once as a library and cached (see the
  architecture notes).
- clang is selected by `ZORE_CC`, falling back to `clang` on `PATH`.
  Invocation: `clang -O2 -Wno-override-module -fPIC -c program.ll -o program.o`.
- rustc is selected by `ZORE_RUSTC`, falling back to `rustc` on `PATH`.
  It compiles the runtime library, and the entry shim `main.rs` against it, with
  edition 2024, optimization level 2, and `panic=abort`, using clang as the
  linker driver and passing the object
  with `-C link-arg=<program.o>`. Rustc manages its standard-library and native
  library dependencies; see the
  [Rust linkage reference](https://doc.rust-lang.org/reference/linkage.html).
- The IR uses opaque pointers (`ptr`) and no target triple or data layout;
  clang supplies the host's. Both compilers must target the same host.
- The compiler embeds all files from `runtime/src/` needed by the entry shim.
  Installed compiler binaries do not need the repository beside them.
- The workspace's `zore-runtime` library target supports independent tests and
  linting. Its `main.rs` is not a Cargo binary: it requires a generated
  `zore_entry` symbol and is compiled only by the native build driver.
  The compiler has no dependency on the runtime crate or LLVM libraries.

## Alternatives considered

- **LLVM C API through inkwell/llvm-sys.** Requires LLVM development libraries
  and pins a backend version. Reconsider when in-process compilation is needed.
- **Another code generator.** Contradicts §2.2.
- **C runtime (previous implementation).** Used the same clang invocation for
  IR and C, avoiding a second compiler at native-build time. Replaced with Rust
  to follow the architecture guide.
- **Rust staticlib linked directly by clang.** Requires maintaining the runtime's
  native library dependencies per host. Rustc now manages the final link.
- **Runtime in LLVM IR.** Harder to maintain than Rust.

## Runtime ABI (internal, unstable)

| Symbol | Purpose |
| --- | --- |
| `zore_entry()` | Emitted by the compiler; calls the §3.19 main function |
| Process entry | Rust startup initializes the process; runtime main calls zore_entry |
| `zore_println_str(ptr, i64)`, `zore_println_i64(i64)`, `zore_println_u64(i64)`, `zore_println_bool(i1 zeroext)`, `zore_println_rune(i32)` | §37.1 output; complete-line locking, explicit flush, failure becomes panic |
| `zore_string_compare(ptr, i64, ptr, i64) -> i32` | Byte-wise UTF-8 ordering (§6.6), including embedded NUL bytes |
| `zore_panic(ptr, i64)` | Reports panic in the main task on stderr and exits with status 2 |

Values use these LLVM types: bool is `i1`; integers are `iN`;
float32/float64 are `float`/`double`; rune is `i32`; string is
`{ ptr, i64 }` pointing at immutable UTF-8 bytes; structs are named LLVM struct
types in declaration order. Multiple results use an anonymous struct.
All accepted types are Copy, so parameters are currently passed by value;
borrows by address await Move types.

Only the runtime ABI boundary uses unsafe Rust. Generated code must supply
live immutable buffers of the declared length and valid scalar values. Empty
strings may have null pointers; they are handled without constructing a Rust
slice from null. This introduces no Zore pointers or unsafe syntax.

Rust startup ignores SIGPIPE on the supported Unix hosts, allowing failed
writes to reach Zore's panic handling; see
[Rust's SIGPIPE default](https://doc.rust-lang.org/beta/nightly-rustc/src/rustc_session/config/sigpipe.rs.html).
The runtime locks stdout across each complete line and flushes before returning.
Allocation for output uses fallible reservation. Panic reporting is best effort
if stderr also fails.

Checked arithmetic and conversion failures still call `zore_panic` with source
locations. Because accepted values have no destructors, it exits directly.
Zore drop unwinding and task-local panic handling must arrive before Move values
and tasks are accepted. Rust's `panic=abort` handles internal Rust failures;
it is not the implementation of Zore's future unwind semantics.

## Consequences and validation

Native builds now require clang with LLVM 15+ and rustc 1.98+ on compatible
Linux/macOS hosts. Cross-compilation and Windows native linking are unverified.
Runtime sources are compiled per build; artifact caching is deferred.
Already-built Zore programs do not invoke rustc or clang.

`zore check` and frontend tests invoke neither tool. Root-level Cargo commands
include the compiler and runtime library, with no third-party dependencies.
`cargo test --locked --all-targets` exercises runtime unit tests and native
tests in `tests/codegen/native.rs`: output formats, numeric boundaries,
empty/long/NUL-containing strings, string ordering, arithmetic panics, closed
stdout, paths with spaces, CLI builds, and missing-tool diagnostics.

The migration's formatting, Clippy, build, and executable tests remain
unverified locally because this host has no Cargo/Rust toolchain. Linux/macOS
CI must pass before the migration is considered validated.

Runtime string concatenation still awaits allocation/ownership design (§41.5).
Float printing still awaits its text-format decision (§37.1). Async, channels,
drop insertion, and task scheduling are not added by this migration.
