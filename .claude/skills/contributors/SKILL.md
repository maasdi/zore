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

1. Follow "Commits and pull requests" in `CONTRIBUTING.md` for the message
   and the scope of the change.
2. Credit only the people who did the work. Do not add tool, agent or model
   names, co-author trailers for them, session links or "generated with" text
   to commit messages, pull requests, comments, release notes, code or docs.

## Before opening a pull request

1. Run every check under "Before you open a pull request" in
   `CONTRIBUTING.md`, and say in the pull request which ones you could not run.
2. Fill in the pull request template's checklist.
3. Check that the base branch is `main`. For a pull request built on another
   open one, follow "Stacked and parallel pull requests" in `CONTRIBUTING.md`.

This skill is the only place that defines branch names and the credit rule.
The checks, commit format, test and documentation duties live in
`CONTRIBUTING.md` and `AGENTS.md`; do not copy them here.
