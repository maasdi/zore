# 0001 — Native backend: textual LLVM IR compiled by clang

Status: accepted (implementation choice; not a language rule).

## Context

Spec §2.2 requires LLVM as the initial backend, and §45 requires `check` to work
without it. The semantic checkpoint (`examples/semantic-target`) must now run
natively and print `Maas`. The development host has Apple clang 17 but no
`llvm-config` or LLVM development libraries. CI runs on `ubuntu-latest` and
`macos-latest`.

## Decision

- The compiler lowers HIR to MIR (§33) and emits **textual LLVM IR** (`.ll`).
- `zore build` and `zore run` invoke an external **clang** to optimize, compile,
  and link that IR together with a small **C runtime** embedded in the compiler
  (`runtime/zore_runtime.c`). No LLVM library is linked into `zore`.
- clang is found through the `ZORE_CC` environment variable, falling back to
  `clang` on `PATH`. Invocation: `clang -O2 -Wno-override-module program.ll
  zore_runtime.c -o <output>`.
- The IR uses opaque pointers (`ptr`) and names no target triple or data
  layout, so clang supplies the host's.

## Alternatives considered

- **LLVM C API through `inkwell`/`llvm-sys`.** In-process and faster, but it
  pins one LLVM version through Cargo features, needs LLVM development libraries
  on every build machine (including CI), and makes building `zore` itself depend
  on LLVM. Reconsider when compile speed or in-memory JIT matters (M32).
- **Another code generator (e.g. Cranelift).** Contradicts §2.2.
- **Runtime in Rust or in LLVM IR.** A Rust runtime needs a staticlib build and
  target handling; hand-written IR is hard to maintain. C is compiled by the
  same clang with no extra tooling, and can later be replaced by Zore code.

## Runtime ABI (internal, unstable)

| Symbol | Purpose |
| --- | --- |
| `zore_entry()` | Emitted by the compiler; calls the §3.19 `main` |
| `main` | In the runtime: ignores `SIGPIPE`, calls `zore_entry`, returns 0 |
| `zore_println_str(ptr, i64)`, `zore_println_i64`, `zore_println_u64`, `zore_println_bool(i1)`, `zore_println_rune(i32)` | §37.1 output: one `write` per line; a failed write panics |
| `zore_string_compare(ptr, i64, ptr, i64) -> i32` | Byte-wise UTF-8 ordering (§6.6) |
| `zore_panic(ptr, i64)` | Reports `panic in the main task: <message>` on stderr and exits with status 2 |

Values use these LLVM types: `bool` is `i1`; integers are `iN`; `float32`/`float64`
are `float`/`double`; `rune` is `i32`; `string` is `{ ptr, i64 }` pointing at
immutable UTF-8 bytes; structs are named LLVM struct types in declaration
order; multiple results are returned as an anonymous struct. All accepted types
are Copy, so parameters (default or `own`) are passed by value; passing shared
borrows by address is deferred until Move types exist.

Checked operations (§6.6) are implemented in code generation: integer
overflow, division by zero, invalid shift counts, and out-of-range conversions
branch to `zore_panic` with a message naming the source location. Because no
accepted type has a destructor, a panic has nothing to unwind and exits
directly; explicit MIR assert and cleanup paths arrive with drop insertion
(M18). The exit status 2 for a panic is the implementation-defined nonzero
status allowed by §3.19 (it matches Go).

## Consequences

- `zore build`/`run` require clang with LLVM 15 or newer (opaque pointers).
  Verified with Apple clang 17 (arm64 macOS); CI uses the clang preinstalled on
  its images.
- `zore check` and all frontend tests still need no backend.
- Native tests (`tests/native.rs`) fail with an explicit message when clang is
  missing rather than being skipped, so missing coverage is visible.
- Runtime string concatenation needs an allocation and ownership strategy for
  string buffers (§41.5); until one is chosen, `build` reports it as
  unsupported. Constant concatenation is folded at compile time.
- Float output needs the `println` text format (§37.1, TBD); until then `build`
  reports printing a float as unsupported. Float arithmetic compiles normally.

## Validation

`cargo test --locked --all-targets` runs `tests/native.rs`, which builds and
executes programs and compares stdout, stderr, and exit status, including
panics and a closed standard output.
