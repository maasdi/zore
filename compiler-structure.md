# Zore Compiler Project Structure Guide

**Status:** Authoritative implementation guide for coding agents  
**Applies to:** Bootstrap Zore compiler  
**Primary implementation language:** Rust  
**Related specification:** `spec/language-spec.md`

---

# 1. Purpose

This document defines the expected project structure for the Zore bootstrap compiler.

Coding agents MUST use this structure as the architectural baseline unless a change is explicitly approved.

The goals are:

- keep compiler phases clearly separated
- avoid mixing parsing, typing, ownership, lowering, and code generation
- keep the first compiler easy to change while the language is still evolving
- support later extraction into multiple crates
- preserve a path toward eventually rewriting the compiler in Zore itself

The most important rule is:

> **Do not collapse compiler stages into one monolithic pass.**

---

# 2. Repository Structure

Use the following repository structure as the target architecture:

```text
zore/
├── Cargo.toml
├── README.md
├── spec/language-spec.md
├── compiler-structure.md
├── zore.toml
│
├── compiler/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       │
│       ├── driver/
│       │   ├── mod.rs
│       │   ├── command.rs
│       │   └── session.rs
│       │
│       ├── source/
│       │   ├── mod.rs
│       │   ├── source_file.rs
│       │   ├── source_map.rs
│       │   └── span.rs
│       │
│       ├── lexer/
│       │   ├── mod.rs
│       │   ├── lexer.rs
│       │   ├── token.rs
│       │   └── token_kind.rs
│       │
│       ├── parser/
│       │   ├── mod.rs
│       │   ├── parser.rs
│       │   ├── expression.rs
│       │   ├── statement.rs
│       │   ├── declaration.rs
│       │   └── type_syntax.rs
│       │
│       ├── ast/
│       │   ├── mod.rs
│       │   ├── node.rs
│       │   ├── expr.rs
│       │   ├── stmt.rs
│       │   ├── decl.rs
│       │   └── types.rs
│       │
│       ├── resolve/
│       │   ├── mod.rs
│       │   ├── resolver.rs
│       │   ├── symbol.rs
│       │   ├── scope.rs
│       │   └── ids.rs
│       │
│       ├── types/
│       │   ├── mod.rs
│       │   ├── ty.rs
│       │   ├── type_id.rs
│       │   ├── type_store.rs
│       │   ├── function_type.rs
│       │   └── classify.rs
│       │
│       ├── hir/
│       │   ├── mod.rs
│       │   ├── expr.rs
│       │   ├── stmt.rs
│       │   ├── function.rs
│       │   └── lower.rs
│       │
│       ├── ownership/
│       │   ├── mod.rs
│       │   ├── place.rs
│       │   ├── projection.rs
│       │   ├── borrow.rs
│       │   ├── region.rs
│       │   ├── move_state.rs
│       │   └── checker.rs
│       │
│       ├── mir/
│       │   ├── mod.rs
│       │   ├── body.rs
│       │   ├── block.rs
│       │   ├── statement.rs
│       │   ├── terminator.rs
│       │   ├── operand.rs
│       │   ├── rvalue.rs
│       │   └── lower.rs
│       │
│       ├── dropck/
│       │   ├── mod.rs
│       │   ├── analysis.rs
│       │   └── insertion.rs
│       │
│       ├── async_lowering/
│       │   ├── mod.rs
│       │   ├── state_machine.rs
│       │   ├── suspension.rs
│       │   └── lower.rs
│       │
│       ├── diagnostic/
│       │   ├── mod.rs
│       │   ├── diagnostic.rs
│       │   ├── label.rs
│       │   ├── code.rs
│       │   └── renderer.rs
│       │
│       ├── codegen/
│       │   ├── mod.rs
│       │   ├── llvm.rs
│       │   ├── layout.rs
│       │   └── abi.rs
│       │
│       └── context/
│           ├── mod.rs
│           └── compiler_context.rs
│
├── runtime/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── alloc.rs
│       ├── task.rs
│       ├── scheduler.rs
│       ├── channel.rs
│       ├── panic.rs
│       └── io.rs
│
├── std/
│   ├── core/
│   ├── io/
│   ├── collections/
│   └── sync/
│
├── tests/
│   ├── lexer/
│   ├── parser/
│   ├── resolve/
│   ├── typecheck/
│   ├── ownership/
│   ├── mir/
│   ├── async/
│   ├── channels/
│   ├── diagnostics/
│   └── codegen/
│
├── examples/
│   ├── hello/
│   ├── structs/
│   ├── ownership/
│   ├── errors/
│   ├── async/
│   └── channels/
│
└── bootstrap/
    └── README.md
```

