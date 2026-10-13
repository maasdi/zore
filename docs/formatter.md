# Formatting Zore source

```console
zore fmt main.ore
zore fmt .
zore fmt --check .
```

`fmt` writes a `.ore` file in place, or recursively formats `.ore` files below
the named directory. It formats each file independently: imports need not
resolve, and neither clang nor runtime linking is required. Use `--` before
a target whose name starts with `-`.

`--check` writes nothing. It lists paths that need formatting in deterministic
order and exits 1 if any do; formatted input exits 0 with no output. Invalid
syntax or an I/O error also exits 1 with a diagnostic on standard error.

The style uses four spaces per block/list indentation, spaces around binary
and assignment operators, compact calls/indexes/types, one blank line between
top-level functions/types, and one final newline. It keeps the existing
line breaks in expression/list layouts and does not impose a maximum width.
Explicit semicolons are retained; counting-loop and fixed-array type separators
stay inline. Mixed binding targets retain their explicit `let`/`var` markers.
Comments remain in source order and declaration comments stay with their
declarations. String/rune spellings, raw-string contents, numeric spellings,
parentheses and trailing commas are preserved.

All selected files are parsed and their output is reparsed before any write.
The formatter checks that spelling and statement-boundary tokens are preserved,
allowing only implicit separators before a closing brace or EOF. If a file is
invalid or this check fails, no selected file is modified. Semantic validity is
the job of `zore check`; formatting also accepts bundled native declarations
without bodies.

Directory traversal skips symlinks, hidden directories, `target`, and
`node_modules`; it does not follow imports outside the selected tree. A symlink
named directly as the target is rejected. No `.ore` files is an error. Successful
writes preserve file permissions and replace each changed file through a
temporary file in its own directory. Writes are atomic per file; an I/O failure
during a multi-file write can leave earlier files formatted. Source changes
observed during preflight abort the operation before writes.

This is the first formatter implementation. It retains user-chosen expression
wrapping and explicit semicolons rather than rewriting those source choices;
future style refinements need corresponding idempotence and preservation tests.
