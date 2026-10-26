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
| [Platform support](platform-support.md) | Native host/toolchain requirements, platform APIs, and Windows validation. |
| [MVP evidence inventory](conformance/mvp.md) | Current specification coverage, direct test gaps, intentional restrictions, and tracked follow-ups. |
| [Tagged dependencies](versioned-dependencies.md) | Compatible minimum versions, tagged source verification, lock provenance and offline consumption. |
| [Release metadata](release-metadata.md) | Optional local release preflight, canonical version/tag grammar, licensing and compatibility metadata. |
| [Release workflow](release-workflow.md) | Ordinary Git author/tag/push publication, read-only update reports and explicit application upgrades. |
| [Formatter](formatter.md) | `zore fmt`, check mode, style, traversal, and write behavior. |
| [Lexical task scopes](scoped-tasks.md) | Plain-function borrowing tasks, mandatory join/cleanup, failure rules and staged async support. |
| [Performance baseline](performance.md) | Reproducible compiler, REST/JSON and frame measurements, raw data and measured follow-ups. |
| [Performance follow-ups](performance-followups.md) | Internal LLVM linkage and cancellable request deadlines, paired compiler measurements and long REST retention runs. |
| [Runtime population recapture](runtime-scaling-refresh.md) | Current Linux/macOS/Windows task, socket and timer populations, source identities and cleanup. |
| [Historical runtime populations](runtime-scaling.md) | Original three-host capture, pinned storage and Windows follow-ups. |
| [Native build scheduling](performance-build-scheduling.md) | Current-main stage profiles and alternating Linux warm/cold measurements for overlapping runtime preparation and object compilation. |

## Decision records

- [0001 — Native backend: LLVM objects linked with a Rust runtime](decisions/0001-native-backend.md)
- [0002 — Generated JSON codecs](decisions/0002-json-codecs.md)
- [0003 — HTTP function handlers and automatic signals](decisions/0003-http-functions-and-signals.md)

## Withdrawn proposals

These proposals were not accepted and changed nothing. They are kept for their reasoning.

- [`go` on a callee stored in a struct field](proposals/spawn-field-callee.md)

## Open proposals

These documents record design directions; the specification defines the implemented contracts.

- [HTTP server and URL design](proposals/http-server.md) (issue #105; implemented in §37.6)
- [Opt-in scoped tasks](proposals/scoped-tasks.md) (Q10)
- [Staged self-hosting plan](proposals/self-hosting-plan.md) (issue #56; stages 1 and 2 done, later stages deferred)

Retained proposals preserve their reasoning. Accepted decisions are locked in
the [language specification](../spec/language-spec.md), which is authoritative.