---

# 3. Important Constraint: Do Not Split Into Many Crates Too Early

The compiler should begin as one primary Rust compiler crate:

```text
compiler/
```

The internal architecture should already be modular, but the project should NOT immediately create separate crates such as:

```text
zore-lexer
zore-parser
zore-ast
zore-resolve
zore-typeck
zore-hir
zore-ownership
zore-mir
zore-codegen
```

Those boundaries may be extracted later once APIs stabilize.

Reason:

- AST will change
- HIR will change
- MIR will change
- ownership analysis will change
- compiler passes will need rapid iteration

Premature crate boundaries make refactoring slower.

---

# 4. Compiler Pipeline

The compiler should follow this conceptual architecture:

```text
driver
  ↓
source
  ↓
lexer
  ↓
parser
  ↓
ast
  ↓
resolve
  ↓
types
  ↓
hir
  ↓
ownership
  ↓
mir
  ↓
dropck / async_lowering
  ↓
codegen
```

Cross-cutting modules:

```text
diagnostic
context
```

The exact order between ownership, MIR construction, async lowering, and drop insertion may evolve.

Do not use this freedom to mix all responsibilities together.

---

# 5. Module Responsibilities

## `driver/`
Responsible for CLI routing, sessions, and compiler pass orchestration. Must not contain parsing, type, ownership, or LLVM logic.

## `source/`
Responsible for source files, source maps, byte offsets, spans, and line/column lookup. Preserve source locations through all compiler stages.

## `lexer/`
Responsible only for `source text → tokens`. No name resolution, type checking, or ownership analysis.

## `parser/`
Responsible only for `tokens → AST`. No Copy/Move decisions, borrow checking, drop insertion, or LLVM work.

## `ast/`
Represents what the programmer wrote. Keep it close to source syntax. Do not encode full ownership state into AST nodes.

## `resolve/`
Responsible for scopes, symbols, name resolution, and strong semantic IDs. Later passes should use IDs instead of repeated string lookups.

Recommended IDs:

```rust
pub struct FileId(pub u32);
pub struct SymbolId(pub u32);
pub struct FunctionId(pub u32);
pub struct StructId(pub u32);
pub struct FieldId(pub u32);
pub struct LocalId(pub u32);
pub struct TypeId(pub u32);
pub struct BasicBlockId(pub u32);
pub struct BorrowId(pub u32);
pub struct RegionId(pub u32);
```

## `types/`
Responsible for type representation, type IDs, type storage, function types, validation, equality, and Copy/Move type classification.

Do not confuse type classification with value state.

## `hir/`
Represents what the program means after resolution and typing. HIR should contain resolved IDs and semantic structure.

## `ownership/`
Responsible for places, moves, borrows, mutable borrows, inferred regions/lifetimes, and ownership-state checking.

Important concepts:

```text
Place
Projection
Borrow
Region
MoveState
```

Do not scatter ownership flags across AST nodes.

## `mir/`
Represents executable control flow. It should be CFG/basic-block based and explicit about Copy vs Move.

Expected concepts:

```text
BasicBlock
Statement
Terminator
Operand
Rvalue
Place
```

MIR must eventually support:

```text
Copy(place)
Move(place)
Constant(...)
```

## `dropck/`
Responsible for determining where destruction is required and inserting explicit drop operations, including early returns, `?`, branches, and async states.

## `async_lowering/`
Responsible for suspension points, locals live across `await`, state-machine lowering, and ownership-safe async transformation.

Do not implement a separate async ownership model.

## `diagnostic/`
Responsible for structured compiler errors, warnings, codes, source labels, notes, and rendering.

