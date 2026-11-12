# Archive conformance

Contracts: spec §§37.27–37.30, plus `io.ReadSeeker` in §37.3.

| Contract | Executable evidence |
| --- | --- |
| Owned endpoints, Move wrappers, private state, mutability and frontend-only checking | `tests/packages/archive.rs` |
| IEEE CRC32 check vector, empty and incremental checksums | `tests/codegen/archive.rs` |
| USTAR/stored and DEFLATE ZIP reads and writes through short I/O | Native roundtrip tests and independent Python oracle |
| Entry presence/EOF, explicit skip, unread entry rejection | Native lifecycle tests |
| Concrete sink recovery after successful/failed finalization | Native sink recovery and `.tar.gz` oracle/guide tests |
| Source/sink stalls, invalid counts, broken seeks, partial bytes with errors | Native error/cleanup tests |
| Constructor failures, early close, normal/error/panic drop | Leak-checked native tests |
| One-worker progress through a blocked sink | Controlled native task test |
| Header checksums, directory bounds/metadata, CRC32, descriptors and UTF-8 names | `tests/hardening/archive_oracle.py` with `tarfile`/`zipfile` fixtures |
| Exact byte/data/count limits, truncated structures, unsupported flags/types | Python oracle plus native tests |
| Relative names, traversal, Unicode controls, Windows characters/device components | Python oracle |
| Unsigned descriptors with CRC equal to descriptor signature | Algebraically constructed independent CRC payload in Python oracle |
| `.tar.gz` composition and full gzip trailer validation | Python oracle and compiled guide example |
| Distinct generic receiver methods in one package | `tests/codegen/generic_acceptance.rs` regression with interface dispatch and drop |

Run `cargo test --locked --test packages archive`,
`cargo test --locked --test native archive`, and, after building the compiler,
`python tests/hardening/archive_oracle.py`. CI runs the oracle on Linux, macOS
and Windows. `python tests/hardening/extraction_oracle.py` validates extraction
and rollback on Linux/macOS/Windows.
Extended TAR/ZIP forms remain unsupported.

Raw DEFLATE boundary/counter/limit/finalization tests and staged-tree one-worker
and panic cleanup are in `tests/codegen/archive_followup.rs`. Frontend-only
checks cover moved sources, Move codec/tree state, visibility and async calls.
`runtime/src/staged_tree_unix.rs` exercises no-follow traversal, publication
races, renamed parents, exclusive leaves, modes, node/depth limits and cleanup.
The extraction oracle checks independent TAR/ZIP/gzip fixtures, DEFLATE bombs,
corruption after staged output, duplicate/case/path/type conflicts, existing and
dangling-link destinations, implicit nodes and 64-component boundaries. The
macOS run additionally rejects normalization aliases on its filesystem; a native
regression rejects inheritable ACL grants before creating staging.
Nested generic interface specialization has its own native regression.

Windows provider regressions in `runtime/src/staged_tree_windows.rs` check
protected owner/LocalSystem DACLs despite permissive inherited ACLs, junctions
and file/directory symlinks, case aliases and available 8.3 aliases, replacement
identity checks, node/depth limits, destination races, renamed parents and
cleanup. The native one-worker and panic cases run on Windows too. The complete
Python extraction oracle runs on every host.

## PAX and GNU TAR names (§37.33)

| Case | Required result | Evidence |
| --- | --- | --- |
| Python `tarfile` PAX and GNU archives with names over 255 bytes, non-ASCII names, long directories and ignored PAX records | Full names, sizes and data | `archive_oracle.py` |
| PAX `path` with a GNU long name and a USTAR name | The PAX name | `archive_oracle.py` |
| PAX `size` over a zero header size; an empty `path` value | The PAX size; the USTAR name | `archive_oracle.py` |
| Global header with ignored records; with `path` or `size` | Ignored; failure | `archive_oracle.py` |
| `./` root and `./a.txt` | Root skipped; `a.txt` | `archive_oracle.py` |
| Bad record length, leading zero, missing `=`, empty key, missing newline, invalid UTF-8, bad or oversized `size`, `linkpath`, `GNU.sparse.*`, non-UTF-8 `hdrcharset`, `../` path | Failure | `archive_oracle.py` |
| Repeated `x` or `L`, extended header at the end, oversized `x` or `L`, `K`, `S` | Failure | `archive_oracle.py` |
| Extended header bytes and the archive limit | Counted | `archive_oracle.py` |
| `TARWriter` names of 101, 300 and 1,024 bytes and a 300-byte directory | PAX `path`; Python and Zore read them back | `archive_oracle.py`; native `long_tar_names_use_pax_and_dot_prefixes_are_removed` |
| Writers and ZIP reading with `./a` | Rejected | `archive_oracle.py` |
| GNU tar 1.35 `gnu`, `posix`, `oldgnu` and `ustar` archives with long paths and `./` names | Read in full | Checked locally only; CI has the Python cases |

