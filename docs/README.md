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
- [Async functions as state machines, without fibers](proposals/async-state-machines.md) (spec §17.3, §17.7, §18.3, §20.2, §35.1, §37.3; all six slices implemented)
- [Declared function values and `go` on owning callables](proposals/function-values-and-spawned-closures.md) (spec §16.2, §16.4, §16.6, §18.3, §18.4; issue #52)

## Proposals under review

These proposals are not accepted. They lock nothing, and the specification does not change until the maintainer accepts them.

- [Method values](proposals/method-values.md) (Q34; follow-up to issue #52)
