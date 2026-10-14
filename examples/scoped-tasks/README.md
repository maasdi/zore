# Lexical task scopes

Run `zore run examples/scoped-tasks/main.ore`.

Two children share a parent-owned array without cloning or moving it. The second
block ignores its mutable child's handle and still joins before reading count.
See [the contract and limits](../../docs/scoped-tasks.md).
