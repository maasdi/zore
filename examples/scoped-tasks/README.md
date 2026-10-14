# Lexical task scopes

Run `zore run examples/scoped-tasks/main.ore`.

Two children share a parent-owned array without cloning or moving it. The second
block ignores its mutable child's handle and still joins before reading count.
The final ordinary spawn transfers the array into an async function whose own
scope lends it to two children and retrieves their results with `await`.
See [the contract and limits](../../docs/scoped-tasks.md).
