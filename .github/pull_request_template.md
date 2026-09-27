## Summary

<!-- What changes and why. Link the issue, milestone, or spec question. -->

## Spec sections

<!-- Which parts of spec/language-spec.md this implements or affects. -->

## Checklist

- [ ] Follows the specification; language changes update the spec first (§53).
- [ ] New unresolved questions are recorded in `docs/spec-questions.md`.
- [ ] Accepted behavior is paired with rejection tests where relevant.
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --all-targets -- -D warnings`
- [ ] `cargo build --locked`
- [ ] `cargo test --locked --all-targets`
- [ ] Docs (README, architecture, roadmap, tests/README) updated if status or
      behavior changed.
- [ ] No unrun tests or pending conformance cases are described as passing.
