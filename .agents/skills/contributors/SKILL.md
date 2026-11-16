---
name: contributors
description: Apply the project's contributor rules when starting, committing, or submitting work. Use before creating a branch, writing a commit message, or opening or reviewing a pull request, including work delegated to other agents.
---

# Contributor rules

## Before creating a branch

1. Start from the latest `main` and keep one change per branch.
2. Name the branch `<type>/<description>`.
   - `feature/` for new behavior.
   - `bugfix/` for fixes.
   - `hotfix/` for urgent fixes to a release.
   - `release/` for release preparation, such as `release/v0.1.0-alpha.2`.
   - `chore/` for docs, CI, dependencies and other non-code work.
3. Write the description in short lowercase kebab-case that names the change:
   ASCII letters, digits and hyphens only. Dots are allowed only in `release/`
   versions. No leading, trailing or repeated hyphens or dots.
4. An issue number may lead the description, as in
   `feature/issue-218-package-distribution`, but never stands alone.
5. Never put a tool, agent, model or person name in a branch name.
6. Check the name before pushing. Outside `release/` it must match
   `^(feature|bugfix|hotfix|chore)/[a-z0-9]+(-[a-z0-9]+)*$`.

Good: `feature/http2-header-compression`, `bugfix/http-lingering-close`,
`chore/contributors-skill`.
Bad: `feat/hpack` (unknown type), `feature/HTTP2` (uppercase), `feature/290`
(number alone), `feature/claude-fixes` (tool name), `my-branch` (no type).

## Before committing

1. Keep the change focused on one milestone or topic.
2. Write a short imperative summary line, then a body that says what changed
   and why. Include specification references for semantic changes and explain
   any conservative restriction.
3. Credit only the people who did the work. Do not add tool, agent or model
   names, co-author trailers for them, session links or "generated with" text
   to commit messages, pull requests, comments, release notes, code or docs.
4. Change `Cargo.lock` only on purpose. A new dependency needs a
   justification, and the frontend must stay usable without LLVM.

## Before opening a pull request

1. Run the same checks as CI and fix what they report:

   ```sh
   cargo fmt --all -- --check
   cargo clippy --locked --all-targets -- -D warnings
   cargo build --locked
   target/debug/zore fmt --check .
   cargo test --locked --all-targets
   ```

   If Python files changed, also run `python .github/scripts/import_modules.py`
   and the unit tests of each benchmark harness you touched.
2. Language changes update the specification first and lock the decision
   before implementation. Record open questions in `docs/spec-questions.md`.
3. Add focused tests with every compiler or library change and pair accepted
   behavior with rejection cases.
4. Update the affected documentation when behavior, commands, prerequisites or
   status change: README, architecture, roadmap, `tests/README.md` and the
   changelog.
5. Fill in the pull request template's checklist. Say which checks you could
   not run. Never describe unrun tests or pending conformance cases as
   passing, and never report unsupported programs as working.
6. Check that the base branch is `main`. For a pull request stacked on another,
   use the parent's branch as base, name the parent in the description, and
   move the base to `main` once the parent merges.
7. Before merging work developed alongside other pull requests, update it with
   current `main` and wait for CI to pass.
