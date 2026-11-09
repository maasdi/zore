# Archive follow-ups to streaming gzip

Status: design proposal, not a public API or implemented archive support.
Tracked by [#224](https://github.com/maasdi/zore/issues/224), with library breadth
under [#210](https://github.com/maasdi/zore/issues/210). The implemented first slice
is [streaming gzip](../gzip.md), locked in spec §37.26.

## Package and API direction

Proposed `std/archive/tar` would own TAR headers, entry iteration and writing.
A reader would own an `io.Reader`, return explicit entry presence plus errors,
and expose each current entry through a bounded read operation. Advancing must
explicitly drain or reject unread entry content; a borrowed entry must not
outlive the reader. A writer would require an entry size, reject shorter/longer
content and explicitly finish padding and end records. Reject unsupported
extensions instead of silently interpreting them. GNU/PAX long names, sparse
files and special nodes need separate decisions.

Proposed `std/archive/zip` would own ZIP records, central-directory validation,
entry metadata, CRC32 and stored/DEFLATE payloads. The first reader should use a
bounded seekable source, since a central directory can contradict local headers.
The exact combined seek/read interface and owned per-entry reader lifetime must
be locked before implementation. Encrypted entries, ZIP64, split archives,
unsupported compression methods and ambiguous duplicate paths should initially
fail explicitly. Writer finalization would validate entry sizes/checksums and
write the central directory; dropping would release resources without finishing.

Both APIs need explicit entry presence and errors, compressed/decompressed byte
ceilings per entry and per archive, an entry-count ceiling, bounded metadata
lengths and nesting policy. Limits include skipped entries. Partial reads are
unverified until the entry checksum and declared size are validated. These
proposed names are package boundaries, not a commitment to signatures.

## Extraction is a separate explicit policy

Entry iteration must never write files automatically. A future extraction API
must require a destination root and a validated policy rather than inherit
archive authority. Proposed defaults:

- Reject absolute paths, drive/UNC/device paths, NULs, `..` components and names
  that normalize outside the root. Define backslash behavior independently of
  the current host, and reject host-equivalent duplicate/colliding names.
- Reject symbolic links, hard links, devices, FIFOs and sockets initially.
  If links are added, validate their targets and all parent traversals under
  the same root policy; do not follow an existing destination symlink.
- Create directories/files with conservative application-selected permissions;
  ignore archive ownership, setuid/setgid and executable bits by default.
  Do not restore timestamps or ACLs implicitly.
- Refuse overwrites by default, including duplicate archive names. An explicit
  overwrite mode needs atomic publication and a defined existing-file policy.
- Enforce counts, metadata and expanded-size limits before creating or growing
  files. Reject a declared size beyond remaining capacity before extraction,
  and enforce the actual size while streaming even when metadata claims less.
- Write regular files to temporary siblings, verify checksum/size, then publish.
  Specify cleanup of temporary files and directories after failure. Never claim
  atomic rollback of already published entries.

String normalization alone cannot prevent filesystem races. Root-relative
no-follow file creation/traversal and atomic publish primitives must be reviewed
in `std/os` before promising secure extraction. On hosts lacking the necessary
capability, reject extraction rather than weaken the policy.

Before an implementation starts, follow spec §53: lock exact signatures,
ownership/entry lifetimes, finalization, errors, plain/async behavior, numeric
limits and platform guarantees. Add interoperable fixtures and adversarial path,
link, overwrite, malformed-metadata, checksum and cleanup conformance tests.
