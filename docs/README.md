# Zore documentation

| Document | Purpose |
| --- | --- |
| [Language specification](../spec/language-spec.md) | The authoritative definition of the Zore MVP. Normative rules override examples; changes follow §53. |
| [Architecture](architecture.md) | How the bootstrap compiler is structured: stages, intermediate representations, constant evaluation, and code generation. |
| [Roadmap](roadmap.md) | Canonical milestones, current status, detailed phases, and active validation work package. |
| [Compiler structure](../compiler-structure.md) | Target compiler and runtime organization; add only implemented modules. |
| [Specification questions](spec-questions.md) | Open language questions. Decided rules are in the specification. |
| [Decision records](decisions/) | Substantial implementation choices with context, alternatives, and consequences. |
| [Testing guide](../tests/README.md) | Test suites, conformance documents, and conventions. |

## Decision records

- [0001 — Native backend: LLVM objects linked with a Rust runtime](decisions/0001-native-backend.md)

## Withdrawn proposals

These proposals were not accepted and changed nothing. They are kept for their reasoning.

- [`go` on a callee stored in a struct field](proposals/spawn-field-callee.md)

## Open proposals

These proposals await a decision and change nothing yet.

- [Opt-in scoped tasks](proposals/scoped-tasks.md) (Q10)
- [Staged self-hosting plan](proposals/self-hosting-plan.md) (issue #56; stages 1 and 2 done)

Proposals that were accepted and implemented are removed from this folder. Their
decisions are locked in the [language specification](../spec/language-spec.md),
which is authoritative.
