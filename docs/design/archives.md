# Remaining archive work

Issue [#224](https://github.com/maasdi/zore/issues/224) provides gzip, USTAR,
stored/DEFLATE ZIP with ZIP64, and confined new-directory extraction on Linux/macOS/Windows.
See the [archive guide](../archives.md) and spec §§37.26–37.30, §§37.33–37.35.

Windows extraction now uses handle-relative no-reparse operations, protected
owner/LocalSystem DACLs, filesystem identity checks, handle-based cleanup and
atomic no-replace publication. Its native and archive tests cover reparse
points, aliases, ACL inheritance, races and cleanup. Future platform extensions
must preserve these guarantees and fail closed when a capability is unavailable.

ZIP64 is read and written up to 64 GiB and 1,000,000 entries (§37.35).
Encryption, split archives, CP437 names and more permissive central/local
ordering remain excluded. Extensions must retain bounded directory memory,
unambiguous offsets, exact size/CRC validation and explicit entry lifetimes.
PAX and GNU long names are read and PAX names are written (§37.33). Links are
reported, never written, and extracted only on request when they provably stay
inside the new tree; executable bits are restored as owner-only execute
(§37.34). Windows symbolic links would need a privilege policy. Sparse files and special nodes need separately
locked metadata and expansion limits.

Extraction currently publishes a new directory only. Merging into existing
trees, overwrite modes, metadata restoration and link handling need separate
policies and adversarial race/alias tests. A successful archive read does not
authorize overwriting application files. Keep source parsing, portable path
syntax and confined filesystem operations in their owning packages.
