# String methods

Common string operations read from left to right:

```ore
let title = "  Zore  ".TrimSpace().ToUpper()
let valid = title.HasPrefix("ZO")
let words = "one,two".Split(",")
```

These methods are available without an import. Existing package functions such
as `strings.TrimSpace(text)` remain supported. Import `std/strings` when using
its package functions or `Builder`; an import used only for method calls is
unused under the ordinary import rules.

Each method behaves like its identically named package function with the first
string argument supplied as the receiver. The fixed set is:

| Purpose | Methods |
| --- | --- |
| Search and comparison | `Contains`, `ContainsRune`, `HasPrefix`, `HasSuffix`, `Index`, `LastIndex`, `IndexByte`, `IndexRune`, `Count`, `EqualFold` |
| Case conversion | `ToUpper`, `ToLower` |
| Trimming | `TrimSpace`, `Trim`, `TrimLeft`, `TrimRight`, `TrimPrefix`, `TrimSuffix` |
| Transformation | `Repeat`, `Replace`, `ReplaceAll` |
| Splitting and conversion | `Cut`, `Split`, `SplitN`, `Fields`, `Bytes` |

Methods share-borrow the receiver and never change it. They evaluate the
receiver once, then arguments from left to right. String results preserve their
ordinary storage-sharing behavior. Chaining does not promise to combine
allocations; `Split`, `SplitN`, `Fields`, and `Bytes` return owned arrays whose
cleanup follows the usual ownership rules. Search indices remain byte offsets,
and Unicode behavior and panic diagnostics remain the same as package calls.

Strings cannot be `nil`. Empty strings are valid, and absence stays explicit:

```ore
let key, value, found = input.Cut("=")
if found {
    println(key.TrimSpace() + ": " + value.TrimSpace())
}
```

No separator gives `input`, `""`, and `false`. An empty right-hand value with a
separator gives `found == true`. Existing map lookups retain `(value, found)`,
and failures retain explicit errors and ordinary `?` propagation. JSON missing
and null handling is unchanged.

`strings.Join(parts, separator)`, `strings.FromBytes(data)` and
`strings.NewBuilder()` remain package functions because their first argument is
not a string. Byte-to-text conversion still validates UTF-8 and returns an error.

Method values use the same lifetime rules as struct methods. Bind the receiver
to a local, or use a field rooted in a local:

```ore
let text = "hello"
let upper = text.ToUpper
println(upper())
```

The receiver is borrowed under ordinary closure rules; owning task captures use
the existing Copy semantics for strings. Direct calls also accept literals,
computed strings, indexed values and temporary results. String methods can
satisfy compatible interfaces and can be called inside async functions or
spawned as ordinary synchronous tasks.

Named string types keep their own method sets. Convert explicitly with
`string(value)` to use the standard methods. User packages cannot declare
methods on primitive types or replace this fixed set, including by naming a
package `strings` or defining a function with the same name.

See spec §9.2 for the locked contract and §37.2 for each operation's semantics.
The standard-packages and REST API examples demonstrate the method syntax.
