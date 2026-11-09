# Remaining archive work

Issue [#224](https://github.com/maasdi/zore/issues/224) provides gzip, USTAR,
classic stored/DEFLATE ZIP, and confined new-directory extraction on Linux/macOS.
See the [archive guide](../archives.md) and spec §§37.26–37.30.

Windows extraction needs a verified handle-relative provider: reject reparse
points at each component, track filesystem identities to reject name aliases,
create owner-only staging, publish without replacement using directory handles,
and clean up without following links. The current provider returns unsupported;
no path-based fallback weakens confinement.

ZIP64, encryption, split archives, CP437 names and more permissive central/local
ordering remain excluded. Extensions must retain bounded directory memory,
unambiguous offsets, exact size/CRC validation and explicit entry lifetimes.
GNU/PAX long names, sparse files, links and special nodes need separately locked
metadata precedence and byte/count/expansion limits.

Extraction currently publishes a new directory only. Merging into existing
trees, overwrite modes, metadata restoration and link handling need separate
policies and adversarial race/alias tests. A successful archive read does not
authorize overwriting application files. Keep source parsing, portable path
syntax and confined filesystem operations in their owning packages.
