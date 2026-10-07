# Zore documentation

| Document | Purpose |
| --- | --- |
| [Language specification](../spec/language-spec.md) | The authoritative definition of the Zore MVP. Normative rules override examples; changes follow §53. |
| [Architecture](architecture.md) | How the bootstrap compiler is structured: stages, intermediate representations, constant evaluation, and code generation. |
| [Roadmap](roadmap.md) | Canonical milestones, current status, detailed phases, and active validation work package. |
| [Compiler structure](../compiler-structure.md) | Target compiler and runtime organization; add only implemented modules. |
| [Specification questions](spec-questions.md) | Open and resolved language questions, and the conservative choices made while they were open. |
| [Decision records](decisions/) | Substantial implementation choices with context, alternatives, and consequences. |
| [Testing guide](../tests/README.md) | Test suites, conformance documents, and conventions. |

## Decision records

- [0001 — Native backend: LLVM objects linked with a Rust runtime](decisions/0001-native-backend.md)

## Accepted proposals

These proposals were accepted and folded into the specification. They are kept
for their rationale; the specification is authoritative.

- [Arrays, indexing, and slicing](proposals/arrays-slices.md) (spec §12.6)
- [Maps](proposals/maps.md) (spec §13.3)

## Proposals under review

These are not part of the specification. Nothing in them is implemented until the
maintainer accepts them.

- [Async functions as state machines, without fibers](proposals/async-state-machines.md) (Q32)
