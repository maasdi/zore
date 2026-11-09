# HTML escaping conformance

Contract: spec §37.31, issue #226.

| Contract | Executable evidence |
| --- | --- |
| Exact five replacements, empty/existing entities, repeated escaping | `tests/codegen/html.rs` and the Python differential oracle |
| UTF-8 and control-byte preservation, multibyte boundaries, dense expansion | Native cases and seeded/all-Latin-1 Python inputs |
| Plain string signature, caller reuse, function values, frontend-only use | `tests/packages/html.rs` |
| Bytes rejected and private helpers hidden | Frontend rejection cases |
| Results outlive source builders and survive async suspension | Leak-checked native test on a controlled single worker |
| Supported text and fixed single/double-quoted text attributes | Independent Python HTML parser checks |
| Executable user-facing guide | Compiled native guide test |

Run `cargo test --locked --test packages html`,
`cargo test --locked --test native html`, and, after building the compiler,
`python tests/hardening/html_oracle.py`. CI runs all three hosts.

No context detection, sanitization, URL validation, script/style escaping,
unquoted-attribute support, entity decoding or template renderer is implemented.
The separate [template decision](../../docs/design/templates.md) and Q16 keep
that deferred work visible; they are not passing implementation claims.
