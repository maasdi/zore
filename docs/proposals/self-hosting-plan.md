# Staged self-hosting plan

Status: planning proposal for issue #56. No new source syntax, library API, or
compiler behavior is accepted by this document. The Rust compiler remains the
reference and bootstrap compiler throughout the stages below.

Progress: stages 1 and 2 are done.

- Stage 1 (issue #79): `compiler-zore/lexer` implements every token form of the
  Rust lexer, and the `selfhost_lexer` test compares their records exactly.
- Stage 2: `compiler-zore/{source,diagnostic,ast,parser}` add a source manager,
  diagnostic records with rendering, an indexed syntax tree, and a parser that
  ports `compiler/src/parser` with its recovery. The `selfhost_parser` test
  compares the complete tree with byte spans, every parser diagnostic with its
  labels, notes, and rendered text, and the source manager's line and column
  answers.
- The inputs include the parser, lexer, and source-diagnostic test literals,
  the conformance documents, the specification's code blocks, every `.ore`
  file, and seeded generated and edited programs.

`compiler-zore/README.md` lists the commands, the record formats, and the one
thing not compared: lexer diagnostic display text.

Stage 3 still needs:

- name resolution and type checking in Zore
- a decision on how the Zore frontend loads a multi-file project; this requires
  the directory and path APIs listed below
- a comparison of accepted and rejected programs and their structured
  diagnostics

Specification §38 sets a long-term goal, not an MVP requirement. The smallest
useful first deliverable is a Zore lexer that processes in-memory source bytes
and can be compared with the Rust lexer. A compiler executable and a
self-compilation cycle come much later.

## Capability map

The representations here use concrete structs, integer IDs, `Array<T>`, maps,
and explicit tag fields. They do not assume user-defined generics, traits,
enums, pattern matching, raw pointers, FFI, or package globals. A tag plus
payload fields may use more memory than Rust enums; validate that cost at each
stage. These are implementation sketches, not required language types.

| Rust subsystem and evidence | Candidate Zore representation | Dependency or risk |
| --- | --- | --- |
| `compiler/src/source/{source_file,source_map,span}.rs` | `FileId` as `int`; owned source text or bytes in an `Array<SourceFile>`; byte-offset spans with file ID | `std/os.ReadFile` exists; preserve UTF-8 validation and stable IDs before diagnostics |
| `compiler/src/diagnostic/{diagnostic,label,renderer}.rs` | Diagnostic records with severity tag, primary span, and `Array<Label>`; render after sorting | Structured comparison of severity, code/message, spans, and related labels is needed before comparing display text |
| `compiler/src/lexer/` and `compiler/src/parser/`, `compiler/src/ast/` | Byte-indexed scanner; tokens with kind tag and span; AST node IDs in indexed arenas | Parser variants need explicit tags; lexer can avoid recursive data entirely |
| `compiler/src/resolve/` and `compiler/src/types/` | Interned symbol/type IDs; scope stack and maps from names to IDs; concrete type records and indexed child arrays | Check map key/value support and borrow rules against actual code before porting; `types/bignum.rs` needs its own bounded milestone |
| `compiler/src/hir/` and `compiler/src/mir/` | Typed IDs and separate arrays of expressions, statements, functions, blocks, places, projections, operands, and terminators | Explicit IDs keep stage boundaries; avoid encoding live borrows inside long-lived arena nodes |
| `compiler/src/ownership/`, `compiler/src/dropck/`, `compiler/src/async_lowering/` | Worklist arrays and indexed loan, move-state, drop, and suspension records | Most proof-sensitive stages; compare accept/reject, loan diagnostics, cleanup, and runtime behavior independently |
| `compiler/src/codegen/` and `compiler/src/driver/{check,build,project}.rs` | Text/byte IR emitter plus concrete driver and project records | Native build needs filesystem traversal, arguments, process execution, temporary files, and compiler/runtime linking |

Recursive owned trees are possible for finite syntax, but they are not a
prerequisite. Indexed arenas represent recursive grammar and type graphs with
integer links and `Array<Node>` storage, avoid self-referential borrows, and
provide stable IDs. Compare both approaches on parser allocation, traversal,
and diagnostics before choosing a permanent representation. Existing Rust
modules show the conceptual boundaries; their exact Rust types and APIs are
not a porting contract (`docs/architecture.md`).

## Library and toolchain inventory

| Operation required by a stage | Available now | Gap and first need |
| --- | --- | --- |
| Preserve arbitrary source bytes, index them, validate or decode UTF-8 | `os.ReadFile`; `strings.Bytes` and `FromBytes`; `zore/unicode/utf8`; `Array<byte>` and slices | Enough for an in-memory lexer. Keep spans in bytes and compare invalid UTF-8 behavior with `compiler/src/source/` |
| Build text, parse and format basic values | `zore/strings` (search, split, trim, case, `Builder`), `zore/bytes`, `zore/strconv` (integers in any base, booleans, `Quote`/`Unquote`), `zore/unicode` | Check allocation and large-input throughput of `strings.Builder` |
| Read and write a named file | `os.ReadFile`, `WriteFile`, `os.File`, `bufio.Reader`/`Writer`; explicit `error` results | Enough for a one-file driver; file errors and invalid UTF-8 must preserve useful path/context |
| Discover `.ore` files, manifests and imports | `compiler/src/driver/project.rs` uses sorted `read_dir`, `is_file`, ancestor search and canonical paths | `os.ReadDir` (sorted), `os.Stat`, `path`/`filepath` (`Join`, `Dir`, `Base`, `Ext`, `Clean`, `Abs`). Missing: resolving symbolic links for canonical paths |
| Read CLI arguments and selected environment | `compiler/src/driver/{command,session,build}.rs` uses `OsString` args, current directory, temp directory and `ZORE_CC`/cache settings | `os.Args`, `os.Getenv`, `os.Getwd`, `os.Exit`. Arguments and values that are not UTF-8 are replaced lossily; a byte policy is still open, and there is no temporary-folder lookup |
| Invoke backend tools and run produced programs | `compiler/src/driver/build.rs` invokes clang and rustc with `std::process::Command` | `zore/os/exec`: argument vector without a shell, exit status in the error, captured output. Missing: separate stderr capture and inherited output |
| Manage temporary output and cleanup | Rust `TempDir` in `compiler/src/driver/build.rs` creates and removes a build directory | `os.MkdirAll`, `Remove`, `RemoveAll`. Missing: a unique temporary folder and failure-safe cleanup |

The standard package surface above is defined in `std/`; runtime backing lives
in `runtime/src/` (`os.rs`, `exec.rs`, `net.rs`, `strings.rs`, `strconv.rs`,
`unicode.rs`, `sys.rs`) and native bindings in `compiler/src/codegen/native.rs`.
The Rust driver has `fmt` and `test` command names, but
`compiler/src/driver/command.rs` reports them as unimplemented. A formatter and
Zore-native test runner are useful after the compiler works; neither blocks
the first lexer, frontend, or bootstrap comparison. Rust/Cargo tests remain
the test harness meanwhile.

Package initialization is not needed for the proposed stages: pass explicit
compiler contexts and construct tables in `main`. Borrowed map-entry access
is also not required initially: store integer IDs or Copy descriptors in
maps and retain owned records in arenas. Revisit either only after a concrete
workload demonstrates that the current API is insufficient. Do not silently
interpret Rust `HashMap` usage as a requirement for a new Zore map feature.

Each missing library or language operation gets its own specification §53
decision, ownership/error/async contract, and conformance tests before use.
Native host APIs must specify resource closure on normal return, `?`, and
panic, and distinguish recoverable I/O errors from process aborts. No FFI,
package globals, or new syntax is implied by this plan.

## Bounded stages and comparisons

The stage-0 Rust binary and its current test suite are retained at every
stage. Keep the Zore implementation in a separate `compiler-zore/` tree once
there is runnable code; no empty module scaffold is needed first.

| Stage | Deliverable and dependencies | Validation and rollback |
| --- | --- | --- |
| 1. Lexer oracle | A Zore function taking validated source bytes or a borrowed byte view and returning tokens, spans, and lexical errors. A minimal Rust test harness supplies bytes; no CLI or filesystem API in Zore. | Compare token kind, byte range, decoded literal and error location against `compiler/src/lexer/` on `tests/lexer/lexer.rs`, `tests/conformance/{comments,strings,integers,statement-boundaries}.md`, empty input, and seeded fuzz cases. Compare malformed UTF-8 rejection separately against `compiler/src/source/source_map.rs`, before lexing. Use Rust lexer if mismatched; no replacement yet. |
| 2. Parser and source diagnostics | Parse tokens to an indexed AST; add SourceManager and diagnostic records. Depends on stage 1 and representation checks. | Reuse `tests/parser/parser.rs` and `tests/diagnostics/source_diagnostics.rs`; compare tree shape through a stable test serialization, then normalized diagnostics with primary and related byte spans. Rust frontend remains authoritative. |
| 3. Name and type frontend | Resolve packages, type-check, build HIR; add directory/path APIs only when multi-file loading starts. | Reuse `tests/packages/packages.rs`, `tests/typecheck/check.rs`, and `tests/ownership/ownership.rs`. Compare accepted/rejected programs and structured diagnostics. A Rust-hosted harness can continue providing source files until Zore project loading exists. |
| 4. Semantic and executable core | MIR lowering, ownership, drop insertion, then async lowering; expose only tested subsets to the new backend. | Compare MIR invariants and diagnostics on bounded fixtures; execute differential tests from `tests/codegen/native.rs` for results, errors, panic reports, and drop order. Keep the Rust path for any unsupported program. |
| 5. Native codegen and driver | LLVM text emission, runtime ABI, project loading, CLI, clang/rustc invocation and temporary output cleanup. Requires host APIs listed above and a stable runtime link contract. | Reuse `tests/driver/cli.rs`, package tests, and native tests. Compare deterministic IR structure where practical, plus program exit, stdout/stderr, and diagnostics. Keep Rust `check` and `build` as fallback/reference. |
| 6. Self-compilation | Stage-0 Rust compiler builds the Zore compiler source into stage-1; stage-1 builds the same pinned source into stage-2; stage-2 repeats into stage-3. | Run the same frontend/native corpus through each stage, compare normalized diagnostics and program behavior, and repeat builds from clean inputs. Preserve stage-0 and the source commit/toolchain manifest for recovery. |

The initial lexer oracle should return a test-oriented record stream, such as
`kind, start, end, payload` plus error records; exact encoding is internal to
the harness. Feed identical byte buffers to both implementations. Fixtures
must include semicolon insertion around comments/newlines, Unicode,
raw/quoted strings, numeric errors, and token spans. Check bad UTF-8 at the
source-loading boundary. Require exact
ordered token and error records for the supported grammar and zero unexplained
differences on seeded randomized inputs. A passing lexer says nothing about
parser, type checker, or whole-compiler readiness.

## Bootstrap trust and completion

Pin the compiler source revision, standard package sources, runtime revision,
host target, clang/rustc versions, command arguments, environment inputs, and
test corpus for each bootstrap run. Stage-0 builds stage-1; stage-1 builds
stage-2; stage-2 builds stage-3 from the same source. Verify each executable
can compile the full pinned corpus with matching acceptance decisions,
normalized diagnostic records, exit status, stdout/stderr, and relevant
resource cleanup. Record hashes of generated binaries and LLVM output, but do
not require byte equality until nondeterministic paths, temporary names,
symbol ordering, toolchain metadata, and timestamps are measured and removed.
Where IR is stable, compare normalized IR as a stronger additional check.

Call Zore self-hosting complete only when stage-1 and stage-2 are built by the
specified preceding stage on a clean machine, stage-2 and stage-3 show stable
compiler behavior over the complete pinned compiler/test corpus, the Zore
compiler can build its own source without Rust compiler logic, and the Rust
bootstrap can still reproduce stage-1 from the pinned source. This is a
measurable bootstrap milestone, not a claim of production readiness, full MVP
conformance, cross-platform support, or independence from the Rust runtime.
The Rust compiler remains available for regressions and recovery after that
milestone.
