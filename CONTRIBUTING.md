# Contributing

Read the language specification and `AGENTS.md` first. The repository is at the
M3–M4 parser stage, with resolution and type checking next; choose work from
`docs/roadmap.md`.

For each change:

1. Identify the relevant spec sections and milestone acceptance criteria.
2. Check `docs/spec-questions.md` for unresolved syntax or semantics. Record new
   questions with their affected stage; do not resolve them accidentally in code.
3. Implement the smallest coherent step, retaining source spans and compiler
   stage boundaries. Add tests for new behavior and regressions.
4. Run the four validation commands in the README. State any checks not run.
5. Update milestone status, examples, and developer documentation as needed.

Describe the concrete behavior changed and validation results in reviews. Include
spec references for semantic work and explain conservative restrictions. Language
changes follow §53; architecture choices may evolve without changing semantics.

Use the repository Rust toolchain. Update its pin and `Cargo.lock` deliberately,
validating Linux and macOS in CI. Keep generated artifacts under `target/`.
No release, distribution, or licensing policy has been selected yet.
