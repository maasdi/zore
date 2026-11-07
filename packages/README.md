# Maintained packages

This folder holds the external packages that the Zore project maintains and
releases. They are ordinary Zore projects with curated native adapters, not
standard packages: an application opts in by adding a dependency, and nothing
here is imported implicitly.

| Package | Imports | Adapter | Provider |
| --- | --- | --- | --- |
| [`tls`](tls) | `tls`, `tls/https` | `tls-mbedtls-v1` | Mbed TLS 3.6.7 LTS, embedded in the compiler |
| [`sqlite`](sqlite) | `sqlite` | `sqlite-v1` | SQLite 3.54.1 amalgamation, in `sqlite/native/` |

Secure random bytes, hashing, HMAC and constant-time comparison are in the
standard library (`std/crypto/...`). The older `crypto` package in
`examples/crypto/crypto` is not released from here.

## Identity

Each package is identified by this repository's URL, its subdirectory and its
project name:

| Package | `git` | `subdir` | Tag |
| --- | --- | --- | --- |
| `tls` | `https://github.com/maasdi/zore.git` | `packages/tls` | `tls/vX.Y.Z` |
| `sqlite` | `https://github.com/maasdi/zore.git` | `packages/sqlite` | `sqlite/vX.Y.Z` |

Use exactly this URL spelling. Zore does not treat other spellings, mirrors or
forks as the same package.

## Using a release

```toml
name = "myapp"

[dependencies]
tls = { git = "https://github.com/maasdi/zore.git", subdir = "packages/tls", version = "0.1.0-alpha.2" }
sqlite = { git = "https://github.com/maasdi/zore.git", subdir = "packages/sqlite", version = "0.1.0-alpha.2" }
```

```sh
zore install .
git add zore.toml zore.lock
zore check --locked --offline ./main.ore
zore build --locked --offline ./main.ore
zore test --locked --offline .
```

`install` looks up the tag once, verifies the source and records the exact
commit in `zore.lock`. Commit that lock. Later commands, and `install --locked`
on another machine with a populated cache, work offline and never pick a newer
release by themselves. Use `zore outdated .` to see newer releases and
`zore lock .` after raising a version. Nothing needs to be copied out of this
repository.

## Release policy

- Versions follow `MAJOR.MINOR.PATCH`, optionally with an `-alpha.N` suffix.
  Before 1.0.0 a minor bump (0.1 to 0.2) is a breaking change; a patch bump is
  not. `0.1.0-alpha.1` comes before `0.1.0-alpha.2`, which comes before `0.1.0`.
- Each package's version equals the compiler's version in
  `compiler/Cargo.toml`; a test enforces this. A compiler release tags all
  three on one commit, so compiler `v0.1.0-alpha.2` pairs with
  `tls/v0.1.0-alpha.2` and `sqlite/v0.1.0-alpha.2`. The first package release
  ships with compiler `v0.1.0-alpha.2`; earlier compilers cannot read alpha
  package versions.
- A package release is tagged on a commit of this repository's `main` branch,
  after CI passes on Linux, macOS and Windows for that commit.
- The native adapter recipe is part of the compiler. Use a package release with
  the compiler built from the same commit or a later compiler that still
  registers the same adapter recipe; a mismatched recipe fails at install with
  an explicit error rather than building different native code.
- A published tag is never moved or deleted. A fix gets a new version.
- Changing an adapter's reviewed native sources or recipe requires a new
  package version and a compiler update in the same change.

### Cutting a release

The compiler's release PR bumps `compiler/Cargo.toml` and both package
manifests to the new version and refreshes their locks. After it merges and CI
passes on that `main` commit, push the compiler tag and then the package tags
on the same commit:

```sh
version=0.1.0-alpha.2
zore check --release --tag tls/v$version packages/tls
zore check --release --tag sqlite/v$version packages/sqlite
git tag -a v$version -m "Release $version"
git tag -a tls/v$version -m "Release tls $version"
git tag -a sqlite/v$version -m "Release sqlite $version"
git push origin refs/tags/v$version
git push origin refs/tags/tls/v$version refs/tags/sqlite/v$version
```

## Supported hosts

Linux GNU and macOS on x86_64 and aarch64, and Windows x64 with MSVC, using
the normal clang and Rust toolchain. `zore check` needs neither. The CI
workflow lints each package's native bridge and runs its conformance suite,
including installing a tagged release from an offline cache, on all three
hosts.

## Future direction

The long-term goal is to implement TLS and SQLite in Zore itself, so these
packages no longer depend on C providers or compiler-registered native
recipes. Until then they stay here, next to the compiler that registers their
adapters, so a provider fix and its recipe change ship in one reviewed change.

## Provider updates and advisories

Maintainers watch upstream security advisories for Mbed TLS and SQLite. When a
fix is needed:

1. Update the reviewed provider source, its SHA-256 records and provenance
   notes, and the adapter recipe in the compiler.
2. Run the package's known-answer, failure and host tests on all three hosts.
3. Raise the package version, release a compiler with the new recipe and tag
   the package from the same commit.
4. Announce the affected and fixed versions in the release notes and through
   the repository's security advisory process.

Locks never upgrade on their own. Applications pick up a fix by raising the
dependency version and running `zore lock`. Report suspected vulnerabilities
privately as described in the repository's security policy.