## Entry kinds and executable files (§37.34)

| Case | Required result | Evidence |
| --- | --- | --- |
| TAR symlink, hard link, PAX `linkpath` and GNU `K` long targets in PAX, GNU and USTAR formats | `Next` kinds and targets | `archive_oracle.py` |
| ZIP Unix symlinks, stored and deflated | `SymlinkEntry` with the data as target | `archive_oracle.py` |
| TAR modes 0755 and 0610; ZIP Unix 0755 and 0644; non-Unix ZIP with 0755 bits | Executable only for set bits on TAR or Unix ZIP | `archive_oracle.py` |
| Link header with data, empty target, `linkpath` or `K` on a file, global `linkpath`, dangling `K`, ZIP target empty, over 1,024 bytes, invalid UTF-8 or with NUL | Failure | `archive_oracle.py` |
| Writers given an executable file | TAR mode 0755, ZIP Unix 0o100755 read by Python | `archive_oracle.py`; native `entries_report_kinds_and_writers_reject_links_and_executable_directories` |
| Writers given links, a kind that does not match the header, or an executable directory | Sticky failure | native, same test |
| Extracting TAR and ZIP executables | Mode 0700; other files 0600; non-Unix ZIP not executable | `extraction_oracle.py`; runtime `publication_and_private_permissions` |
| Extracting a symlink or hard link without `Links` | `archive: links are not extracted`; nothing published | `extraction_oracle.py` |
| With `Links`: relative symlinks inside the tree, `..` that stays inside, a symlink to a folder, ZIP symlinks, hard links with and without `./` | Created exactly; hard links share the file; Windows fails symlinks | `extraction_oracle.py` |
| With `Links`: `..` above the root, absolute, `\` or `C:` targets, a target through an earlier or later symlink, hard links to missing, folder or later entries, `../` hard links, files written below a symlink | Failure; nothing published or left outside | `extraction_oracle.py` |
| `ExtractOptions` defaults | `Links` false | package `archive_entries_are_copy_values_and_kinds_are_typed` |
| `Entry` literals need every field; `Executable` is a `bool` | Rejected otherwise | package `archive_entries_are_copy_values_and_kinds_are_typed` |

## ZIP64 and size limits (§37.35)

| Case | Required result | Evidence |
| --- | --- | --- |
| Python ZIP64 extra fields on stored and deflated entries, seekable and streamed with 24-byte descriptors | Read; data and sizes match | `archive_oracle.py` |
| 70,000-entry Python archive | Read with an entry limit of 70,000; fails with 69,999 | `archive_oracle.py` |
| 70,000 entries written by `ZIPWriter` | ZIP64 end record and locator; Python lists every name; Zore reads it back | `archive_oracle.py` |
| Three entries written by `ZIPWriter` | No ZIP64 records | `archive_oracle.py` |
| ZIP64 end record with a bad signature, size, counts, entries, central size; locator with a bad disk, offset or disk count; no locator; a marker without a locator | Failure | `archive_oracle.py` |
| ZIP64 extra field that is short, repeated, or holds a value over `MaxBytes` | Failure; an empty field is accepted | `archive_oracle.py` |
| ZIP64 central size, compressed size and offset fields, individually and together | Read; Python and Zore recover the same payload | `archive_oracle.py` |
| Streamed ZIP64 stored/deflated entries with unsigned 20-byte descriptors | Read; payload matches the signed descriptor case | `archive_oracle.py` |
| Empty ZIP entries with 1024-byte names up to the central-directory metadata limit | Finalize successfully; one more entry fails without reserving unused ZIP64 extras | `archive_oracle.py` counting sink |
| 4.5 GB stored entry written by Zore and a 4.6 GB entry written by Python; 8.6 GB TAR entry | Read by the other side in full | Checked by hand only (too large for CI) |

## Extracting into an existing folder (§37.36)

| Case | Required result | Evidence |
| --- | --- | --- |
| TAR and ZIP merged into a folder with other files and an existing subfolder | New files added; existing files unchanged | `extraction_oracle.py`; runtime `merge_adds_missing_nodes_and_never_replaces` |
| The same archive merged twice | Second merge fails; nothing changes | `extraction_oracle.py` |
| A staged file over an existing file; a folder over a file; a file over a folder | Failure naming the path; nothing added | `extraction_oracle.py`; runtime |
| A symlinked subfolder in the destination | Failure; nothing written outside | `extraction_oracle.py`; runtime |
| The destination is a file or a symlink | Failure before staging | `extraction_oracle.py`; runtime |
| The destination is missing | Published as a new folder | `extraction_oracle.py`; runtime |
