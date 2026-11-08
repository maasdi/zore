# Archive conformance

Contracts: spec §§37.25–37.26, plus `io.ReadSeeker` in §37.3.

| Contract | Executable evidence |
| --- | --- |
| Owned endpoints, Move wrappers, private state, mutability and frontend-only checking | `tests/packages/archive.rs` |
| IEEE CRC32 check vector, empty and incremental checksums | `tests/codegen/archive.rs` |
| USTAR/stored ZIP reads and writes through short I/O | Native roundtrip tests and independent Python oracle |
| Entry presence/EOF, explicit skip, unread entry rejection | Native lifecycle tests |
| Concrete sink recovery after successful/failed finalization | Native sink recovery and `.tar.gz` oracle/guide tests |
| Source/sink stalls, invalid counts, broken seeks, partial bytes with errors | Native error/cleanup tests |
| Constructor failures, early close, normal/error/panic drop | Leak-checked native tests |
| One-worker progress through a blocked sink | Controlled native task test |
| Header checksums, directory bounds/metadata, CRC32, descriptors and UTF-8 names | `tests/hardening/archive_oracle.py` with `tarfile`/`zipfile` fixtures |
| Exact byte/data/count limits, truncated structures, unsupported flags/types/ZIP64 | Python oracle plus native tests |
| Relative names, traversal, Unicode controls, Windows characters/device components | Python oracle |
| Unsigned descriptors with CRC equal to descriptor signature | Algebraically constructed independent CRC payload in Python oracle |
| `.tar.gz` composition and full gzip trailer validation | Python oracle and compiled guide example |
| Distinct generic receiver methods in one package | `tests/codegen/generic_acceptance.rs` regression with interface dispatch and drop |

Run `cargo test --locked --test packages archive`,
`cargo test --locked --test native archive`, and, after building the compiler,
`python tests/hardening/archive_oracle.py`. CI runs the oracle on Linux, macOS
and Windows. ZIP DEFLATE, filesystem extraction and extended TAR/ZIP forms are
unsupported and tracked as follow-ups, not represented as passing coverage.
