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
| Every level `0`–`9` round-trips empty, one-byte, text, noise and long-run input | native `every_level_round_trips_and_output_does_not_depend_on_write_sizes` |
| Output is the same whether input arrives in one `Write` or 7-byte pieces | native, same test |
| Level 9 is no larger than level 1; level 0 stores; XFL is 2/4/0 | native, same test; oracle |
| Invalid levels fail before output; `Flush` makes the prefix decodable, keeps history, and is sticky on failure; `Flush` after `Finish` fails | native `invalid_levels_fail_before_output_and_flush_makes_data_readable` |
| Python zlib reads every level and streams flushed every 1,000 bytes; level 6 stays within 5% of zlib | `gzip_oracle.py` |
| Levels are typed `flate.Level`; `Flush` needs a mutable writer | package `compression_levels_are_typed_and_flush_needs_a_mutable_writer` |

Run `cargo test --locked --test packages gzip`,
`cargo test --locked --test native gzip`, and
`python tests/hardening/gzip_oracle.py` after building the compiler. The oracle
builds one native tool and checks deterministic external fixtures in both
directions. CI runs it on Linux, macOS and Windows. Local Linux results do not
establish results on the other hosts.
