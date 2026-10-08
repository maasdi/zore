---
name: code-style
description: Apply the project's code style when writing, editing, or reviewing code. Use before committing code changes to strip unnecessary comments and document references.
---

# Code style

When writing or editing code:

1. Name things so the code explains itself. Prefer a descriptive variable, function, method, or type name over a comment.
2. Do not add comments by default. Add one only when it is truly necessary, such as a non-obvious reason, a safety constraint, or a workaround. Keep it to one short line.
3. Never reference other documents or Markdown files in code or comments. That means no spec section numbers like `(§16.4)`, no doc file paths, and no "see X.md".

Before committing:

1. Review the diff for comments you added.
2. Delete comments that restate what the code does. Rename instead when the code is unclear.
3. Shorten each remaining comment to one line, and remove any document references.
