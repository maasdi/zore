# Streaming gzip conformance

Locked contract: spec §37.26. ZIP/TAR I/O and confined Linux/macOS/Windows extraction are mapped in
`archives.md`; remaining format work is in `docs/design/archives.md`.

| Contract | Evidence |
| --- | --- |
| Owned endpoints, Move wrappers, private state and mutable operations | `tests/packages/gzip.rs` acceptance/rejection pairs |
| Explicit finish/close, successful repeat finish, no drop finalization | `tests/codegen/gzip.rs` |
| Partial positive sink writes, invalid counts, zero progress, sticky errors | `tests/codegen/gzip.rs` |
| Inclusive byte limits, no output on invalid construction, input preflight, partial sink prefixes | native tests plus `tests/hardening/gzip_oracle.py` |
| Final source bytes plus EOF/other error, no-progress source, invalid counts | native tests with one-byte custom sources |
| Reading to EOF validates trailers; close does not drain | native tests and truncated/corrupt oracle fixtures |
| Stored, fixed and dynamic DEFLATE; CRC32/ISIZE; empty and concatenated members | Python gzip/zlib interoperability oracle |
| Optional extra/name/comment/header CRC; corrupt checksums and every truncated prefix | Python oracle |
| Member count, output expansion limits and trailing garbage | Python oracle |
| Normal/error/panic cleanup, one-worker task execution with blocked sink | leak-checked native tests |
| Documentation example compiles and runs | native guide test |

Run `cargo test --locked --test packages gzip`,
`cargo test --locked --test native gzip`, and
`python tests/hardening/gzip_oracle.py` after building the compiler. The oracle
builds one native tool and checks deterministic external fixtures in both
directions. CI runs it on Linux, macOS and Windows. Local Linux results do not
establish results on the other hosts.
