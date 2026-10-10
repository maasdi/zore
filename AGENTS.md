# Working on Zore

## Authority and scope

- Read `spec/language-spec.md` before implementation. It is authoritative;
  normative rules override examples. Read the sections relevant to each change.
- Follow locked decisions. TBD means unresolved, and OUT OF MVP means excluded.
  Do not infer unspecified syntax from Go, Rust, or illustrative examples.
- Follow spec §53 for language changes: update the specification and explicitly
  lock the decision with examples, ownership/error/async implications, compiler
  impact, and conformance tests before treating it as implementation-ready.
- Record unresolved dependencies in `docs/spec-questions.md`. Isolate blocked
  functionality and use conservative internal behavior where necessary. A
  temporary implementation choice must not become a source-language contract.
- Consult `docs/roadmap.md` and keep status honest. A scaffold or placeholder is
  not an implemented compiler stage. Do not report successful checking/building
  for unsupported programs.

## Architecture

- Start with one Rust crate (§44). Add modules when they have real work to do;
  avoid empty modules, speculative frameworks, and premature crate splitting.
- Keep parsing, resolution, typing, ownership, lowering, and code generation
  separate. AST, HIR, and MIR have distinct purposes (§25).
- Keep semantic checking independent of LLVM. Introduce backend dependencies
  only when needed and document the supported version and host requirements.
- Preserve file identity and byte spans through lowering. Diagnostics should
  explain the source locations and ownership relationships involved (§24–26).
- Use stable typed semantic IDs after resolution (§27–28). Model storage as
  places with projections; distinguish type classification from value state.
- Separate ownership validation from drop placement. Plan for partial moves,
  control flow, suspension, and persistent async storage (§30–35).
- Favor language-neutral structures that can eventually be ported to Zore.
  Rust implementation behavior does not define Zore semantics.

## Semantic guardrails

- Parameters borrow by default; `mut` grants mutable borrowing and `own`
  specifies ownership transfer. Call sites have no move markers.
- Copy/Move is type-driven. Strings and channel handles are Copy; owned dynamic
  arrays and maps are Move. Derive struct classification from fields.
- Reject ownership/lifetime violations when safety cannot be proven. Preserve
  deterministic cleanup on normal and error exits and across async execution.
- Tasks and async use ordinary ownership rules. Dropping a Task handle detaches
  it; it does not cancel work. Channels transfer Move messages and copy Copy ones.
- Do not introduce `:=`, source lifetimes, raw pointers, `unsafe`, generic
  types or interface and generic features beyond spec §22, hidden ordinary
  exceptions, or tracing GC into the MVP.
- Do not treat conceptual mutex dereference syntax, diagnostic examples, or
  incomplete sample declarations as additional normative language rules.

## Workflow and checks

- Keep changes focused on the active milestone. Update supporting docs when
  architecture, commands, prerequisites, or implementation status change.
- Add focused regression/conformance tests for compiler behavior. Pair supported
  semantic behavior with rejection cases where relevant. See `tests/README.md`.
- Run `cargo fmt --all -- --check`,
  `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo build --locked`, and `cargo test --locked --all-targets`.
- Track `Cargo.lock`. Explain new dependencies and keep the frontend usable
  without LLVM. Do not introduce a runtime/backend dependency for convenience.
- Report what changed, what was verified, and any unresolved blockers. Never
  describe unrun tests or pending conformance examples as passing.

## Code style

- Apply `.agents/skills/code-style/SKILL.md` when writing or reviewing code.
- Prefer descriptive names over comments; keep necessary comments to one short line.
- Do not reference specification sections or Markdown documents in code comments.