Do not scatter manually formatted diagnostic strings throughout compiler passes.

## `codegen/`
Responsible for final MIR to LLVM/native lowering, layout, ABI, calling convention, and runtime calls.

Codegen must not parse source or perform front-end semantic analysis.

## `context/`
Contains compiler-wide shared context such as source manager, type store, symbol store, metadata, and diagnostics. Do not turn it into a dumping ground.

---

# 6. Runtime Structure

Use:

```text
runtime/
├── alloc.rs
├── task.rs
├── scheduler.rs
├── channel.rs
├── panic.rs
└── io.rs
```

Responsibilities:

- `alloc.rs` — owned runtime allocation primitives
- `task.rs` — task representation and lifecycle
- `scheduler.rs` — task scheduling
- `channel.rs` — channel buffering, synchronization, close semantics
- `panic.rs` — runtime panic support
- `io.rs` — runtime I/O and async I/O support

The runtime must not contain compiler parsing or semantic analysis.

---

# 7. Standard Library Structure

Expected high-level structure:

```text
std/
├── core/
├── io/
├── collections/
└── sync/
```

Do not build a large standard library before the compiler supports the required language features.

---

# 8. Test Structure

Use tests by compiler subsystem:

```text
tests/
├── lexer/
├── parser/
├── resolve/
├── typecheck/
├── ownership/
├── mir/
├── async/
├── channels/
├── diagnostics/
└── codegen/
```

Use both positive and negative tests.

Example:

```text
ownership/
├── copy_ok.ore
├── move_ok.ore
├── use_after_move_error.ore
├── mutable_borrow_conflict_error.ore
└── move_while_borrowed_error.ore
```

---

# 9. Implementation Order

## Phase 1 — Compiler Foundation

Start only with:

```text
compiler/src/
├── main.rs
├── lib.rs
├── driver/
├── source/
├── diagnostic/
├── lexer/
├── parser/
└── ast/
```

First target:

```bash
zore check hello.ore
```

## Phase 2 — Semantic Analysis

Add:

```text
resolve/
types/
hir/
```

Implement name resolution, symbols, types, and HIR lowering.

## Phase 3 — Ownership Foundation

Add:

```text
ownership/
mir/
dropck/
```

Implement Copy, Move, shared borrow, mutable borrow, use-after-move errors, and drop insertion.

## Phase 4 — Errors and Collections

Add language support for:

- `error`
- `?`
- arrays
- slices
- `Array<T>`
- maps

## Phase 5 — Concurrency

Only after synchronous ownership is reliable, add:

```text
async_lowering/
runtime/task.rs
runtime/scheduler.rs
runtime/channel.rs
runtime/io.rs
```

Then implement:

- `go`
- `Task`
- `async`
- `await`
- channels
- async I/O

## Phase 6 — Code Generation

LLVM work may begin early for simple programs, but backend complexity must not dictate front-end architecture.

`zore check` must work independently of LLVM.

---

# 10. Dependency Direction Rules

Good:

```text
parser → ast
resolve → ast
hir → types / resolved IDs
ownership → hir / mir concepts
codegen → mir
```

Bad:

```text
lexer → ownership
parser → LLVM
AST → scheduler
codegen → parser
ownership → raw parser tokens
```

Avoid circular semantic dependencies.

---

# 11. Mandatory Architecture Rules for Coding Agents

1. **Do not mix parser and semantic analysis.**
2. **Keep AST source-oriented.**
3. **Use semantic IDs after resolution.**
4. **Keep type classification separate from value state.**
5. **Use `Place` to model storage and projections.**
6. **MIR must explicitly represent Move and Copy.**
7. **Async must reuse the ordinary ownership model.**
8. **`zore check` must not depend on LLVM.**
9. **Diagnostics must be structured and span-aware.**
10. **Preserve source spans through lowering.**
11. **Do not create every future module as empty scaffolding.**
12. **Do not redesign locked language semantics from `spec/language-spec.md`.**

---

# 12. Self-Hosting Constraint

The bootstrap compiler is written in Rust, but the architecture must remain portable to Zore.

