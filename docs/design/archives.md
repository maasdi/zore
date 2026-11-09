# Remaining archive work

The first implementation of [#224](https://github.com/maasdi/zore/issues/224)
provides gzip in §37.25 and `std/archive` USTAR/stored ZIP I/O in §37.27.
The [archive guide](../archives.md) describes the implemented APIs. Archive
readers return metadata and bytes, never filesystem effects. `Sink()` retains
the concrete writer type for ordered finalization through compression layers.

## Format follow-ups

ZIP DEFLATE needs a separately locked raw-DEFLATE primitive below gzip and
archives, preserving compressed/decompressed limits and checksum handling.
ZIP64, encryption, split archives, CP437 names and more permissive central/local
ordering remain excluded. Adding support must preserve unambiguous offsets,
entry lifetimes, CRC32/size validation and bounded directory memory.

USTAR currently rejects GNU/PAX long names, sparse files, links and special
nodes. Any extension needs explicit size/count/metadata limits, precedence
between extension and ordinary headers, and interoperability/adversarial tests.
No metadata extension may silently change entry names, sizes or type policy.

## Extraction is a separate explicit policy

A future extraction API must require a destination root and validated policy:

- Reject absolute paths, drive/UNC/device paths, NULs, `..` components and names
  that normalize outside the root. `ValidName` supplies conservative syntax;
  filesystem-equivalent collisions and root-relative traversal need additional
  checks independent of the host spelling of separators.
- Reject symbolic links, hard links, devices, FIFOs and sockets initially.
  If links are added, validate targets and all parent traversals under the same
  root policy. Never follow an existing destination symlink implicitly.
- Create directories/files with conservative application-selected permissions;
  ignore archive ownership, setuid/setgid and executable bits by default.
  Do not restore timestamps or ACLs implicitly.
- Refuse overwrites and duplicate destinations by default. Reading duplicate
  metadata entries does not authorize overwriting files. An explicit overwrite
  mode needs atomic publication and a defined existing-file policy.
- Enforce entry, metadata and expanded-size limits before creating or growing
  files, and enforce actual sizes while streaming when metadata claims less.
- Write regular files to temporary siblings, verify checksum/size, then publish.
  Define cleanup of temporary files/directories after failure without promising
  rollback of already published entries.

String validation cannot prevent filesystem races. Root-relative no-follow
creation/traversal and atomic publication primitives must be reviewed in
`std/os` before promising safe extraction. On hosts lacking the necessary
capability, reject extraction rather than weaken policy.

Before implementation, follow spec §53: lock signatures, ownership, failure
cleanup, numeric bounds, plain/async behavior and platform guarantees. Include
external fixtures and adversarial path, link, overwrite and race tests.
