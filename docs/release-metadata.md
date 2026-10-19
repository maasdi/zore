# Release metadata

`zore check --release ./library` validates the nearest `zore.toml` locally. It is
an optional author preflight, available without Git, clang or a native linker.
Ordinary Git tagging/pushing works independently of this helper.

Only name, canonical version and licensing are required:

```toml
name = "greeting"
version = "1.2.3"
license = "MIT OR Apache-2.0"
```

Versions have exactly three decimal components without leading zeroes, signs,
whitespace or suffixes. `0.1.0` is valid; `v1.2.3`, `01.2.3`, `1.2`, prereleases,
build suffixes and ranges are not. Components compare numerically without a
fixed integer-width limit. The release tag convention is `greeting/v1.2.3`.

The `license` field accepts recognized SPDX identifiers, including deprecated
GNU forms such as `GPL-2.0+` and `LGPL-2.1+`. Expressions support `AND`, `OR`,
`WITH` (or their lowercase forms) and parentheses. For custom terms, use
`license-file = "COPYING.txt"` instead. The file must contain nonempty
UTF-8 text. Both fields may be supplied; both must validate. Custom LicenseRef
and AdditionRef expressions use a license file instead. Retain applicable
license/notice contents in the committed source.

## Optional information

```toml
description = "A small greeting library"
readme = "README.md"
repository = "https://example.org/team/libraries.git"
documentation = "https://example.org/greeting/"
zore-min = "0.1.0"
zore-max = "0.2.0"
platforms = ["linux", "macos", "windows"]
```

Description must be nonempty text without control characters. Readme and license
paths must be portable, relative paths to readable regular UTF-8 files within
the canonical project root. Parent/dot components, absolute paths, backslashes,
Windows reserved names and symlinks escaping the root are rejected. An internal
symlink may work locally; Git source distribution still rejects all symlinks.

Repository links follow the existing explicit, credential-free Git URL grammar.
Documentation links use HTTPS with an ASCII DNS/IPv4 host or bracketed IPv6
address and optional numeric port. Credentials, whitespace, controls, backslashes,
query strings and fragments are rejected. These links are descriptive: the actual
Git dependency URL remains authoritative for source identity and attribution.
No link is fetched during metadata validation.

`zore-min` is inclusive; `zore-max` is exclusive. Both use canonical versions;
if both occur, minimum must be less than maximum. Platforms are a nonempty list
of distinct `linux`, `macos`, `windows` names; omission imposes no restriction.
Supplied bounds and platforms are enforced against the current compiler/host
for every project, including unused dependencies. They describe declared support;
they do not prove test results or enable cross-compilation. Native adapters retain
their separate reviewed source, recipe and build-time host/ABI requirements.

Other metadata is optional. The release helper warns if both description and
readme are missing; ordinary application commands produce no omission warnings.
Examples, tests and test instructions are encouraged without becoming new gates.

## What the helper checks

```sh
zore check --release --offline --tag greeting/v1.2.3 ./library
```

The supplied tag's name and version must match the manifest exactly. `--tag` is
optional and checks a string only: the helper never looks up a Git ref, checks
that a tag points at this working tree, creates tags or pushes. Its success
message reports the canonical tag and explicitly states that it was not looked up.

The helper validates only the selected project's metadata and, if declared, its
existing curated native source/recipe. It does not typecheck `.ore` files, resolve
dependencies, execute source/tests/build scripts, fetch anything, or write lockfiles
or caches. `--offline` is accepted; no network is used either way. `--locked` is
rejected because this mode does not validate a lock. The flags apply only to
`check`; repeated flags and `--tag` without `--release` are usage errors.

Check/test source independently before committing and tagging, for example:

```sh
zore check ./library/library.ore
zore test ./library
```

The [greeting sample](../examples/release-metadata) demonstrates the helper and
source checks together. Metadata failure exits 1; invalid command usage exits 2.

## Existing projects and follow-ups

Local projects and full-revision Git dependencies do not need release versions or
licenses. Their existing `version` strings remain unchecked unless release
validation is explicitly requested. Supplied new metadata is always validated.
Version-1 locks, exact manifest-byte digests, commit/tree verification and curated
native recipe fingerprints are unchanged; no new source digest is introduced.
Changing manifest bytes still requires the ordinary explicit lock update.

The reusable compiler interface is `zore::driver::release`: `Version` and `Tag`
parse canonical identities, `Metadata::validate_support` accepts an explicit
compiler version/host, `validate_metadata` checks supplied fields/files, and
`validate` checks release requirements plus an optional expected tag. Filesystem
access uses the existing `FileSystem` abstraction. SPDX parsing adds the compiler-only
`spdx` crate with default features disabled; its recognized identifier database
and expression parser avoid a separate, incomplete license list or grammar.
Generated Zore programs and the runtime acquire no dependency.

[Tagged dependencies](versioned-dependencies.md) (#145) add tag lookup,
version-to-commit mapping, compatible minimum-version dependency selection and
version-2 lock/offline support. The helper above stays local and metadata-only.
The [release workflow](release-workflow.md) covers ordinary Git publication,
consumer validation and read-only update discovery. The local helper alone
does not establish a hosted release or whole dependency-graph readiness.