Prefer concepts that can later exist naturally in Zore:

```text
SourceMap
Token
AST
Symbol
Type
HIR
Place
Borrow
Region
MIR
BasicBlock
Diagnostic
```

Avoid making compiler semantics depend on Rust-only techniques such as:

- pervasive `Rc<RefCell<...>>`
- compiler semantics encoded only in Rust lifetime types
- heavy macro-generated compiler logic
- architecture that cannot reasonably be represented in Zore later

Rust implements the bootstrap compiler.

Rust does not define Zore semantics.

---

# 13. Future Crate Extraction

Later, stable modules may become workspace crates:

```text
zore-cli
zore-source
zore-lexer
zore-parser
zore-ast
zore-resolve
zore-typeck
zore-hir
zore-ownership
zore-mir
zore-codegen
zore-runtime
```

Only extract a crate when boundaries are stable and there is clear value.

Do not split crates for aesthetics.

---

# 14. Expected First Repository State

The initial repository should be intentionally smaller:

```text
zore/
├── Cargo.toml
├── README.md
├── spec/language-spec.md
├── compiler-structure.md
│
├── compiler/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── driver/
│       ├── source/
│       ├── diagnostic/
│       ├── lexer/
│       ├── parser/
│       └── ast/
│
├── tests/
│   ├── lexer/
│   └── parser/
│
└── examples/
    └── hello/
        ├── zore.toml
        └── main.ore
```

Do not generate empty implementations for all future subsystems.

---

# 15. First Milestone

Command:

```bash
zore check examples/hello/main.ore
```

Input:

```ore
package main

func main() {
    println("Hello, Zore!")
}
```

Expected:

1. source loads
2. lexer produces tokens
3. parser produces AST
4. diagnostics render correctly
5. valid syntax exits successfully

At this stage the compiler does **not** need:

- ownership checking
- LLVM
- async
- channels
- scheduler

---

# 16. Final Architectural Principle

Always preserve:

```text
AST
 ↓
what the programmer wrote

HIR
 ↓
what the program means

MIR
 ↓
how the program executes

LLVM
 ↓
how the machine executes it
```

Optimize first for:

- correctness
- clear compiler boundaries
- testability
- diagnostic quality
- ownership-model clarity
- future self-hosting

Optimize later for:

- compiler micro-optimizations
- crate count
- aggressive parallel compilation
- advanced backend optimization

---

# 17. Approved Deviations

The maintainer approved these differences between this guide and the code on
2026-10-07. Each one has a reason; `docs/architecture.md` describes the current
layout in detail.

1. **The type checker lives in `hir/lower.rs`, not `types/`.** It builds HIR as
   it checks. Putting it in `types/` made `types`, `hir`, and `resolve` depend on
   each other.
2. **Copy/Move classification is `Package::is_copy` in `hir/`, not
   `types/classify.rs`.** It needs struct fields and `drop` methods, which live
   in the HIR package; a `types/` home would recreate the same cycle.
3. **`Place` and `Projection` live in `mir/`, not `ownership/`.** The ownership
   checker reads them, and moving them would make `mir` depend on `ownership`.
4. **There is no `async_lowering/` and no `runtime/scheduler.rs`.** Tasks are
   stackful fibers (`runtime/fiber.rs`, `runtime/task.rs`), so an async function
   is an ordinary function that runs on a fiber's stack and needs no state-machine
   lowering. Async still reuses the ordinary ownership model (rule 7).
5. **There is no `context/` and no `diagnostic/code.rs`.** Nothing needs shared
   compiler context yet, and diagnostics have no codes yet (rule 11).
6. **The runtime has more modules than section 6 lists.** It also holds
   `fiber.rs`, `mutex.rs`, `deadlock.rs`, `reactor.rs`, `blocking.rs`, `sys.rs`,
   `net.rs`, `map.rs`, and the text modules, one per implemented feature.
7. **Integration tests are Rust files grouped by subsystem**, not `.ore` fixture
   files per folder. Async and channel behavior is tested end to end in
   `tests/codegen/native.rs`.

Any other difference still needs explicit approval (section 1).
